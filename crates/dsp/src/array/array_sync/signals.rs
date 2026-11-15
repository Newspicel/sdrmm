use std::f64::consts::TAU;

use num_complex::Complex;

use super::{FIT_BAND, frequency::bin_frequency, line_fit::LineFit};
use crate::{
    math::complex::{narrow, widen},
    math::fft::{FftPair, Transform},
    math::rng::Rng,
    math::window::hann_weight,
};

/// Unit-power complex white Gaussian noise.
pub(super) struct Gaussian(Rng);

impl Gaussian {
    pub(super) fn new(seed: u64) -> Self {
        Self(Rng::mixed(seed))
    }

    pub(super) fn sample(&mut self) -> Complex<f32> {
        narrow(self.0.complex_normal())
    }

    pub(super) fn block(&mut self, len: usize) -> Vec<Complex<f32>> {
        (0..len).map(|_| self.sample()).collect()
    }
}

pub(super) fn shaped(
    input: &[Complex<f32>],
    response: impl Fn(f64) -> Complex<f64>,
) -> Vec<Complex<f32>> {
    let len = input.len();
    let mut fft = FftPair::<f64>::new(len);
    let mut spectrum: Vec<Complex<f64>> = input.iter().copied().map(widen).collect();
    fft.forward(&mut spectrum);
    for (bin, value) in spectrum.iter_mut().enumerate() {
        *value *= response(bin_frequency(bin, len));
    }
    fft.inverse(&mut spectrum);
    spectrum
        .iter()
        .map(|value| narrow(value / len as f64))
        .collect()
}

pub(super) fn lane_response(delay: f64, phase_rad: f64, gain: f64) -> impl Fn(f64) -> Complex<f64> {
    move |nu| Complex::from_polar(gain, phase_rad - TAU * nu * delay)
}

pub(super) fn with_noise(
    input: &[Complex<f32>],
    amplitude: f32,
    noise: &mut Gaussian,
) -> Vec<Complex<f32>> {
    input
        .iter()
        .map(|value| value + noise.sample() * amplitude)
        .collect()
}

pub(super) fn delay_between(
    reference: &[Complex<f32>],
    lane: &[Complex<f32>],
    block: usize,
) -> f64 {
    let mut fft = Transform::<f64>::forward(block);
    let window: Vec<f64> = (0..block).map(|n| hann_weight(n, block)).collect();
    let mut cross = vec![Complex::<f64>::default(); block];
    let mut start = 0;
    while start + block <= reference.len().min(lane.len()) {
        let mut spectrum = |samples: &[Complex<f32>]| {
            let mut buf: Vec<Complex<f64>> = samples[start..start + block]
                .iter()
                .zip(&window)
                .map(|(value, w)| widen(*value) * w)
                .collect();
            fft.process(&mut buf);
            buf
        };
        let a = spectrum(reference);
        let b = spectrum(lane);
        for ((sum, a), b) in cross.iter_mut().zip(&a).zip(&b) {
            *sum += b * a.conj();
        }
        start += block / 2;
    }
    let mut fit = LineFit::default();
    for (bin, value) in cross.iter().enumerate() {
        let nu = bin_frequency(bin, block);
        if nu.abs() <= FIT_BAND {
            fit.add(value.norm(), -TAU * nu, value.arg());
        }
    }
    fit.solve().1
}
