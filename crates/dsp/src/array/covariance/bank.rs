use num_complex::Complex;

use super::{
    CovarianceError,
    hermitian::{set_mirrored, write_identity},
};
use crate::math::complex::{decay_cross_add, scale_real_into, widen};
use crate::math::fft::FftPair;
use crate::math::linalg::{CMat, LinalgError, MAX_ORDER};
use crate::math::window::hann;

const MIN_FFT: usize = 64;
const MAX_FFT: usize = 8192;

pub struct CovarianceBank {
    lanes: usize,
    fft_size: usize,
    hop: usize,
    window: Vec<f32>,
    fft: FftPair,
    frames: Vec<Vec<Complex<f32>>>,
    spectra: Vec<Vec<Complex<f32>>>,
    fill: usize,
    sums: Vec<Complex<f32>>,
    weight: f64,
}

impl CovarianceBank {
    pub fn new(lanes: usize, fft_size: usize, hop: usize) -> Result<Self, CovarianceError> {
        if lanes == 0 || lanes > MAX_ORDER {
            return Err(LinalgError::Order(lanes).into());
        }
        if !fft_size.is_power_of_two() || !(MIN_FFT..=MAX_FFT).contains(&fft_size) {
            return Err(CovarianceError::FftSize(fft_size));
        }
        if hop == 0 || hop > fft_size {
            return Err(CovarianceError::Hop(hop));
        }
        let tri = triangle(lanes);
        Ok(Self {
            lanes,
            fft_size,
            hop,
            window: hann(fft_size),
            fft: FftPair::new(fft_size),
            frames: vec![vec![Complex::new(0.0, 0.0); fft_size]; lanes],
            spectra: vec![vec![Complex::new(0.0, 0.0); fft_size]; lanes],
            fill: 0,
            sums: vec![Complex::new(0.0, 0.0); fft_size * tri],
            weight: 0.0,
        })
    }

    pub fn push(
        &mut self,
        lanes: &[&[Complex<f32>]],
        decay_per_frame: f32,
    ) -> Result<u32, CovarianceError> {
        if lanes.len() != self.lanes {
            return Err(LinalgError::Order(lanes.len()).into());
        }
        let len = lanes.first().map_or(0, |lane| lane.len());
        if lanes.iter().any(|lane| lane.len() != len) {
            return Err(CovarianceError::LaneLength);
        }
        let mut at = 0;
        let mut completed = 0;
        while at < len {
            let take = (self.fft_size - self.fill).min(len - at);
            for (frame, lane) in self.frames.iter_mut().zip(lanes) {
                frame[self.fill..self.fill + take].copy_from_slice(&lane[at..at + take]);
            }
            self.fill += take;
            at += take;
            if self.fill == self.fft_size {
                self.complete_frame(decay_per_frame);
                completed += 1;
                for frame in &mut self.frames {
                    frame.copy_within(self.hop.., 0);
                }
                self.fill = self.fft_size - self.hop;
            }
        }
        Ok(completed)
    }

    #[must_use]
    pub const fn bins(&self) -> usize {
        self.fft_size
    }

    #[must_use]
    pub const fn lanes(&self) -> usize {
        self.lanes
    }

    #[must_use]
    pub const fn weight(&self) -> f64 {
        self.weight
    }

    pub fn group_matrix(
        &self,
        first_bin: usize,
        bins: usize,
        out: &mut CMat,
    ) -> Result<bool, CovarianceError> {
        let end = first_bin.saturating_add(bins);
        if bins == 0 || end > self.fft_size {
            return Err(CovarianceError::Bins(first_bin, end));
        }
        let n = self.lanes;
        out.resize(n)?;
        if self.weight <= 0.0 {
            write_identity(out);
            return Ok(false);
        }
        let half = self.fft_size / 2;
        let mask = self.fft_size - 1;
        let scale = 1.0 / (self.weight * bins as f64);
        let mut index = 0;
        for i in 0..n {
            for j in i..n {
                let row = &self.sums[index * self.fft_size..(index + 1) * self.fft_size];
                let sum: Complex<f64> = (first_bin..end)
                    .map(|bin| widen(row[(bin + half) & mask]))
                    .sum();
                set_mirrored(out, i, j, sum * scale);
                index += 1;
            }
        }
        Ok(true)
    }

    #[must_use]
    pub fn bin_power(&self, bin: usize) -> f32 {
        if bin >= self.fft_size || self.weight <= 0.0 {
            return 0.0;
        }
        let n = self.lanes;
        let raw = (bin + self.fft_size / 2) & (self.fft_size - 1);
        let mut index = 0;
        let mut trace = 0.0f64;
        for i in 0..n {
            trace += f64::from(self.sums[index * self.fft_size + raw].re);
            index += n - i;
        }
        (trace / (n as f64 * self.weight)) as f32
    }

    pub fn reset(&mut self) {
        self.fill = 0;
        self.weight = 0.0;
        self.sums.fill(Complex::new(0.0, 0.0));
    }

