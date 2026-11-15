use num_complex::Complex;

use super::layout::Layout;
use crate::array::covariance::PhaseMode;
use crate::array::manifold::{Direction, Geometry, phase_rates, steer, testing::uca_for_beta};
use crate::math::complex::narrow;
use crate::math::linalg::{CMat, Eigen, HermitianEigen};
use crate::math::special::wrap_deg;

pub(super) const FREQ: f64 = 433.92e6;

pub(super) fn steering(geometry: &Geometry, azimuth_deg: f64) -> Vec<Complex<f32>> {
    let mut a = vec![Complex::new(0.0f32, 0.0); geometry.len()];
    steer(
        geometry.positions(),
        FREQ,
        Direction::horizon(azimuth_deg),
        &mut a,
    );
    a
}

pub(super) fn rates_at(geometry: &Geometry, azimuth_deg: f64) -> Vec<f64> {
    let mut rates = vec![0.0; geometry.len()];
    phase_rates(
        geometry.positions(),
        FREQ,
        Direction::horizon(azimuth_deg),
        &mut rates,
    );
    rates
}

pub(super) fn noise(n: usize, power: f32) -> CMat {
    let mut r = CMat::identity(n).unwrap();
    r.scale(power);
    r
}

/// Covariance of unit sources at the given element-to-element phases on an `m`-element line.
pub(super) fn line_covariance(m: usize, phases: &[f64], noise_power: f32) -> CMat {
    let mut r = noise(m, noise_power);
    for &phase in phases {
        let a: Vec<Complex<f32>> = (0..m)
            .map(|i| narrow(Complex::from_polar(1.0, phase * i as f64)))
            .collect();
        r.add_outer(&a, 1.0);
    }
    r
}

pub(super) fn decompose(r: &CMat) -> Eigen {
    let mut solver = HermitianEigen::new(r.order()).unwrap();
    let mut eigen = Eigen::new();
    solver.solve(r, &mut eigen).unwrap();
    eigen
}

/// Distance in degrees from `phase` to the nearest phase among `values`.
pub(super) fn closest_phase(values: &[Complex<f64>], phase: f64) -> f64 {
    values
        .iter()
        .map(|z| wrap_deg((z.arg() - phase).to_degrees()).abs())
        .fold(f64::INFINITY, f64::min)
}

pub(super) fn uca_layout(count: usize, beta: f64) -> (Layout, PhaseMode) {
    let geometry = uca_for_beta(count, beta, FREQ);
    let phase = PhaseMode::new(&geometry, FREQ).unwrap();
    (Layout::of(&geometry), phase)
}
