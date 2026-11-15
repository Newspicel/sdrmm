use num_complex::Complex;
use sdrmm_dsp::math::complex::widen;

use super::Constellation;

pub fn gray_4pam() -> Constellation {
    Constellation::from_points(
        vec![
            Complex::new(-3.0, 0.0),
            Complex::new(-1.0, 0.0),
            Complex::new(1.0, 0.0),
            Complex::new(3.0, 0.0),
        ],
        vec![0b00, 0b01, 0b11, 0b10],
    )
    .unwrap()
}

pub fn mean_energy(c: &Constellation) -> f64 {
    c.points().iter().map(|&p| widen(p).norm_sqr()).sum::<f64>() / c.len() as f64
}
