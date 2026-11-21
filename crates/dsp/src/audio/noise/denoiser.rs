use num_complex::Complex;
use realfft::FftError;

use crate::math::fft::RealTransform;
use crate::math::special::exponential_integral;
use crate::{math::db::Decibels, math::window::hann};

/// Weights of a bin and its two neighbours when the noise tracker smooths across frequency.
const BIN_SMOOTHING: [f32; 3] = [0.25, 0.5, 0.25];
/// Frames of input the pending buffer holds before it must grow.
const PENDING_FRAMES: usize = 4;

pub struct SpectralDenoiser {
    fft: RealTransform,
    window: Vec<f32>,
    pending: Vec<f32>,
    frame: Vec<f32>,
    overlap: Vec<f32>,
    ready: Vec<f32>,
    read: usize,
    power: Vec<f32>,
    smoothed: Vec<f32>,
    minimum: Vec<f32>,
    running_min: Vec<f32>,
    presence: Vec<f32>,
    noise: Vec<f32>,
    prior_snr: Vec<f32>,
    gains: Vec<f32>,
    min_age: usize,
    frames: u32,
    floor: f32,
    log_floor: f32,
    spectrum: Vec<Complex<f32>>,
}

impl SpectralDenoiser {
    pub const FRAME: usize = 512;
    pub const HOP: usize = Self::FRAME / 2;
    const BINS: usize = Self::FRAME / 2 + 1;
    const MAX_ATTENUATION_DB: f32 = 20.0;
    const WARMUP_FRAMES: u32 = 8;
    const POWER_RETENTION: f32 = 0.8;
    const PRESENCE_RETENTION: f32 = 0.2;
    const NOISE_RETENTION: f32 = 0.85;
    const SNR_RETENTION: f32 = 0.95;
    const MINIMUM_WINDOW: usize = 256;
    const MINIMUM_BIAS: f32 = 2.5;
    const PRESENCE_RATIO: f32 = 5.0;
    const MIN_PRIOR_SNR: f32 = 0.003;
    const MAX_POST_SNR: f32 = 1.0e3;
    const MIN_NU: f32 = 1.0e-6;
    const MAX_NU: f32 = 500.0;
    const MIN_ABSENCE: f32 = 0.02;
    const MAX_ABSENCE: f32 = 0.95;

    #[must_use]
    pub fn new(strength: f32) -> Self {
        let mut denoiser = Self {
            fft: RealTransform::new(Self::FRAME),
            window: hann(Self::FRAME).iter().map(|w| w.sqrt()).collect(),
            pending: Vec::with_capacity(Self::FRAME * PENDING_FRAMES),
            frame: vec![0.0; Self::FRAME],
            overlap: vec![0.0; Self::FRAME],
            ready: vec![0.0; Self::FRAME],
            read: 0,
            power: vec![0.0; Self::BINS],
            smoothed: vec![0.0; Self::BINS],
            minimum: vec![0.0; Self::BINS],
            running_min: vec![0.0; Self::BINS],
            presence: vec![0.0; Self::BINS],
            noise: vec![0.0; Self::BINS],
            prior_snr: vec![0.0; Self::BINS],
            gains: vec![1.0; Self::BINS],
            min_age: 0,
            frames: 0,
            floor: 1.0,
            log_floor: 0.0,
            spectrum: vec![Complex::new(0.0, 0.0); Self::BINS],
        };
        denoiser.set_strength(strength);
        denoiser
    }

    pub fn set_strength(&mut self, strength: f32) {
        let strength = strength.clamp(0.0, 1.0);
        self.floor = (-strength * Self::MAX_ATTENUATION_DB).db_to_amplitude();
        self.log_floor = self.floor.ln();
    }

    #[must_use]
    pub fn latency(&self) -> usize {
        Self::FRAME
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.overlap.fill(0.0);
        self.ready.clear();
        self.ready.resize(Self::FRAME, 0.0);
        self.read = 0;
        self.power.fill(0.0);
        self.smoothed.fill(0.0);
        self.minimum.fill(0.0);
        self.running_min.fill(0.0);
        self.presence.fill(0.0);
        self.noise.fill(0.0);
        self.prior_snr.fill(0.0);
        self.gains.fill(1.0);
        self.min_age = 0;
        self.frames = 0;
    }

    pub fn process(&mut self, samples: &mut [f32]) -> Result<(), FftError> {
        if self.read > 0 {
            self.ready.drain(..self.read.min(self.ready.len()));
            self.read = 0;
        }
        self.pending
            .extend(samples.iter().map(|s| if s.is_finite() { *s } else { 0.0 }));
        let mut consumed = 0;
        let mut result = Ok(());
        while self.pending.len() - consumed >= Self::FRAME {
            result = result.and(self.transform(consumed));
            consumed += Self::HOP;
        }
        self.pending.drain(..consumed);
        for s in samples.iter_mut() {
            *s = self.ready.get(self.read).copied().unwrap_or(0.0);
            self.read += 1;
        }
        result
    }

