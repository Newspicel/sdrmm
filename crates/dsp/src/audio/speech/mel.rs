use num_complex::Complex;

use super::SpeechError;
use crate::math::fft::RealTransform;

const LOG_GUARD: f32 = 5.960_464_5e-8;
const NORM_EPSILON: f32 = 1e-5;
const SLANEY_LINEAR_HZ: f64 = 1000.0;
const SLANEY_LINEAR_MEL: f64 = 15.0;

/// The log-mel front end: pre-emphasis, a centred STFT and per-band normalisation over the clip.
pub(super) struct LogMel {
    bands: usize,
    hop: usize,
    preemphasis: f32,
    window: Vec<f32>,
    filters: Vec<Vec<(usize, f32)>>,
    fft: RealTransform,
}

/// Normalised log-mel frames, `bands` values each; frames at and past `valid` are zero.
pub(super) struct Features {
    pub frames: Vec<f32>,
    pub count: usize,
    pub valid: usize,
}

impl LogMel {
    pub fn new(
        rate: u32,
        bands: usize,
        (n_fft, win, hop): (usize, usize, usize),
        preemphasis: f32,
    ) -> Self {
        let mut window = vec![0.0; n_fft];
        let offset = (n_fft - win) / 2;
        for (i, value) in window[offset..offset + win].iter_mut().enumerate() {
            let phase = 2.0 * std::f64::consts::PI * i as f64 / (win - 1) as f64;
            *value = (0.5 - 0.5 * phase.cos()) as f32;
        }
        Self {
            bands,
            hop,
            preemphasis,
            window,
            filters: slaney_filters(f64::from(rate), n_fft, bands),
            fft: RealTransform::new(n_fft),
        }
    }

    pub fn features(&mut self, pcm: &[f32]) -> Result<Features, SpeechError> {
        let valid = pcm.len() / self.hop;
        if valid < 2 {
            return Err(SpeechError::Audio("clip too short".into()));
        }
        let padded = self.padded(pcm);
        let count = 1 + pcm.len() / self.hop;
        let n_fft = self.window.len();
        let mut frame = vec![0.0; n_fft];
        let mut spectrum = vec![Complex::new(0.0, 0.0); n_fft / 2 + 1];
        let mut power = vec![0.0; n_fft / 2 + 1];
        let mut frames = vec![0.0; count * self.bands];
        for (index, out) in frames.chunks_exact_mut(self.bands).enumerate() {
            let start = index * self.hop;
            for ((dst, &x), &w) in frame
                .iter_mut()
                .zip(&padded[start..start + n_fft])
                .zip(&self.window)
            {
                *dst = x * w;
            }
            self.fft
                .forward(&mut frame, &mut spectrum)
                .map_err(|error| SpeechError::Audio(error.to_string()))?;
            for (p, bin) in power.iter_mut().zip(&spectrum) {
                *p = bin.norm_sqr();
            }
            for (value, filter) in out.iter_mut().zip(&self.filters) {
                let energy: f32 = filter.iter().map(|&(bin, gain)| gain * power[bin]).sum();
                *value = (energy + LOG_GUARD).ln();
            }
        }
        normalise(&mut frames, self.bands, valid);
        Ok(Features {
            frames,
            count,
            valid,
        })
    }

    fn padded(&self, pcm: &[f32]) -> Vec<f32> {
        let half = self.window.len() / 2;
        let mut padded = vec![0.0; pcm.len() + 2 * half];
        let mut previous = 0.0;
        for (index, (dst, &x)) in padded[half..].iter_mut().zip(pcm).enumerate() {
            *dst = if index == 0 {
                x
            } else {
                x - self.preemphasis * previous
            };
            previous = x;
        }
        padded
    }
}

fn normalise(frames: &mut [f32], bands: usize, valid: usize) {
    for band in 0..bands {
        let values = || frames[..valid * bands].iter().skip(band).step_by(bands);
        let mean = values().sum::<f32>() / valid as f32;
        let var = values().map(|&x| (x - mean) * (x - mean)).sum::<f32>() / (valid - 1) as f32;
        let scale = (var.sqrt() + NORM_EPSILON).recip();
        for value in frames[..valid * bands].iter_mut().skip(band).step_by(bands) {
            *value = (*value - mean) * scale;
        }
    }
    frames[valid * bands..].fill(0.0);
}

fn hz_to_mel(hz: f64) -> f64 {
    let step = 27.0 / 6.4f64.ln();
    if hz < SLANEY_LINEAR_HZ {
        3.0 * hz / 200.0
    } else {
        SLANEY_LINEAR_MEL + (hz / SLANEY_LINEAR_HZ).ln() * step
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    let step = 6.4f64.ln() / 27.0;
    if mel < SLANEY_LINEAR_MEL {
        200.0 * mel / 3.0
    } else {
        SLANEY_LINEAR_HZ * ((mel - SLANEY_LINEAR_MEL) * step).exp()
    }
}

/// Slaney-scale triangles with Slaney area normalisation, each kept as its non-zero bins.
fn slaney_filters(rate: f64, n_fft: usize, bands: usize) -> Vec<Vec<(usize, f32)>> {
    let bins = n_fft / 2 + 1;
    let top = hz_to_mel(rate / 2.0);
    let edges: Vec<f64> = (0..bands + 2)
        .map(|i| mel_to_hz(top * i as f64 / (bands + 1) as f64))
        .collect();
    let bin_hz = |bin: usize| rate / 2.0 * bin as f64 / (bins - 1) as f64;
    (0..bands)
        .map(|band| {
            let (low, centre, high) = (edges[band], edges[band + 1], edges[band + 2]);
            let norm = 2.0 / (high - low);
            (0..bins)
                .filter_map(|bin| {
                    let hz = bin_hz(bin);
                    let down = (hz - low) / (centre - low);
                    let up = (high - hz) / (high - centre);
                    let gain = down.min(up).max(0.0) * norm;
                    (gain > 0.0).then_some((bin, gain as f32))
                })
                .collect()
        })
        .collect()
}
