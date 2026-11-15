pub mod demap;
mod error;
#[cfg(test)]
mod fixtures;
pub mod tables;

use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::complex::{narrow, widen};

pub use error::ConstellationError;

/// The largest rotational symmetry looked for; no table in use has more points on a ring.
const MAX_ROTATIONAL_ORDER: u32 = 64;

/// Share of the minimum distance a rotated point may miss its neighbour by and still count as
/// landing on it.
const SYMMETRY_TOLERANCE: f64 = 1e-3;

/// Widest label a table may carry.
const MAX_BITS_PER_SYMBOL: u32 = 32;

#[derive(Clone, Debug, PartialEq)]
pub struct Constellation {
    points: Vec<Complex<f32>>,
    labels: Vec<u32>,
    index_of_label: Vec<usize>,
    bits_per_symbol: usize,
}

impl Constellation {
    pub fn from_points(
        points: Vec<Complex<f32>>,
        labels: Vec<u32>,
    ) -> Result<Self, ConstellationError> {
        let n = points.len();
        if !n.is_power_of_two() || n < 2 || n.trailing_zeros() > MAX_BITS_PER_SYMBOL {
            return Err(ConstellationError::SizeNotPowerOfTwo(n));
        }
        if labels.len() != n {
            return Err(ConstellationError::LabelCountMismatch {
                points: n,
                labels: labels.len(),
            });
        }
        let mut seen = vec![false; n];
        for &label in &labels {
            match seen.get_mut(label as usize) {
                Some(slot) if !*slot => *slot = true,
                _ => return Err(ConstellationError::BadLabel(label)),
            }
        }
        let power = points.iter().map(|&p| widen(p).norm_sqr()).sum::<f64>() / n as f64;
        if !(power.is_finite() && power > 0.0) {
            return Err(ConstellationError::ZeroPower);
        }
        let scale = power.sqrt().recip();
        let points = points
            .into_iter()
            .map(|p| narrow(widen(p) * scale))
            .collect();
        let mut index_of_label = vec![0usize; n];
        for (index, &label) in labels.iter().enumerate() {
            index_of_label[label as usize] = index;
        }
        Ok(Self {
            points,
            labels,
            index_of_label,
            bits_per_symbol: n.trailing_zeros() as usize,
        })
    }

    #[must_use]
    pub fn points(&self) -> &[Complex<f32>] {
        &self.points
    }

    #[must_use]
    pub fn labels(&self) -> &[u32] {
        &self.labels
    }

