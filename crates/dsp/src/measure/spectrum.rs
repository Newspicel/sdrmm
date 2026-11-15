mod average;
mod decimate;
mod floor;
mod scale;

use num_complex::Complex;

pub use average::PowerAverage;
pub use decimate::decimate_signal;
pub use floor::{NoiseFloor, SpanFloor};
pub use scale::{DbWindowSmoother, adaptive_db_window, quantize_db};

use crate::{
    math::fastmath::fast_power_db,
    math::fft::Transform,
    math::window::{coherent_gain, hann},
};

/// Added to every power before it becomes dB, so an empty bin reads as a deep floor instead of
/// minus infinity.
const POWER_EPSILON: f32 = 1e-24;

pub struct SpectrumAnalyzer {
    fft: Transform,
    size: usize,
    window: Vec<f32>,
    inv_gain: f32,
    buf: Vec<Complex<f32>>,
}

impl SpectrumAnalyzer {
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self::with_window(hann(size))
    }

    #[must_use]
    pub fn with_window(window: Vec<f32>) -> Self {
        let size = window.len();
        let fft = Transform::forward(size.max(1));
        let inv_gain = 1.0 / coherent_gain(&window).max(f32::MIN_POSITIVE);
        Self {
            fft,
            size,
            window,
            inv_gain,
            buf: vec![Complex::new(0.0, 0.0); size],
        }
    }

    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    pub fn power_db(&mut self, input: &[Complex<f32>], out: &mut [f32]) {
        assert_eq!(input.len(), self.size, "input length must equal FFT size");
        assert_eq!(out.len(), self.size, "output length must equal FFT size");
        self.transform(input);
        let scale = self.power_scale();
        let (negative, positive) = self.display_halves();
        let (low, high) = out.split_at_mut(self.size / 2);
        bins_db(negative, scale, low);
        bins_db(positive, scale, high);
    }

    /// Adds one transform's linear power to `average` in display order, skipping the dB round
    /// trip that [`Self::power_db`] followed by [`PowerAverage::add`] would cost.
    pub fn accumulate(&mut self, input: &[Complex<f32>], average: &mut PowerAverage) {
        assert_eq!(input.len(), self.size, "input length must equal FFT size");
        assert_eq!(average.len(), self.size, "average length mismatch");
        self.transform(input);
        let scale = self.power_scale();
        let (negative, positive) = self.display_halves();
        average.add_power(negative, positive, scale);
    }

    fn transform(&mut self, input: &[Complex<f32>]) {
        for ((dst, &s), &w) in self.buf.iter_mut().zip(input).zip(&self.window) {
            *dst = s * w;
        }
        self.fft.process(&mut self.buf);
    }

    fn power_scale(&self) -> f32 {
        self.inv_gain * self.inv_gain
    }

    /// The negative-frequency bins, then the positive ones, so DC sits mid-display.
    fn display_halves(&self) -> (&[Complex<f32>], &[Complex<f32>]) {
        let (positive, negative) = self.buf.split_at(self.size - self.size / 2);
        (negative, positive)
    }
}

fn bins_db(bins: &[Complex<f32>], scale: f32, out: &mut [f32]) {
    for (slot, bin) in out.iter_mut().zip(bins) {
        *slot = fast_power_db(bin.norm_sqr() * scale + POWER_EPSILON);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::complex_tone;

    #[test]
    fn tone_lands_in_expected_bin_near_0dbfs() {
        let size = 1024;
        let mut an = SpectrumAnalyzer::new(size);

        let bin = size / 8;
        let input = complex_tone(bin as f64 / size as f64, size);

        let mut db = vec![0.0f32; size];
        an.power_db(&input, &mut db);

        let expected = size / 2 + bin;
        let (peak_idx, &peak_val) = db
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert_eq!(peak_idx, expected, "peak bin");
        assert!(peak_val > -1.0, "peak near 0 dBFS, got {peak_val}");

        assert!(
            db[expected / 2] < -60.0,
            "floor too high: {}",
            db[expected / 2]
        );
    }

    #[test]
    fn accumulating_power_matches_averaging_each_transform_in_db() {
        let size = 1_024;
        let mut analyzer = SpectrumAnalyzer::new(size);
        let mut via_db = PowerAverage::new(size);
        let mut direct = PowerAverage::new(size);
        let mut db = vec![0.0; size];
        for seed in 1..=4u32 {
            let input: Vec<_> = (0..size)
                .map(|n| {
                    let phase = 0.37 * n as f32 * seed as f32;
                    Complex::new(phase.cos(), phase.sin()) * 0.1
                })
                .collect();
            analyzer.power_db(&input, &mut db);
            via_db.add(&db);
            analyzer.accumulate(&input, &mut direct);
        }
        assert_eq!(direct.count(), 4);
        let (mut expected, mut actual) = (vec![0.0; size], vec![0.0; size]);
        via_db.take_db(&mut expected);
        direct.take_db(&mut actual);
        for (bin, (want, got)) in expected.iter().zip(&actual).enumerate() {
            assert!((want - got).abs() < 0.01, "bin {bin}: {want} vs {got}");
        }
    }
}
