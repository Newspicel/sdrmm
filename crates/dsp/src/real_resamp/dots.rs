//! Batches of resampler dot products, each output with its own window and branch.

use super::Clock;

/// Outputs computed side by side, so one horizontal reduction serves them all.
pub(super) const BATCH: usize = 8;

/// The widest dot kernel this CPU runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dots {
    #[cfg(not(target_arch = "aarch64"))]
    Portable,
    #[cfg(target_arch = "aarch64")]
    Neon,
    #[cfg(target_arch = "x86_64")]
    Avx2,
}

impl Dots {
    pub(super) fn detect() -> Self {
        #[cfg(target_arch = "aarch64")]
        {
            Self::Neon
        }
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
                Self::Avx2
            } else {
                Self::Portable
            }
        }
        #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
        {
            Self::Portable
        }
    }

    /// Fills `out`, a whole number of batches, one output per clock tick.
    ///
    /// # Safety
    /// Every tick's window `samples[start..start + N]` and branch `rows[phase * N..][..N]`
    /// must lie inside its slice.
    pub(super) unsafe fn fill<const N: usize>(
        self,
        samples: &[f32],
        rows: &[f32],
        clock: &mut Clock,
        out: &mut [f32],
    ) {
        match self {
            #[cfg(not(target_arch = "aarch64"))]
            Self::Portable => portable::fill::<N>(samples, rows, clock, out),
            #[cfg(target_arch = "aarch64")]
            Self::Neon => unsafe { neon::fill::<N>(samples, rows, clock, out) },
            #[cfg(target_arch = "x86_64")]
            Self::Avx2 => unsafe { avx2::fill::<N>(samples, rows, clock, out) },
        }
    }
}

/// Four running sums folded pairwise, the order the NEON kernel keeps too.
#[inline(always)]
pub(super) fn dot(samples: &[f32], taps: &[f32]) -> f32 {
    let mut sum = [0.0f32; 4];
    for (x, h) in samples
        .as_chunks::<4>()
        .0
        .iter()
        .zip(taps.as_chunks::<4>().0)
    {
        for lane in 0..4 {
            sum[lane] = crate::madd(x[lane], h[lane], sum[lane]);
        }
    }
    (sum[0] + sum[1]) + (sum[2] + sum[3])
}

#[cfg(not(target_arch = "aarch64"))]
mod portable {
    use super::{Clock, dot};

