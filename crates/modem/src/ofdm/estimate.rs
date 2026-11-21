use num_complex::Complex;

use super::{interpolate::interpolate, smoother::DelaySmoother, subcarrier::SubcarrierMap};

pub const MIN_NOISE_VAR: f64 = 1e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelEstimator {
    LongTraining,
    ShortComb,
}

#[derive(Clone, Debug)]
pub struct ChannelEstimate {
    inv: Vec<Complex<f32>>,
    gain: Vec<f32>,
    h: Vec<Complex<f32>>,
    known: Vec<bool>,
    noise_var: f64,
}

impl ChannelEstimate {
    #[must_use]
    pub fn new(fft: usize) -> Self {
        Self {
            inv: vec![Complex::new(0.0, 0.0); fft],
            gain: vec![0.0; fft],
            h: vec![Complex::new(0.0, 0.0); fft],
            known: vec![false; fft],
            noise_var: 0.0,
        }
    }

    pub fn clear(&mut self) {
        self.h.fill(Complex::new(0.0, 0.0));
        self.inv.fill(Complex::new(0.0, 0.0));
        self.gain.fill(0.0);
        self.known.fill(false);
        self.noise_var = 0.0;
    }

    pub fn set(&mut self, bin: usize, h: Complex<f32>) {
        self.h[bin] = h;
        self.known[bin] = true;
    }

    pub fn finish(
        &mut self,
        map: &SubcarrierMap,
        noise_var: f64,
        ramp_cycles_per_bin: f64,
        smoothing: Option<(&mut DelaySmoother, f64)>,
    ) {
        if ramp_cycles_per_bin != 0.0 {
            self.rotate(map, -ramp_cycles_per_bin);
        }
        if let Some((smoother, estimate_var)) = smoothing {
            smoother.smooth(map, &mut self.h, estimate_var);
        }
        interpolate(map, &self.known, &mut self.h);
        if ramp_cycles_per_bin != 0.0 {
            self.rotate(map, ramp_cycles_per_bin);
        }
        for c in map.occupied() {
            let h = self.h[c.bin];
            let gain = h.norm_sqr();
            self.gain[c.bin] = gain;
            self.inv[c.bin] = if gain > 0.0 {
                h.conj() / gain
            } else {
                Complex::new(0.0, 0.0)
            };
        }
        self.noise_var = noise_var.max(MIN_NOISE_VAR);
    }

    fn rotate(&mut self, map: &SubcarrierMap, cycles_per_bin: f64) {
        let occupied = map.occupied();
        let Some(first) = occupied.first() else {
            return;
        };
        let step_angle = std::f64::consts::TAU * cycles_per_bin;
        let (sin_s, cos_s) = step_angle.sin_cos();
        let step = Complex::new(cos_s, sin_s);
        let mut offset = first.offset;
        let mut rot = Complex::from_polar(1.0, step_angle * f64::from(offset));
        for c in occupied {
            let delta = c.offset - offset;
            if delta == 1 {
                rot *= step;
                offset = c.offset;
            } else if delta == 2 {
                rot *= step * step;
                offset = c.offset;
            } else if delta != 0 {
                offset = c.offset;
                rot = Complex::from_polar(1.0, step_angle * f64::from(offset));
            }
            let h = self.h[c.bin];
            self.h[c.bin] = Complex::new(
                (f64::from(h.re) * rot.re - f64::from(h.im) * rot.im) as f32,
                (f64::from(h.re) * rot.im + f64::from(h.im) * rot.re) as f32,
            );
        }
    }

    #[must_use]
    pub fn h(&self, bin: usize) -> Complex<f32> {
        self.h[bin]
    }

    #[must_use]
    pub fn gain(&self, bin: usize) -> f32 {
        self.gain[bin]
    }

    #[must_use]
    pub fn equalize(&self, bin: usize, y: Complex<f32>) -> Complex<f32> {
        y * self.inv[bin]
    }

    #[must_use]
    pub fn noise_var(&self) -> f64 {
        self.noise_var
    }

    #[must_use]
    pub fn bin_noise_var(&self, bin: usize) -> Option<f64> {
        let gain = f64::from(self.gain[bin]);
        (gain > 0.0).then(|| self.noise_var / gain)
    }
}

#[must_use]
pub fn noise_var_from_repeats(first: &[Complex<f32>], second: &[Complex<f32>]) -> f64 {
    if first.is_empty() {
        return 0.0;
    }
    let sum: f64 = first
        .iter()
        .zip(second)
        .map(|(a, b)| f64::from((a - b).norm_sqr()))
        .sum();
    sum / (2.0 * first.len() as f64)
}

#[cfg(test)]
mod tests {
    use sdrmm_modem_test_support::ber::rng::Rng;

    use super::*;
    use crate::ofdm::OfdmParams;

    #[test]
    fn averaged_repeats_leave_half_the_noise_variance() {
        let mut rng = Rng::new(0x1e5);
        for &sigma2 in &[0.01f64, 0.1, 1.0] {
            let sigma = (sigma2 / 2.0).sqrt();
            let draw = |rng: &mut Rng| {
                Complex::new((rng.normal() * sigma) as f32, (rng.normal() * sigma) as f32)
            };
            let n = 20_000;
            let (mut first, mut second) = (Vec::with_capacity(n), Vec::with_capacity(n));
            let mut error = 0.0f64;
            for _ in 0..n {
                let (a, b) = (
                    Complex::new(1.0, 0.0) + draw(&mut rng),
                    Complex::new(1.0, 0.0) + draw(&mut rng),
                );
                let estimate = (a + b) * 0.5;
                error += f64::from((estimate - Complex::new(1.0, 0.0)).norm_sqr());
                first.push(a);
                second.push(b);
            }
            let measured_var = noise_var_from_repeats(&first, &second);
            assert!(
                (measured_var / sigma2 - 1.0).abs() < 0.05,
                "σ² {sigma2}: measured {measured_var}"
            );
            let mse = error / n as f64;
            assert!(
                (mse / (sigma2 / 2.0) - 1.0).abs() < 0.05,
                "σ² {sigma2}: estimator MSE {mse}, closed form {}",
                sigma2 / 2.0
            );
        }
    }

    #[test]
    fn a_nulled_bin_erases_instead_of_amplifying() {
        let map = OfdmParams::wifi_like().map().clone();
        let mut estimate = ChannelEstimate::new(map.fft());
        for c in map.occupied() {
            estimate.set(c.bin, Complex::new(1.0, 0.0));
        }
        let dead = map.data()[3].bin;
        estimate.set(dead, Complex::new(0.0, 0.0));
        estimate.finish(&map, 0.01, 0.0, None);
        assert_eq!(
            estimate.equalize(dead, Complex::new(0.4, -0.2)),
            Complex::new(0.0, 0.0)
        );
        assert_eq!(estimate.bin_noise_var(dead), None);
        let live = map.data()[4].bin;
        let noise_var = estimate.bin_noise_var(live).unwrap();
        assert!((noise_var - 0.01).abs() < 1e-9);
    }
}
