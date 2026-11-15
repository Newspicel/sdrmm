use num_complex::Complex;

use super::SampleCovariance;
use crate::array::manifold::{Direction, Vec3, steer};
use crate::math::linalg::{CMat, Eigen, HermitianEigen, testing::random_vector};
use crate::testutil::XorShift32;

pub(super) const FREQ: f64 = 433.92e6;

pub(super) fn random_lanes(lanes: usize, len: usize, seed: u32) -> Vec<Vec<Complex<f32>>> {
    let mut rng = XorShift32(seed);
    (0..lanes).map(|_| random_vector(&mut rng, len)).collect()
}

pub(super) fn covariance_of(lanes: &[Vec<Complex<f32>>]) -> CMat {
    let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
    let mut covariance = SampleCovariance::new(lanes.len()).unwrap();
    covariance.accumulate(&views);
    let mut r = CMat::zeros(1).unwrap();
    assert!(covariance.matrix(&mut r));
    r
}

fn music(r: &CMat, sources: usize, steering: &[Complex<f32>]) -> f32 {
    let n = r.order();
    let mut eigen = HermitianEigen::new(n).unwrap();
    let mut decomposition = Eigen::new();
    eigen.solve(r, &mut decomposition).unwrap();
    let noise: f32 = (0..n - sources)
        .map(|k| {
            let v = decomposition.vector(k);
            v.iter()
                .zip(steering)
                .map(|(e, a)| e.conj() * a)
                .sum::<Complex<f32>>()
                .norm_sqr()
        })
        .sum();
    steering.iter().map(Complex::norm_sqr).sum::<f32>() / noise.max(1e-12)
}

pub(super) fn music_at(r: &CMat, sources: usize, positions: &[Vec3], az: f64) -> f32 {
    let mut a = vec![Complex::new(0.0f32, 0.0); positions.len()];
    steer(positions, FREQ, Direction::horizon(az), &mut a);
    music(r, sources, &a)
}

pub(super) fn peaks(spectrum: &[f32], floor_ratio: f32) -> Vec<usize> {
    let top = spectrum.iter().copied().fold(0.0f32, f32::max);
    (1..spectrum.len() - 1)
        .filter(|&i| spectrum[i] > spectrum[i - 1] && spectrum[i] >= spectrum[i + 1])
        .filter(|&i| spectrum[i] >= top * floor_ratio)
        .collect()
}

pub(super) fn line_spectrum(r: &CMat, sources: usize, positions: &[Vec3]) -> Vec<f32> {
    (0..=180)
        .map(|az| music_at(r, sources, positions, f64::from(az)))
        .collect()
}

pub(super) fn sharpness(spectrum: &[f32]) -> f32 {
    let mut sorted = spectrum.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[sorted.len() - 1] / sorted[sorted.len() / 2]
}

pub(super) fn signal_count(r: &CMat) -> usize {
    let mut eigen = HermitianEigen::new(r.order()).unwrap();
    let mut decomposition = Eigen::new();
    eigen.solve(r, &mut decomposition).unwrap();
    let floor = decomposition.values()[0].max(1e-12);
    decomposition
        .values()
        .iter()
        .filter(|&&value| value > 20.0 * floor)
        .count()
}