    fn complete_frame(&mut self, decay: f32) {
        for (spectrum, frame) in self.spectra.iter_mut().zip(&self.frames) {
            scale_real_into(spectrum, frame, &self.window);
            self.fft.forward(spectrum);
        }
        let n = self.lanes;
        let bins = self.fft_size;
        let mut index = 0;
        for i in 0..n {
            let left = &self.spectra[i];
            for right in &self.spectra[i..] {
                let row = &mut self.sums[index * bins..(index + 1) * bins];
                decay_cross_add(row, decay, left, right);
                index += 1;
            }
        }
        self.weight = self.weight * f64::from(decay) + 1.0;
    }
}

const fn triangle(n: usize) -> usize {
    n * (n + 1) / 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::covariance::testing::random_lanes;
    use crate::math::fft::Transform;

    fn direct_bins(
        lanes: &[Vec<Complex<f32>>],
        size: usize,
        hop: usize,
        decay: f64,
    ) -> Vec<Vec<Complex<f64>>> {
        let mut fft = Transform::<f64>::forward(size);
        let window = hann(size);
        let n = lanes.len();
        let mut sums = vec![vec![Complex::new(0.0f64, 0.0); n * n]; size];
        let mut weight = 0.0;
        let mut start = 0;
        while start + size <= lanes[0].len() {
            let spectra: Vec<Vec<Complex<f64>>> = lanes
                .iter()
                .map(|lane| {
                    let mut buf: Vec<Complex<f64>> = lane[start..start + size]
                        .iter()
                        .zip(&window)
                        .map(|(x, w)| widen(x * w))
                        .collect();
                    fft.process(&mut buf);
                    buf
                })
                .collect();
            for (bin, sum) in sums.iter_mut().enumerate() {
                let raw = (bin + size / 2) % size;
                for i in 0..n {
                    for j in 0..n {
                        sum[i * n + j] =
                            sum[i * n + j] * decay + spectra[i][raw] * spectra[j][raw].conj();
                    }
                }
            }
            weight = weight * decay + 1.0;
            start += hop;
        }
        for sum in &mut sums {
            for value in sum.iter_mut() {
                *value /= weight;
            }
        }
        sums
    }

    #[test]
    fn bank_bin_matrix_matches_a_direct_per_bin_computation() {
        let lanes = random_lanes(3, 1500, 21);
        let mut bank = CovarianceBank::new(3, 256, 128).unwrap();
        let mut frames = 0;
        for (start, end) in [(0, 100), (100, 700), (700, 701), (701, 1500)] {
            let views: Vec<&[Complex<f32>]> = lanes.iter().map(|l| &l[start..end]).collect();
            frames += bank.push(&views, 0.9).unwrap();
        }
        assert_eq!(frames, 10);
        let direct = direct_bins(&lanes, 256, 128, 0.9);
        let mut out = CMat::zeros(1).unwrap();
        for (bin, expected) in direct.iter().enumerate() {
            assert!(bank.group_matrix(bin, 1, &mut out).unwrap());
            for i in 0..3 {
                for j in 0..3 {
                    let want = expected[i * 3 + j];
                    let got = widen(out.get(i, j));
                    assert!(
                        (got - want).norm() <= 1e-4 * want.norm().max(1.0),
                        "{bin} {i} {j}"
                    );
                }
            }
            let trace: f64 = (0..3).map(|i| expected[i * 3 + i].re).sum::<f64>() / 3.0;
            assert!((f64::from(bank.bin_power(bin)) - trace).abs() <= 1e-4 * trace.max(1.0));
        }
        bank.group_matrix(10, 4, &mut out).unwrap();
        let want: Complex<f64> = (10..14).map(|bin| direct[bin][1]).sum::<Complex<f64>>() / 4.0;
        let got = out.get(0, 1);
        assert!((widen(got) - want).norm() < 1e-3 * want.norm().max(1.0));
    }

    #[test]
    fn a_bank_refuses_bad_shapes_and_reports_empty() {
        assert_eq!(
            CovarianceBank::new(3, 100, 50).err(),
            Some(CovarianceError::FftSize(100))
        );
        assert_eq!(
            CovarianceBank::new(3, 16384, 50).err(),
            Some(CovarianceError::FftSize(16384))
        );
        assert_eq!(
            CovarianceBank::new(3, 64, 65).err(),
            Some(CovarianceError::Hop(65))
        );
        let mut bank = CovarianceBank::new(2, 64, 64).unwrap();
        let mut out = CMat::zeros(1).unwrap();
        assert!(!bank.group_matrix(0, 64, &mut out).unwrap());
        assert_eq!(out, CMat::identity(2).unwrap());
        assert_eq!(
            bank.group_matrix(60, 5, &mut out),
            Err(CovarianceError::Bins(60, 65))
        );
        let lane = vec![Complex::new(1.0f32, 0.0); 64];
        assert_eq!(
            bank.push(&[&lane], 1.0),
            Err(CovarianceError::Linalg(LinalgError::Order(1)))
        );
        assert_eq!(
            bank.push(&[&lane, &lane[..63]], 1.0),
            Err(CovarianceError::LaneLength)
        );
        assert_eq!(bank.push(&[&lane, &lane], 1.0), Ok(1));
        assert!(bank.bin_power(32) > 0.0);
        bank.reset();
        assert_eq!(bank.bin_power(32), 0.0);
    }
}
