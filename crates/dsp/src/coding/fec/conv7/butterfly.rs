/// One sign mask per generator and butterfly: `-1` where that output bit is clear for the even
/// predecessor, so `(value ^ sign) - sign` turns a received soft value into its correlation.
pub(super) type Signs = [[i32; 32]; super::MAX_OUTPUTS];

#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub(super) fn step<const N: usize>(
    metrics: &[i32; 64],
    next: &mut [i32; 64],
    signs: &Signs,
    values: [i32; N],
) -> u64 {
    use std::arch::aarch64::*;
    unsafe {
        let received = values.map(|value| vdupq_n_s32(value));
        let weights = vld1q_u32([1, 2, 4, 8].as_ptr());
        let mut decisions = 0u64;
        for group in 0..8 {
            let offset = group * 4;
            let previous = vld2q_s32(metrics.as_ptr().add(offset * 2));
            let mut branch = vdupq_n_s32(0);
            for (value, sign) in received.iter().zip(signs) {
                let mask = vld1q_s32(sign.as_ptr().add(offset));
                branch = vaddq_s32(branch, vsubq_s32(veorq_s32(*value, mask), mask));
            }
            let a = vaddq_s32(previous.0, branch);
            let b = vsubq_s32(previous.1, branch);
            let c = vsubq_s32(previous.0, branch);
            let d = vaddq_s32(previous.1, branch);
            vst1q_s32(next.as_mut_ptr().add(offset), vmaxq_s32(a, b));
            vst1q_s32(next.as_mut_ptr().add(offset + 32), vmaxq_s32(c, d));
            decisions |= u64::from(vaddvq_u32(vandq_u32(vcgtq_s32(b, a), weights))) << offset;
            decisions |=
                u64::from(vaddvq_u32(vandq_u32(vcgtq_s32(d, c), weights))) << (offset + 32);
        }
        decisions
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(super) fn step<const N: usize>(
    metrics: &[i32; 64],
    next: &mut [i32; 64],
    signs: &Signs,
    values: [i32; N],
) -> u64 {
    use std::arch::x86_64::*;
    unsafe {
        let received = values.map(|value| _mm_set1_epi32(value));
        let mut decisions = 0u64;
        for group in 0..8 {
            let offset = group * 4;
            let p = _mm_castsi128_ps(_mm_loadu_si128(metrics.as_ptr().add(offset * 2).cast()));
            let q = _mm_castsi128_ps(_mm_loadu_si128(metrics.as_ptr().add(offset * 2 + 4).cast()));
            let even = _mm_castps_si128(_mm_shuffle_ps::<0x88>(p, q));
            let odd = _mm_castps_si128(_mm_shuffle_ps::<0xdd>(p, q));
            let mut branch = _mm_setzero_si128();
            for (value, sign) in received.iter().zip(signs) {
                let mask = _mm_loadu_si128(sign.as_ptr().add(offset).cast());
                branch = _mm_add_epi32(branch, _mm_sub_epi32(_mm_xor_si128(*value, mask), mask));
            }
            let a = _mm_add_epi32(even, branch);
            let b = _mm_sub_epi32(odd, branch);
            let c = _mm_sub_epi32(even, branch);
            let d = _mm_add_epi32(odd, branch);
            let low = _mm_cmpgt_epi32(b, a);
            let high = _mm_cmpgt_epi32(d, c);
            let best0 = _mm_or_si128(_mm_and_si128(low, b), _mm_andnot_si128(low, a));
            let best1 = _mm_or_si128(_mm_and_si128(high, d), _mm_andnot_si128(high, c));
            _mm_storeu_si128(next.as_mut_ptr().add(offset).cast(), best0);
            _mm_storeu_si128(next.as_mut_ptr().add(offset + 32).cast(), best1);
            decisions |= (_mm_movemask_ps(_mm_castsi128_ps(low)) as u64) << offset;
            decisions |= (_mm_movemask_ps(_mm_castsi128_ps(high)) as u64) << (offset + 32);
        }
        decisions
    }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
#[inline(always)]
pub(super) fn step<const N: usize>(
    metrics: &[i32; 64],
    next: &mut [i32; 64],
    signs: &Signs,
    values: [i32; N],
) -> u64 {
    let mut decisions = 0u64;
    for i in 0..32 {
        let branch: i32 = values
            .iter()
            .zip(signs)
            .map(|(&value, sign)| (value ^ sign[i]) - sign[i])
            .sum();
        let a = metrics[2 * i] + branch;
        let b = metrics[2 * i + 1] - branch;
        let c = metrics[2 * i] - branch;
        let d = metrics[2 * i + 1] + branch;
        next[i] = a.max(b);
        next[i + 32] = c.max(d);
        decisions |= u64::from(b > a) << i;
        decisions |= u64::from(d > c) << (i + 32);
    }
    decisions
}
