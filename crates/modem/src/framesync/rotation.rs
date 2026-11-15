use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::complex::{narrow, widen};

#[must_use]
pub fn conj_product(a: Complex<f32>, b: Complex<f32>) -> Complex<f64> {
    widen(a).conj() * widen(b)
}

#[must_use]
pub fn rotor(cfo: f64, n: usize) -> (Complex<f64>, Complex<f64>) {
    let phase = -TAU * cfo;
    let start = phase * n as f64;
    let (s0, c0) = start.sin_cos();
    let (s1, c1) = phase.sin_cos();
    (Complex::new(c0, s0), Complex::new(c1, s1))
}

pub fn derotate(x: &[Complex<f32>], start: usize, cfo: f64, out: &mut [Complex<f32>]) {
    let phase = (TAU * cfo * start as f64).rem_euclid(TAU);
    derotate_from(x, start, cfo, phase, out);
}

pub fn derotate_from(
    x: &[Complex<f32>],
    start: usize,
    cfo: f64,
    phase: f64,
    out: &mut [Complex<f32>],
) {
    let (sin, cos) = (-phase).sin_cos();
    let mut rot = Complex::new(cos, sin);
    let step = rotor(cfo, 1).1;
    for (n, slot) in out.iter_mut().enumerate() {
        let s = x.get(start + n).copied().unwrap_or(Complex::new(0.0, 0.0));
        *slot = narrow(widen(s) * rot);
        rot *= step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derotation_undoes_a_carrier_offset_from_any_start() {
        let x: Vec<Complex<f32>> = (0..64)
            .map(|n| Complex::from_polar(1.0, (TAU * 0.02 * f64::from(n)) as f32))
            .collect();
        let mut out = vec![Complex::new(0.0, 0.0); 16];
        derotate(&x, 30, 0.02, &mut out);
        for v in out {
            assert!((v - Complex::new(1.0, 0.0)).norm() < 1e-4);
        }
    }
}
