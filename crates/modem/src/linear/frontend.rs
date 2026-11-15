use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::complex::{narrow, widen};

use super::params::LinearParams;

const PROPER_LIMIT: f64 = 0.05;

const ZERO_MEAN_LIMIT: f64 = 0.05;

const SIGNIFICANCE: f64 = 4.0;

/// How far from a whole turn twice the per-symbol rotation must sit to spread the image.
const SPREADING_ROTATION_RAD: f64 = 1e-6;

/// Below this share of the power the image weight's quadratic degenerates to a linear one.
const LINEAR_IMAGE_LIMIT: f64 = 1e-12;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrontCorrection {
    pub dc: Complex<f64>,
    pub image: Complex<f64>,
}

impl FrontCorrection {
    pub fn apply(&self, iq: &mut [Complex<f32>]) {
        let dc = narrow(self.dc);
        let image = narrow(self.image);
        for s in iq.iter_mut() {
            let centred = *s - dc;
            *s = centred - image * centred.conj();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrontEstimator {
    remove_dc: bool,
    remove_image: bool,
    sps: usize,
}

impl FrontEstimator {
    #[must_use]
    pub fn for_params(params: &LinearParams) -> Self {
        let points = params.constellation().points();
        let n = points.len() as f64;
        let power: f64 = points.iter().map(|p| f64::from(p.norm_sqr())).sum::<f64>() / n;
        let mean: Complex<f64> = points.iter().map(|&p| widen(p)).sum::<Complex<f64>>() / n;
        let square: Complex<f64> = points
            .iter()
            .map(|&p| widen(p) * widen(p))
            .sum::<Complex<f64>>()
            / n;
        let spun = (2.0 * params.rotation_rad()).rem_euclid(TAU);
        let rotation_spreads =
            spun > SPREADING_ROTATION_RAD && (TAU - spun) > SPREADING_ROTATION_RAD;
        let zero_mean = mean.norm_sqr() <= ZERO_MEAN_LIMIT * power;
        Self {
            sps: params.sps(),
            remove_dc: zero_mean,
            remove_image: zero_mean && (square.norm() <= PROPER_LIMIT * power || rotation_spreads),
        }
    }

    #[must_use]
    pub fn estimate(&self, iq: &[Complex<f32>]) -> FrontCorrection {
        if iq.is_empty() || !(self.remove_dc || self.remove_image) {
            return FrontCorrection::default();
        }
        let n = iq.len() as f64;
        let symbols = (n / self.sps as f64).max(1.0);
        let raw_power = iq.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>() / n;
        let mean = iq.iter().map(|&s| widen(s)).sum::<Complex<f64>>() / n;
        let dc = if self.remove_dc
            && mean.norm_sqr() > SIGNIFICANCE * SIGNIFICANCE * raw_power / symbols
        {
            mean
        } else {
            Complex::new(0.0, 0.0)
        };
        if !self.remove_image {
            return FrontCorrection {
                dc,
                image: Complex::new(0.0, 0.0),
            };
        }
        let (square, power) = second_moments(iq, dc);
        let significant = square.norm() / power > SIGNIFICANCE / symbols.sqrt();
        FrontCorrection {
            dc,
            image: if significant {
                circular_weight(square / n, power / n)
            } else {
                Complex::new(0.0, 0.0)
            },
        }
    }
}

/// Sums of `y^2` and `|y|^2` over `iq` less `dc`.
fn second_moments(iq: &[Complex<f32>], dc: Complex<f64>) -> (Complex<f64>, f64) {
    let (mut square, mut power) = (Complex::new(0.0f64, 0.0), 0.0f64);
    for &s in iq {
        let y = widen(s) - dc;
        square += y * y;
        power += y.norm_sqr();
    }
    (square, power)
}

fn circular_weight(square: Complex<f64>, power: f64) -> Complex<f64> {
    if power <= 0.0 {
        return Complex::new(0.0, 0.0);
    }
    let a = square.conj();
    let b = -2.0 * power;
    let c = square;
    if a.norm() <= LINEAR_IMAGE_LIMIT * power {
        return -c / b;
    }
    let root = (b * b - a * c * 4.0).sqrt();
    let w = (-b - root) / (a * 2.0);
    if w.norm().is_finite() && w.norm() < 1.0 {
        w
    } else {
        -c / b
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::draws;
    use sdrmm_dsp::Decibels;

    use super::*;
    use crate::{
        constellation::{Constellation, tables},
        linear::{
            LinearMod,
            testutil::{SPS, rrc},
        },
    };

    fn params(table: Constellation, rotation: f64) -> LinearParams {
        LinearParams::new(table, rrc(), SPS)
            .unwrap()
            .with_rotation(rotation)
            .unwrap()
    }

    fn wave(p: &LinearParams, n: usize) -> Vec<Complex<f32>> {
        let m = p.constellation().len() as u32;
        LinearMod::transmission(p, &draws(n, m as usize, 0x1f0e))
    }

    fn impair(w: &mut [Complex<f32>], gain_db: f64, phase_deg: f64, dc: Complex<f32>) {
        let g = gain_db.db_to_amplitude();
        let phi = phase_deg.to_radians();
        for s in w.iter_mut() {
            let (i, q) = (f64::from(s.re), f64::from(s.im));
            let q2 = g * (q * phi.cos() + i * phi.sin());
            *s = Complex::new(i as f32, q2 as f32) + dc;
        }
    }

    fn image_ratio(w: &[Complex<f32>]) -> f64 {
        let (square, power) = second_moments(w, Complex::new(0.0, 0.0));
        square.norm() / power
    }

    #[test]
    fn improper_tables_are_left_alone() {
        for table in [tables::pam(2).unwrap(), tables::pam(4).unwrap()] {
            let est = FrontEstimator::for_params(&params(table, 0.0));
            assert!(!est.remove_image);
        }
        assert!(!FrontEstimator::for_params(&params(tables::ook().unwrap(), 0.0)).remove_dc);
        let spun = FrontEstimator::for_params(&params(
            tables::pam(2).unwrap(),
            std::f64::consts::FRAC_PI_2,
        ));
        assert!(spun.remove_image);
    }

    #[test]
    fn a_gain_and_phase_imbalance_is_undone() {
        let p = params(tables::qam_square(16).unwrap(), 0.0);
        let mut w = wave(&p, 4_000);
        impair(&mut w, 3.0, 20.0, Complex::new(0.05, -0.03));
        assert!(image_ratio(&w) > 0.1);
        let est = FrontEstimator::for_params(&p);
        let correction = est.estimate(&w);
        correction.apply(&mut w);
        assert!(image_ratio(&w) < 0.01, "image left {}", image_ratio(&w));
        let mean: Complex<f32> = w.iter().sum::<Complex<f32>>() / w.len() as f32;
        assert!(mean.norm() < 1e-3, "dc left {mean}");
    }
}
