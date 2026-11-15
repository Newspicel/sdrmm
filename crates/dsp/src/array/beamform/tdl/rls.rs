use num_complex::Complex;

use super::TdlCanceller;
use crate::array::beamform::ZERO;

/// Initial inverse correlation is the identity over this share of the reference power.
const RLS_SEED: f32 = 1e-2;
/// Steps between re-symmetrising the inverse correlation against rounding drift.
const SYMMETRISE_EVERY: u32 = 4096;

impl TdlCanceller {
    pub(super) fn prime(&mut self, lanes: &[&[Complex<f32>]], len: usize) {
        if !self.adaptation.is_rls() || len == 0 {
            return;
        }
        let mut sum = 0.0f64;
        for &lane in &self.references[..self.refs] {
            sum += lanes[lane][..len]
                .iter()
                .filter(|value| value.is_finite())
                .map(|value| f64::from(value.norm_sqr()))
                .sum::<f64>();
        }
        let power = (sum / (len * self.refs) as f64) as f32;
        if !(power.is_finite() && power > 0.0) {
            return;
        }
        let m = self.h.len();
        let seed = (RLS_SEED * power).recip();
        self.p.fill(ZERO);
        for i in 0..m {
            self.p[i * m + i] = Complex::new(seed, 0.0);
        }
        self.since_symmetrise = 0;
        self.primed = true;
    }

    pub(super) fn rls(&mut self, forget: f32, y: Complex<f32>) -> bool {
        if !self.primed {
            return true;
        }
        let m = self.h.len();
        let mut quad = 0.0f32;
        for (row, pi) in self.gain.iter_mut().enumerate() {
            let line = &self.p[row * m..(row + 1) * m];
            *pi = line.iter().zip(&self.regressor).map(|(p, u)| p * u).sum();
        }
        for (u, pi) in self.regressor.iter().zip(&self.gain) {
            quad += (u.conj() * pi).re;
        }
        let denominator = forget + quad;
        if !(denominator.is_finite() && denominator > 0.0) {
            return false;
        }
        let inverse_forget = forget.recip();
        for row in 0..m {
            let k = self.gain[row] / denominator;
            let line = &mut self.p[row * m..(row + 1) * m];
            for (p, pi) in line.iter_mut().zip(&self.gain) {
                *p = (*p - k * pi.conj()) * inverse_forget;
            }
        }
        for (h, pi) in self.h.iter_mut().zip(&self.gain) {
            *h += pi * (y.conj() / denominator);
        }
        self.since_symmetrise += 1;
        if self.since_symmetrise >= SYMMETRISE_EVERY {
            self.since_symmetrise = 0;
            return self.symmetrise();
        }
        true
    }

    fn symmetrise(&mut self) -> bool {
        let m = self.h.len();
        for row in 0..m {
            let diagonal = self.p[row * m + row];
            if !(diagonal.re.is_finite() && diagonal.re > 0.0) {
                return false;
            }
            self.p[row * m + row] = Complex::new(diagonal.re, 0.0);
            for col in row + 1..m {
                let mean = (self.p[row * m + col] + self.p[col * m + row].conj()) * 0.5;
                if !mean.is_finite() {
                    return false;
                }
                self.p[row * m + col] = mean;
                self.p[col * m + row] = mean.conj();
            }
        }
        self.h.iter().all(|value| value.is_finite())
    }
}
