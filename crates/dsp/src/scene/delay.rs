use crate::special::{bessel_i0, sinc};

pub const KAISER_BETA: f64 = 8.0;

#[derive(Clone, Copy, Debug)]
pub struct KaiserSinc {
    half: f64,
    beta: f64,
    scale: f64,
}

impl KaiserSinc {
    #[must_use]
    pub fn new(half: f64, beta: f64) -> Self {
        Self {
            half,
            beta,
            scale: bessel_i0(beta),
        }
    }

    #[must_use]
    pub fn tap(&self, distance: f64) -> f64 {
        let ratio = distance / self.half;
        if ratio.abs() >= 1.0 {
            return 0.0;
        }
        sinc(distance) * bessel_i0(self.beta * (1.0 - ratio * ratio).sqrt()) / self.scale
    }
}

pub fn normalise(taps: &mut [f64]) {
    let sum: f64 = taps.iter().sum();
    if sum == 0.0 {
        return;
    }
    for tap in taps.iter_mut() {
        *tap /= sum;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_sample_shift_keeps_one_tap_and_drops_the_rest() {
        let design = KaiserSinc::new(16.0, KAISER_BETA);
        assert!((design.tap(0.0) - 1.0).abs() < 1e-12);
        for distance in [1.0, -1.0, 7.0, -11.0] {
            assert!(design.tap(distance).abs() < 1e-12, "{distance}");
        }
        assert_eq!(design.tap(16.0), 0.0);
        assert_eq!(design.tap(-20.0), 0.0);
    }

    #[test]
    fn a_half_sample_shift_is_symmetric_and_sums_to_unity() {
        let design = KaiserSinc::new(16.0, KAISER_BETA);
        let mut taps: Vec<f64> = (0..32).map(|k| design.tap(k as f64 - 15.5)).collect();
        for (left, right) in taps.iter().zip(taps.iter().rev()) {
            assert!((left - right).abs() < 1e-12);
        }
        normalise(&mut taps);
        assert!((taps.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn normalising_nothing_leaves_it_alone() {
        let mut taps = vec![0.0f64; 8];
        normalise(&mut taps);
        assert!(taps.iter().all(|tap| *tap == 0.0));
    }
}
