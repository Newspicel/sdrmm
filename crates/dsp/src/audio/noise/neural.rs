//! Speech enhancement by a recurrent network over STFT frames, the model handed in as bytes.

use std::sync::Arc;

use crate::{
    RealDecimator, RealInterpolator, design_lowpass,
    math::fft::RealTransform,
    math::neural::{Net, NetError, Session},
};
use num_complex::Complex;
use realfft::FftError;

const TAPS_PER_FACTOR: usize = 32;
const CUTOFF_PER_FACTOR: f64 = 0.45;
const MODEL_DELAY_FRAMES: usize = 4;
const MAX_ATTENUATION_DB: f32 = 30.0;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("denoiser unavailable: {0}")]
pub struct NeuralDenoiseError(pub String);

impl From<FftError> for NeuralDenoiseError {
    fn from(error: FftError) -> Self {
        Self(error.to_string())
    }
}

impl From<NetError> for NeuralDenoiseError {
    fn from(error: NetError) -> Self {
        Self(error.to_string())
    }
}

struct Shape {
    window_len: usize,
    hop: usize,
    factor: usize,
}

impl Shape {
    fn of(net: &Net, audio_rate: u32) -> Result<Self, NeuralDenoiseError> {
        let window_len: usize = parse(net, "n_fft")?;
        let hop: usize = parse(net, "hop_length")?;
        let rate: u32 = parse(net, "sample_rate")?;
        let factor = (audio_rate / rate.max(1)) as usize;
        let bins = window_len / 2 + 1;
        let fits = hop > 0
            && hop <= window_len
            && factor >= 1
            && audio_rate == rate * factor as u32
            && net.input_len(0) == Some(2 * bins)
            && net.output_len(0) == Some(2 * bins)
            && net.input_len(1) == net.output_len(1);
        if !fits {
            return Err(NeuralDenoiseError("model does not fit the denoiser".into()));
        }
        Ok(Self {
            window_len,
            hop,
            factor,
        })
    }
}

fn parse<T: std::str::FromStr>(net: &Net, key: &str) -> Result<T, NeuralDenoiseError> {
    net.meta(key)
        .and_then(|value| value.trim().parse().ok())
        .ok_or_else(|| NeuralDenoiseError(format!("model metadata lacks {key}")))
}

fn floats(net: &Net, key: &str) -> Result<Vec<f32>, NeuralDenoiseError> {
    net.meta(key)
        .ok_or_else(|| NeuralDenoiseError(format!("model metadata lacks {key}")))?
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse()
                .map_err(|_| NeuralDenoiseError(format!("model metadata {key} is not numeric")))
        })
        .collect()
}

/// The recurrent state a fresh stream starts from, from the normalisation the model ships.
pub fn initial_state(net: &Net) -> Result<Vec<f32>, NeuralDenoiseError> {
    let size = net
        .input_len(1)
        .ok_or_else(|| NeuralDenoiseError("model takes no state".into()))?;
    let erb_norm = floats(net, "erb_norm_init")?;
    let spec_norm = floats(net, "spec_norm_init")?;
    if erb_norm.len() + spec_norm.len() > size {
        return Err(NeuralDenoiseError("model state is too small".into()));
    }
    let mut state = vec![0.0; size];
    state[..erb_norm.len()].copy_from_slice(&erb_norm);
    state[erb_norm.len()..erb_norm.len() + spec_norm.len()].copy_from_slice(&spec_norm);
    Ok(state)
}

pub struct NeuralDenoiser {
    session: Session,
    initial_state: Vec<f32>,
    window_len: usize,
    hop: usize,
    factor: usize,
    fft: RealTransform,
    window: Vec<f32>,
    decimator: RealDecimator,
    interpolator: RealInterpolator,
    narrow: Vec<f32>,
    wide: Vec<f32>,
    frame_in: Vec<f32>,
    frame_out: Vec<f32>,
    overlap: Vec<f32>,
    frame: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    noisy: Vec<Vec<Complex<f32>>>,
    noisy_head: usize,
    ready: std::collections::VecDeque<f32>,
    dry_mix: f32,
}

impl NeuralDenoiser {
    /// A denoiser running `net` on audio at `audio_rate`, a whole multiple of the model's rate.
    pub fn new(net: Arc<Net>, audio_rate: u32, strength: f32) -> Result<Self, NeuralDenoiseError> {
        let Shape {
            window_len,
            hop,
            factor,
        } = Shape::of(&net, audio_rate)?;
        let initial_state = initial_state(&net)?;
        let bins = window_len / 2 + 1;
        let taps = design_lowpass(TAPS_PER_FACTOR * factor, CUTOFF_PER_FACTOR / factor as f64);
        let mut denoiser = Self {
            session: Session::new(net),
            initial_state,
            window_len,
            hop,
            factor,
            fft: RealTransform::new(window_len),
            window: vorbis(window_len),
            decimator: RealDecimator::new(&taps, factor),
            interpolator: RealInterpolator::new(&taps, factor),
            narrow: Vec::with_capacity(window_len * 4),
            wide: Vec::with_capacity(hop * factor),
            frame_in: Vec::with_capacity(window_len * 4),
            frame_out: vec![0.0; hop],
            overlap: vec![0.0; window_len],
            frame: vec![0.0; window_len],
            spectrum: vec![Complex::new(0.0, 0.0); bins],
            noisy: vec![vec![Complex::new(0.0, 0.0); bins]; MODEL_DELAY_FRAMES + 1],
            noisy_head: 0,
            ready: std::collections::VecDeque::with_capacity(window_len * factor * 4),
            dry_mix: 0.0,
        };
        denoiser.set_strength(strength);
        denoiser.reset();
        Ok(denoiser)
    }