    fn transform(&mut self, offset: usize) -> Result<(), FftError> {
        let frame = &self.pending[offset..offset + Self::FRAME];
        for ((dst, &s), &w) in self.frame.iter_mut().zip(frame).zip(&self.window) {
            *dst = s * w;
        }
        let spectral = self.filter_frame();
        if spectral.is_err() {
            self.frame.fill(0.0);
        }

        let norm = 1.0 / Self::FRAME as f32;
        for ((slot, &sample), &w) in self.overlap.iter_mut().zip(&self.frame).zip(&self.window) {
            *slot += sample * norm * w;
        }
        self.ready.extend_from_slice(&self.overlap[..Self::HOP]);
        self.overlap.copy_within(Self::HOP.., 0);
        self.overlap[Self::FRAME - Self::HOP..].fill(0.0);
        spectral
    }

    fn filter_frame(&mut self) -> Result<(), FftError> {
        self.fft.forward(&mut self.frame, &mut self.spectrum)?;
        self.shape();
        self.fft.inverse(&mut self.spectrum, &mut self.frame)
    }

    fn shape(&mut self) {
        for (slot, bin) in self.power.iter_mut().zip(&self.spectrum) {
            *slot = bin.norm_sqr();
        }
        self.track_noise();
        self.update_gains();
        self.frames = self.frames.saturating_add(1);
        for (bin, &gain) in self.spectrum.iter_mut().zip(&self.gains) {
            *bin *= gain;
        }
    }

    fn track_noise(&mut self) {
        let last = Self::BINS - 1;
        self.min_age += 1;
        let rotate = self.min_age >= Self::MINIMUM_WINDOW;
        let warming = self.frames < Self::WARMUP_FRAMES;
        let blend = 1.0 / (self.frames + 1) as f32;
        for k in 0..Self::BINS {
            let local = BIN_SMOOTHING[0] * self.power[k.saturating_sub(1)]
                + BIN_SMOOTHING[1] * self.power[k]
                + BIN_SMOOTHING[2] * self.power[(k + 1).min(last)];
            if warming {
                self.smoothed[k] = local;
                self.minimum[k] = local;
                self.running_min[k] = local;
                self.presence[k] = 0.0;
                self.noise[k] += blend * (self.power[k] - self.noise[k]);
                self.noise[k] = self.noise[k].min(Self::MINIMUM_BIAS * local);
                continue;
            }
            self.smoothed[k] =
                Self::POWER_RETENTION * self.smoothed[k] + (1.0 - Self::POWER_RETENTION) * local;
            let smoothed = self.smoothed[k];
            if rotate {
                self.minimum[k] = smoothed.min(self.running_min[k]);
                self.running_min[k] = smoothed;
            } else {
                self.minimum[k] = self.minimum[k].min(smoothed);
                self.running_min[k] = self.running_min[k].min(smoothed);
            }
            let excess = smoothed / self.minimum[k].max(f32::MIN_POSITIVE);
            let detected = if excess > Self::PRESENCE_RATIO {
                1.0
            } else {
                0.0
            };
            self.presence[k] = Self::PRESENCE_RETENTION * self.presence[k]
                + (1.0 - Self::PRESENCE_RETENTION) * detected;
            let retention =
                Self::NOISE_RETENTION + (1.0 - Self::NOISE_RETENTION) * self.presence[k];
            self.noise[k] = (retention * self.noise[k] + (1.0 - retention) * self.power[k])
                .min(Self::MINIMUM_BIAS * self.minimum[k]);
        }
        if rotate {
            self.min_age = 0;
        }
    }

    fn update_gains(&mut self) {
        if self.frames < Self::WARMUP_FRAMES {
            self.gains.fill(1.0);
            for k in 0..Self::BINS {
                self.prior_snr[k] = self.posterior_snr(k);
            }
            return;
        }
        for k in 0..Self::BINS {
            let posterior = self.posterior_snr(k);
            let prior = (Self::SNR_RETENTION * self.prior_snr[k]
                + (1.0 - Self::SNR_RETENTION) * (posterior - 1.0).max(0.0))
            .max(Self::MIN_PRIOR_SNR);
            let ratio = prior / (1.0 + prior);
            let nu = (ratio * posterior).clamp(Self::MIN_NU, Self::MAX_NU);
            let lsa = (ratio * (0.5 * exponential_integral(nu)).exp()).clamp(self.floor, 1.0);
            let absence = (1.0 - self.presence[k]).clamp(Self::MIN_ABSENCE, Self::MAX_ABSENCE);
            let odds = absence / (1.0 - absence) * (1.0 + prior) * (-nu).exp();
            let speech = 1.0 / (1.0 + odds);
            let gain = (speech * lsa.ln() + (1.0 - speech) * self.log_floor).exp();
            self.prior_snr[k] = gain * gain * posterior;
            self.gains[k] = gain;
        }
    }

    fn posterior_snr(&self, bin: usize) -> f32 {
        (self.power[bin] / self.noise[bin].max(f32::MIN_POSITIVE)).clamp(0.0, Self::MAX_POST_SNR)
    }
}

#[cfg(test)]
mod tests;
