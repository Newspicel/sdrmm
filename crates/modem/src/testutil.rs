use std::f32::consts::FRAC_1_SQRT_2;

use num_complex::Complex;
use sdrmm_dsp::math::rng::Rng as DspRng;
use sdrmm_modem_test_support::ber::{
    impair::{Awgn, Impairment},
    rng::Rng,
};

use crate::constellation::Constellation;

/// `count` uniform draws below `modulus`.
pub(crate) fn draws<T: TryFrom<u64>>(count: usize, modulus: usize, seed: u64) -> Vec<T> {
    let mut rng = Rng::new(seed);
    (0..count)
        .map(|_| {
            T::try_from(rng.next_u64() % modulus as u64)
                .unwrap_or_else(|_| panic!("modulus {modulus} does not fit the draw type"))
        })
        .collect()
}

pub(crate) fn points(table: &Constellation, count: usize, seed: u64) -> Vec<Complex<f32>> {
    let points = table.points();
    let mut rng = DspRng::mixed(seed);
    (0..count)
        .map(|_| points[(rng.bits() % points.len() as u64) as usize])
        .collect()
}

pub(crate) fn add_noise(wave: &mut Vec<Complex<f32>>, seed: u64, noise_var: f64) {
    Awgn::with_sigma((noise_var / 2.0).sqrt()).apply(wave, &mut Rng::new(seed));
}

pub(crate) fn qpsk(count: usize, seed: u64) -> Vec<Complex<f32>> {
    let mut rng = DspRng::mixed(seed);
    let sign = |set: bool| if set { -1.0 } else { 1.0 };
    (0..count)
        .map(|_| {
            let bits = rng.bits();
            Complex::new(sign(bits & 1 == 1), sign(bits & 2 == 2)) * FRAC_1_SQRT_2
        })
        .collect()
}

/// Points decided wrong, plus every point that never came out.
pub(crate) fn symbol_errors(
    table: &Constellation,
    sent: &[Complex<f32>],
    got: &[Complex<f32>],
) -> usize {
    let wrong = sent
        .iter()
        .zip(got)
        .filter(|(a, b)| table.hard_slice(**a) != table.hard_slice(**b))
        .count();
    wrong + sent.len().saturating_sub(got.len())
}