    #[must_use]
    pub fn bits_per_symbol(&self) -> usize {
        self.bits_per_symbol
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.points.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    #[must_use]
    pub fn index_of(&self, label: u32) -> Option<usize> {
        self.index_of_label.get(label as usize).copied()
    }

    #[must_use]
    pub fn nearest_index(&self, y: Complex<f32>) -> usize {
        let mut best = 0usize;
        let mut best_d2 = f64::INFINITY;
        for (i, &p) in self.points.iter().enumerate() {
            let d2 = squared_distance(y, p);
            if d2 < best_d2 {
                best_d2 = d2;
                best = i;
            }
        }
        best
    }

    #[must_use]
    pub fn nearest(&self, y: Complex<f32>) -> Complex<f32> {
        self.points[self.nearest_index(y)]
    }

    #[must_use]
    pub fn min_distance(&self) -> f64 {
        let mut min = f64::INFINITY;
        for i in 0..self.points.len() {
            for j in (i + 1)..self.points.len() {
                min = min.min(f64::from((self.points[i] - self.points[j]).norm()));
            }
        }
        min
    }

    #[must_use]
    pub fn rotational_order(&self) -> u32 {
        let tolerance = SYMMETRY_TOLERANCE * self.min_distance().max(f64::MIN_POSITIVE);
        (1..=MAX_ROTATIONAL_ORDER)
            .rev()
            .find(|&m| self.is_invariant_under(TAU / f64::from(m), tolerance))
            .unwrap_or(1)
    }

    #[must_use]
    pub fn hard_slice(&self, y: Complex<f32>) -> u32 {
        self.labels[self.nearest_index(y)]
    }

    fn is_invariant_under(&self, theta: f64, tolerance: f64) -> bool {
        let rot = narrow(Complex::from_polar(1.0, theta));
        self.points
            .iter()
            .all(|&p| f64::from((self.nearest(p * rot) - p * rot).norm()) <= tolerance)
    }
}

/// Squared distance between two points, taken in `f64` so near ties keep their order.
#[must_use]
pub(crate) fn squared_distance(a: Complex<f32>, b: Complex<f32>) -> f64 {
    (widen(a) - widen(b)).norm_sqr()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constellation::fixtures::gray_4pam;

    #[test]
    fn the_rotational_order_is_the_table_symmetry() {
        for (name, table, want) in [
            ("ook", tables::ook().unwrap(), 1u32),
            ("bpsk", tables::pam(2).unwrap(), 2),
            ("pam4", tables::pam(4).unwrap(), 2),
            ("qpsk", tables::qam_square(4).unwrap(), 4),
            ("8psk", tables::psk(8).unwrap(), 8),
            ("qam16", tables::qam_square(16).unwrap(), 4),
            ("cross32", tables::qam_cross(32).unwrap(), 4),
            ("apsk16", tables::apsk16_dvbs2(2.57).unwrap(), 4),
        ] {
            assert_eq!(table.rotational_order(), want, "{name}");
        }
    }

    #[test]
    fn a_label_resolves_to_its_own_point_without_a_scan() {
        let c = gray_4pam();
        for (index, &label) in c.labels().iter().enumerate() {
            assert_eq!(c.index_of(label), Some(index));
        }
        assert_eq!(c.index_of(4), None);
        assert_eq!(c.index_of(u32::MAX), None);
    }

    #[test]
    fn the_slice_index_names_the_point_and_the_label_it_picked() {
        let c = gray_4pam();
        for y in [-2.0f32, -0.9, 0.0, 0.9, 2.0] {
            let y = Complex::new(y, 0.1);
            let index = c.nearest_index(y);
            assert_eq!(c.points()[index], c.nearest(y));
            assert_eq!(c.labels()[index], c.hard_slice(y));
        }
    }

    #[test]
    fn construction_normalises_to_unit_mean_energy() {
        let c = gray_4pam();
        let power = fixtures::mean_energy(&c);
        assert!((power - 1.0).abs() < 1e-6, "mean Es {power}");
        let a = 5f64.sqrt().recip();
        assert!((f64::from(c.points()[3].re) - 3.0 * a).abs() < 1e-7);
        assert_eq!(c.bits_per_symbol(), 2);
    }

    #[test]
    fn an_already_normalised_table_passes_through() {
        let x = std::f32::consts::FRAC_1_SQRT_2;
        let c = Constellation::from_points(
            vec![
                Complex::new(x, x),
                Complex::new(-x, x),
                Complex::new(-x, -x),
                Complex::new(x, -x),
            ],
            vec![0, 1, 3, 2],
        )
        .unwrap();
        assert!((c.points()[0].re - x).abs() < 1e-7);
    }

    #[test]
    fn hard_slice_picks_the_nearest_label() {
        let c = gray_4pam();
        assert_eq!(c.hard_slice(Complex::new(-2.0, 0.0)), 0b00);
        assert_eq!(c.hard_slice(Complex::new(-0.5, 0.1)), 0b01);
        assert_eq!(c.hard_slice(Complex::new(0.5, -0.1)), 0b11);
        assert_eq!(c.hard_slice(Complex::new(0.95, 0.0)), 0b10);
    }

    #[test]
    fn bad_tables_are_rejected_with_the_right_error() {
        let p = |n: usize| vec![Complex::new(1.0f32, 0.0); n];
        assert_eq!(
            Constellation::from_points(p(3), vec![0, 1, 2]).unwrap_err(),
            ConstellationError::SizeNotPowerOfTwo(3)
        );
        assert_eq!(
            Constellation::from_points(p(1), vec![0]).unwrap_err(),
            ConstellationError::SizeNotPowerOfTwo(1)
        );
        assert_eq!(
            Constellation::from_points(p(4), vec![0, 1]).unwrap_err(),
            ConstellationError::LabelCountMismatch {
                points: 4,
                labels: 2
            }
        );
        assert_eq!(
            Constellation::from_points(p(4), vec![0, 1, 2, 2]).unwrap_err(),
            ConstellationError::BadLabel(2)
        );
        assert_eq!(
            Constellation::from_points(p(4), vec![0, 1, 2, 4]).unwrap_err(),
            ConstellationError::BadLabel(4)
        );
        assert_eq!(
            Constellation::from_points(vec![Complex::new(0.0, 0.0); 2], vec![0, 1]).unwrap_err(),
            ConstellationError::ZeroPower
        );
    }
}
