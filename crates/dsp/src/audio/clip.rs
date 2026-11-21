use num_complex::Complex;

pub const FULL_SCALE: f32 = 0.995;
const CLIPPING_FRACTION: u64 = 10_000;

#[must_use]
pub fn clipped_and_peak(samples: &[Complex<f32>]) -> (u64, f32) {
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{
            vabsq_f32, vaddvq_u32, vcgeq_f32, vdupq_n_f32, vdupq_n_u32, vld2q_f32, vmaxq_f32,
            vmaxvq_f32, vsubq_u32,
        };
        let (chunks, tail) = samples.as_chunks::<4>();
        let mut clipped = 0u64;
        let mut peak = 0.0f32;
        unsafe {
            let limit = vdupq_n_f32(FULL_SCALE);
            let mut vmax = vdupq_n_f32(0.0);
            for block in chunks.chunks(65_536) {
                let mut vcount = vdupq_n_u32(0);
                for quad in block {
                    let v = vld2q_f32(quad.as_ptr().cast());
                    let rail = vmaxq_f32(vabsq_f32(v.0), vabsq_f32(v.1));
                    vmax = vmaxq_f32(vmax, rail);
                    vcount = vsubq_u32(vcount, vcgeq_f32(rail, limit));
                }
                clipped += u64::from(vaddvq_u32(vcount));
            }
            peak = peak.max(vmaxvq_f32(vmax));
        }
        for sample in tail {
            let rail = sample.re.abs().max(sample.im.abs());
            clipped += u64::from(rail >= FULL_SCALE);
            peak = peak.max(rail);
        }
        (clipped, peak)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        samples.iter().fold((0, 0.0), |(clipped, peak), sample| {
            let rail = sample.re.abs().max(sample.im.abs());
            (clipped + u64::from(rail >= FULL_SCALE), peak.max(rail))
        })
    }
}

#[must_use]
pub fn clipping(clipped: u64, samples: u64) -> bool {
    clipped > 0 && clipped.saturating_mul(CLIPPING_FRACTION) >= samples
}

#[must_use]
pub fn clips(samples: &[Complex<f32>]) -> bool {
    clipping(clipped_and_peak(samples).0, samples.len() as u64)
}

/// Holds every sample to the rails a sound card takes.
pub fn clamp_full_scale(pcm: &mut [f32]) {
    for s in pcm {
        *s = s.clamp(-1.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_clamping_stops_at_the_rails_and_keeps_the_rest() {
        let mut pcm = [-3.0, -0.5, 0.0, 0.25, 1.0, 7.5];
        clamp_full_scale(&mut pcm);
        assert_eq!(pcm, [-1.0, -0.5, 0.0, 0.25, 1.0, 1.0]);
    }

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
