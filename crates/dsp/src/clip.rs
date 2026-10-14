use num_complex::Complex;

pub const FULL_SCALE: f32 = 0.995;
const CLIPPING_FRACTION: u64 = 10_000;

#[must_use]
pub fn clipped_and_peak(samples: &[Complex<f32>]) -> (u64, f32) {
    samples.iter().fold((0, 0.0), |(clipped, peak), sample| {
        let rail = sample.re.abs().max(sample.im.abs());
        (clipped + u64::from(rail >= FULL_SCALE), peak.max(rail))
    })
}

#[must_use]
pub fn clipping(clipped: u64, samples: u64) -> bool {
    clipped > 0 && clipped.saturating_mul(CLIPPING_FRACTION) >= samples
}

#[must_use]
pub fn clips(samples: &[Complex<f32>]) -> bool {
    clipping(clipped_and_peak(samples).0, samples.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet(len: usize) -> Vec<Complex<f32>> {
        vec![Complex::new(0.1, -0.2); len]
    }

    #[test]
    fn a_quiet_block_does_not_clip() {
        assert!(!clips(&quiet(100_000)));
    }

    #[test]
    fn a_sample_on_either_rail_clips() {
        let mut block = quiet(1_000);
        block[10] = Complex::new(0.0, -0.9992);
        assert!(clips(&block));
    }

    #[test]
    fn a_single_spike_in_a_long_block_does_not_clip() {
        let mut block = quiet(100_000);
        block[0] = Complex::new(1.0, 0.0);
        assert!(!clips(&block));
    }

    #[test]
    fn the_peak_is_the_loudest_rail() {
        let (clipped, peak) = clipped_and_peak(&[Complex::new(0.1, -0.5), Complex::new(0.3, 0.2)]);
        assert_eq!(clipped, 0);
        assert!((peak - 0.5).abs() < 1e-6);
    }
}