    #[must_use]
    pub fn net(&self) -> &Arc<Net> {
        self.session.net()
    }

    pub fn set_strength(&mut self, strength: f32) {
        let strength = strength.clamp(0.0, 1.0);
        self.dry_mix = 10f32.powf(-strength * MAX_ATTENUATION_DB / 20.0);
    }

    #[must_use]
    pub fn latency(&self) -> usize {
        (self.window_len + MODEL_DELAY_FRAMES * self.hop) * self.factor + self.resampler_delay()
    }

    fn resampler_delay(&self) -> usize {
        if self.factor == 1 {
            0
        } else {
            TAPS_PER_FACTOR * self.factor - 1
        }
    }

    pub fn reset(&mut self) {
        self.session
            .input_mut(1)
            .copy_from_slice(&self.initial_state);
        self.decimator.reset();
        self.interpolator.reset();
        self.frame_in.clear();
        self.overlap.fill(0.0);
        for frame in &mut self.noisy {
            frame.fill(Complex::new(0.0, 0.0));
        }
        self.ready.clear();
        self.ready.resize(self.window_len * self.factor, 0.0);
    }

    pub fn process(&mut self, pcm: &mut [f32]) -> Result<(), NeuralDenoiseError> {
        if self.factor == 1 {
            self.frame_in.extend_from_slice(pcm);
        } else {
            self.decimator.process(pcm, &mut self.narrow);
            self.frame_in.extend_from_slice(&self.narrow);
        }
        let mut consumed = 0;
        while self.frame_in.len() - consumed >= self.window_len {
            self.run_frame(consumed)?;
            if self.factor == 1 {
                self.ready.extend(&self.frame_out);
            } else {
                self.interpolator.process(&self.frame_out, &mut self.wide);
                self.ready.extend(&self.wide);
            }
            consumed += self.hop;
        }
        self.frame_in.drain(..consumed);
        for sample in pcm.iter_mut() {
            *sample = self.ready.pop_front().unwrap_or(0.0);
        }
        Ok(())
    }

    fn run_frame(&mut self, start: usize) -> Result<(), NeuralDenoiseError> {
        let len = self.window_len;
        let bins = self.spectrum.len();
        for ((slot, &x), &w) in self
            .frame
            .iter_mut()
            .zip(&self.frame_in[start..start + len])
            .zip(&self.window)
        {
            *slot = x * w;
        }
        self.fft.forward(&mut self.frame, &mut self.spectrum)?;
        self.noisy_head = (self.noisy_head + 1) % self.noisy.len();
        self.noisy[self.noisy_head].copy_from_slice(&self.spectrum);
        self.infer();
        let delayed = &self.noisy[(self.noisy_head + 1) % self.noisy.len()];
        let wet = 1.0 - self.dry_mix;
        let enhanced = self.session.output(0);
        for (bin, slot) in self.spectrum.iter_mut().enumerate() {
            let model = Complex::new(enhanced[2 * bin], enhanced[2 * bin + 1]);
            *slot = delayed[bin] * self.dry_mix + model * wet;
        }
        self.spectrum[0].im = 0.0;
        if len.is_multiple_of(2) {
            self.spectrum[bins - 1].im = 0.0;
        }
        self.fft.inverse(&mut self.spectrum, &mut self.frame)?;
        let scale = 1.0 / len as f32;
        for ((acc, &value), &w) in self.overlap.iter_mut().zip(&self.frame).zip(&self.window) {
            *acc += value * scale * w;
        }
        let hop = self.hop;
        self.frame_out.copy_from_slice(&self.overlap[..hop]);
        self.overlap.copy_within(hop.., 0);
        self.overlap[len - hop..].fill(0.0);
        Ok(())
    }

    fn infer(&mut self) {
        for (pair, value) in self
            .session
            .input_mut(0)
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&self.spectrum)
        {
            pair[0] = value.re;
            pair[1] = value.im;
        }
        self.session.run();
        self.session.carry(1, 1);
    }
}

fn vorbis(len: usize) -> Vec<f32> {
    let half = len as f32 / 2.0;
    (0..len)
        .map(|n| {
            let s = (0.5 * std::f32::consts::PI * (n as f32 + 0.5) / half).sin();
            (0.5 * std::f32::consts::PI * s * s).sin()
        })
        .collect()
}