    pub(super) fn fill<const N: usize>(
        samples: &[f32],
        rows: &[f32],
        clock: &mut Clock,
        out: &mut [f32],
    ) {
        for slot in out {
            let (start, phase) = clock.next();
            *slot = dot(
                &samples[start..start + N],
                &rows[phase * N..(phase + 1) * N],
            );
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use std::arch::aarch64::{
        float32x4_t, vdupq_n_f32, vfmaq_f32, vld1q_f32, vpaddq_f32, vst1q_f32,
    };

    use super::{BATCH, Clock};

    /// # Safety
    /// As [`super::Dots::fill`].
    #[target_feature(enable = "neon")]
    pub(super) unsafe fn fill<const N: usize>(
        samples: &[f32],
        rows: &[f32],
        clock: &mut Clock,
        out: &mut [f32],
    ) {
        for batch in out.as_chunks_mut::<BATCH>().0 {
            let mut sums = [vdupq_n_f32(0.0); BATCH];
            for sum in &mut sums {
                let (start, phase) = clock.next();
                let (x, h) = (
                    samples.as_ptr().wrapping_add(start),
                    rows.as_ptr().wrapping_add(phase * N),
                );
                for at in (0..N / 4).map(|k| 4 * k) {
                    unsafe {
                        *sum = vfmaq_f32(*sum, vld1q_f32(x.add(at)), vld1q_f32(h.add(at)));
                    }
                }
            }
            let fold = |a: float32x4_t, b: float32x4_t, c: float32x4_t, d: float32x4_t| {
                vpaddq_f32(vpaddq_f32(a, b), vpaddq_f32(c, d))
            };
            unsafe {
                vst1q_f32(batch.as_mut_ptr(), fold(sums[0], sums[1], sums[2], sums[3]));
                vst1q_f32(
                    batch.as_mut_ptr().add(4),
                    fold(sums[4], sums[5], sums[6], sums[7]),
                );
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod avx2 {
    use std::arch::x86_64::{
        __m256, _mm_loadu_ps, _mm_mul_ps, _mm256_add_ps, _mm256_fmadd_ps, _mm256_hadd_ps,
        _mm256_loadu_ps, _mm256_permute2f128_ps, _mm256_setzero_ps, _mm256_storeu_ps,
        _mm256_zextps128_ps256,
    };

    use super::{BATCH, Clock};

    /// # Safety
    /// As [`super::Dots::fill`].
    #[target_feature(enable = "avx2,fma")]
    pub(super) unsafe fn fill<const N: usize>(
        samples: &[f32],
        rows: &[f32],
        clock: &mut Clock,
        out: &mut [f32],
    ) {
        for batch in out.as_chunks_mut::<BATCH>().0 {
            let mut sums = [_mm256_setzero_ps(); BATCH];
            for sum in &mut sums {
                let (start, phase) = clock.next();
                let (x, h) = (
                    samples.as_ptr().wrapping_add(start),
                    rows.as_ptr().wrapping_add(phase * N),
                );
                for at in (0..N / 8).map(|k| 8 * k) {
                    unsafe {
                        *sum = _mm256_fmadd_ps(
                            _mm256_loadu_ps(x.add(at)),
                            _mm256_loadu_ps(h.add(at)),
                            *sum,
                        );
                    }
                }
                if N % 8 == 4 {
                    unsafe {
                        let tail =
                            _mm_mul_ps(_mm_loadu_ps(x.add(N - 4)), _mm_loadu_ps(h.add(N - 4)));
                        *sum = _mm256_add_ps(*sum, _mm256_zextps128_ps256(tail));
                    }
                }
            }
            unsafe { _mm256_storeu_ps(batch.as_mut_ptr(), reduce(&sums)) };
        }
    }

    /// Eight vectors to the eight sums of their lanes, in order.
    #[target_feature(enable = "avx2,fma")]
    fn reduce(s: &[__m256; BATCH]) -> __m256 {
        let pairs = [
            _mm256_hadd_ps(s[0], s[1]),
            _mm256_hadd_ps(s[2], s[3]),
            _mm256_hadd_ps(s[4], s[5]),
            _mm256_hadd_ps(s[6], s[7]),
        ];
        let low = _mm256_hadd_ps(pairs[0], pairs[1]);
        let high = _mm256_hadd_ps(pairs[2], pairs[3]);
        _mm256_add_ps(
            _mm256_permute2f128_ps::<0x20>(low, high),
            _mm256_permute2f128_ps::<0x31>(low, high),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FRACTION_BITS, PHASES};
    use super::*;

    fn check<const N: usize>() {
        let samples: Vec<f32> = (0..400)
            .map(|k| ((k * 37) % 23) as f32 / 7.0 - 1.5)
            .collect();
        let rows: Vec<f32> = (0..(PHASES + 1) * N)
            .map(|k| ((k * 13) % 17) as f32 / 9.0 - 0.8)
            .collect();
        let start = Clock {
            time: (N as u64) << FRACTION_BITS,
            step: 0x1_3A5C_2B71,
            half: N / 2 - 1,
        };
        let mut clock = start;
        let mut got = [0.0f32; 4 * BATCH];
        unsafe { Dots::detect().fill::<N>(&samples, &rows, &mut clock, &mut got) };
        let mut reference = start;
        for (k, &value) in got.iter().enumerate() {
            let (at, phase) = reference.next();
            let want = dot(&samples[at..at + N], &rows[phase * N..(phase + 1) * N]);
            assert!(
                (value - want).abs() < 1e-4,
                "N {N} output {k}: {value} vs {want}"
            );
        }
        assert_eq!(clock.time, reference.time);
    }

    #[test]
    fn the_batched_kernel_matches_one_dot_at_a_time() {
        check::<8>();
        check::<12>();
        check::<16>();
        check::<20>();
        check::<24>();
        check::<32>();
    }
}
