use crate::coding::fec::conv::Soft;

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
    use std::arch::aarch64::{
        uint32x4_t, vaddq_s32, vaddv_u8, vandq_u8, vcgtq_s32, vdupq_n_s32, veorq_s32, vget_high_u8,
        vget_low_u8, vld1q_s32, vld1q_u8, vld2q_s32, vmaxq_s32, vreinterpretq_u8_u16,
        vreinterpretq_u16_u32, vst1q_s32, vsubq_s32, vuzp1q_u8, vuzp1q_u16,
    };
    const BIT_WEIGHTS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];

    #[inline(always)]
    unsafe fn pack16(
        c0: uint32x4_t,
        c1: uint32x4_t,
        c2: uint32x4_t,
        c3: uint32x4_t,
        weights: std::arch::aarch64::uint8x16_t,
    ) -> u64 {
        unsafe {
            let p01 = vuzp1q_u16(vreinterpretq_u16_u32(c0), vreinterpretq_u16_u32(c1));
            let p23 = vuzp1q_u16(vreinterpretq_u16_u32(c2), vreinterpretq_u16_u32(c3));
            let bits = vandq_u8(
                vuzp1q_u8(vreinterpretq_u8_u16(p01), vreinterpretq_u8_u16(p23)),
                weights,
            );
            let lo = u64::from(vaddv_u8(vget_low_u8(bits)));
            let hi = u64::from(vaddv_u8(vget_high_u8(bits)));
            lo | (hi << 8)
        }
    }

    unsafe {
        let received = values.map(|value| vdupq_n_s32(value));
        let weights = vld1q_u8(BIT_WEIGHTS.as_ptr());
        let mp = metrics.as_ptr();
        let np = next.as_mut_ptr();
        let mut decisions = 0u64;
        for half in 0..2 {
            let base = half * 16;
            let mut low_cmp = [vcgtq_s32(vdupq_n_s32(0), vdupq_n_s32(0)); 4];
            let mut high_cmp = low_cmp;
            for g in 0..4 {
                let offset = base + g * 4;
                let previous = vld2q_s32(mp.add(offset * 2));
                let mut branch = vdupq_n_s32(0);
                for (value, sign) in received.iter().zip(signs) {
                    let mask = vld1q_s32(sign.as_ptr().add(offset));
                    branch = vaddq_s32(branch, vsubq_s32(veorq_s32(*value, mask), mask));
                }
                let a = vaddq_s32(previous.0, branch);
                let b = vsubq_s32(previous.1, branch);
                let c = vsubq_s32(previous.0, branch);
                let d = vaddq_s32(previous.1, branch);
                vst1q_s32(np.add(offset), vmaxq_s32(a, b));
                vst1q_s32(np.add(offset + 32), vmaxq_s32(c, d));
                low_cmp[g] = vcgtq_s32(b, a);
                high_cmp[g] = vcgtq_s32(d, c);
            }
            decisions |= pack16(low_cmp[0], low_cmp[1], low_cmp[2], low_cmp[3], weights) << base;
            decisions |=
                pack16(high_cmp[0], high_cmp[1], high_cmp[2], high_cmp[3], weights) << (base + 32);
        }
        decisions
    }
}

#[cfg(target_arch = "aarch64")]
pub(super) fn run_steps<const N: usize>(
    survivors: &mut [[i32; 64]; 2],
    front: &mut usize,
    signs: &Signs,
    coded: &[Soft],
    decisions: &mut Vec<u64>,
) {
    let (chunks, _) = coded.as_chunks::<N>();
    let steps = chunks.len();
    decisions.reserve(steps);
    let base = decisions.len();
    let dst = decisions.spare_capacity_mut().as_mut_ptr().cast::<u64>();
    let mut cur = *front;
    for (idx, chunk) in chunks.iter().enumerate() {
        let [even, odd] = survivors;
        let (metrics, next) = if cur == 0 {
            (&*even, odd)
        } else {
            (&*odd, even)
        };
        let values = std::array::from_fn(|i| i32::from(chunk[i]));
        let word = step::<N>(metrics, next, signs, values);
        unsafe { dst.add(idx).write(word) };
        cur ^= 1;
    }
    unsafe { decisions.set_len(base + steps) };
    *front = cur;
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
#[inline]
unsafe fn step_avx512<const N: usize>(
    metrics: &[i32; 64],
    next: &mut [i32; 64],
    signs: &Signs,
    values: [i32; N],
) -> u64 {
    use std::arch::x86_64::{
        _mm512_add_epi32, _mm512_cmpgt_epi32_mask, _mm512_loadu_si512, _mm512_max_epi32,
        _mm512_permutex2var_epi32, _mm512_set1_epi32, _mm512_setzero_si512, _mm512_storeu_si512,
        _mm512_sub_epi32, _mm512_xor_si512,
    };
    const EVEN_IDX: [i32; 16] = [0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30];
    const ODD_IDX: [i32; 16] = [1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31];
    let even_idx = unsafe { _mm512_loadu_si512(EVEN_IDX.as_ptr().cast()) };
    let odd_idx = unsafe { _mm512_loadu_si512(ODD_IDX.as_ptr().cast()) };
    let received = values.map(|value| _mm512_set1_epi32(value));
    let mp = metrics.as_ptr();
    let np = next.as_mut_ptr();
    let mut decisions = 0u64;
    for half in 0..2 {
        let offset = half * 16;
        let p0 = unsafe { _mm512_loadu_si512(mp.add(offset * 2).cast()) };
        let p1 = unsafe { _mm512_loadu_si512(mp.add(offset * 2 + 16).cast()) };
        let even = _mm512_permutex2var_epi32(p0, even_idx, p1);
        let odd = _mm512_permutex2var_epi32(p0, odd_idx, p1);
        let mut branch = _mm512_setzero_si512();
        for (value, sign) in received.iter().zip(signs) {
            let mask = unsafe { _mm512_loadu_si512(sign.as_ptr().add(offset).cast()) };
            branch = _mm512_add_epi32(
                branch,
                _mm512_sub_epi32(_mm512_xor_si512(*value, mask), mask),
            );
        }
        let a = _mm512_add_epi32(even, branch);
        let b = _mm512_sub_epi32(odd, branch);
        let c = _mm512_sub_epi32(even, branch);
        let d = _mm512_add_epi32(odd, branch);
        unsafe {
            _mm512_storeu_si512(np.add(offset).cast(), _mm512_max_epi32(a, b));
            _mm512_storeu_si512(np.add(offset + 32).cast(), _mm512_max_epi32(c, d));
        }
        let low = _mm512_cmpgt_epi32_mask(b, a);
        let high = _mm512_cmpgt_epi32_mask(d, c);
        decisions |= u64::from(low) << offset;
        decisions |= u64::from(high) << (offset + 32);
    }
    decisions
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn step_avx2<const N: usize>(
    metrics: &[i32; 64],
    next: &mut [i32; 64],
    signs: &Signs,
    values: [i32; N],
) -> u64 {
    use std::arch::x86_64::{
        _mm256_add_epi32, _mm256_castpd_si256, _mm256_castps_pd, _mm256_castsi256_ps,
        _mm256_cmpgt_epi32, _mm256_loadu_si256, _mm256_max_epi32, _mm256_movemask_ps,
        _mm256_permute4x64_pd, _mm256_set1_epi32, _mm256_setzero_si256, _mm256_shuffle_ps,
        _mm256_storeu_si256, _mm256_sub_epi32, _mm256_xor_si256,
    };
    let received = values.map(|value| _mm256_set1_epi32(value));
    let mp = metrics.as_ptr();
    let np = next.as_mut_ptr();
    let mut decisions = 0u64;
    for group in 0..4 {
        let offset = group * 8;
        let p = _mm256_castsi256_ps(unsafe { _mm256_loadu_si256(mp.add(offset * 2).cast()) });
        let q = _mm256_castsi256_ps(unsafe { _mm256_loadu_si256(mp.add(offset * 2 + 8).cast()) });
        let even = _mm256_castpd_si256(_mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(
            _mm256_shuffle_ps::<0x88>(p, q),
        )));
        let odd = _mm256_castpd_si256(_mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(
            _mm256_shuffle_ps::<0xDD>(p, q),
        )));
        let mut branch = _mm256_setzero_si256();
        for (value, sign) in received.iter().zip(signs) {
            let mask = unsafe { _mm256_loadu_si256(sign.as_ptr().add(offset).cast()) };
            branch = _mm256_add_epi32(
                branch,
                _mm256_sub_epi32(_mm256_xor_si256(*value, mask), mask),
            );
        }
        let a = _mm256_add_epi32(even, branch);
        let b = _mm256_sub_epi32(odd, branch);
        let c = _mm256_sub_epi32(even, branch);
        let d = _mm256_add_epi32(odd, branch);
        let low = _mm256_cmpgt_epi32(b, a);
        let high = _mm256_cmpgt_epi32(d, c);
        unsafe {
            _mm256_storeu_si256(np.add(offset).cast(), _mm256_max_epi32(a, b));
            _mm256_storeu_si256(np.add(offset + 32).cast(), _mm256_max_epi32(c, d));
        }
        decisions |= u64::from(_mm256_movemask_ps(_mm256_castsi256_ps(low)) as u32) << offset;
        decisions |=
            u64::from(_mm256_movemask_ps(_mm256_castsi256_ps(high)) as u32) << (offset + 32);
    }
    decisions
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn step_sse2<const N: usize>(
    metrics: &[i32; 64],
    next: &mut [i32; 64],
    signs: &Signs,
    values: [i32; N],
) -> u64 {
    use std::arch::x86_64::{
        _mm_add_epi32, _mm_and_si128, _mm_andnot_si128, _mm_castps_si128, _mm_castsi128_ps,
        _mm_cmpgt_epi32, _mm_loadu_si128, _mm_movemask_ps, _mm_or_si128, _mm_set1_epi32,
        _mm_setzero_si128, _mm_shuffle_ps, _mm_storeu_si128, _mm_sub_epi32, _mm_xor_si128,
    };
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
            decisions |= u64::from(_mm_movemask_ps(_mm_castsi128_ps(low)) as u32) << offset;
            decisions |= u64::from(_mm_movemask_ps(_mm_castsi128_ps(high)) as u32) << (offset + 32);
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
    if std::arch::is_x86_feature_detected!("avx512f") {
        unsafe { step_avx512(metrics, next, signs, values) }
    } else if std::arch::is_x86_feature_detected!("avx2") {
        unsafe { step_avx2(metrics, next, signs, values) }
    } else {
        step_sse2(metrics, next, signs, values)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn run_steps_avx512<const N: usize>(
    survivors: &mut [[i32; 64]; 2],
    front: &mut usize,
    signs: &Signs,
    coded: &[Soft],
    decisions: &mut Vec<u64>,
) {
    let (chunks, _) = coded.as_chunks::<N>();
    let steps = chunks.len();
    decisions.reserve(steps);
    let base = decisions.len();
    let dst = decisions.spare_capacity_mut().as_mut_ptr().cast::<u64>();
    let mut cur = *front;
    for (idx, chunk) in chunks.iter().enumerate() {
        let [even, odd] = survivors;
        let (metrics, next) = if cur == 0 {
            (&*even, odd)
        } else {
            (&*odd, even)
        };
        let values = std::array::from_fn(|i| i32::from(chunk[i]));
        let word = unsafe { step_avx512::<N>(metrics, next, signs, values) };
        unsafe { dst.add(idx).write(word) };
        cur ^= 1;
    }
    unsafe { decisions.set_len(base + steps) };
    *front = cur;
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn run_steps_avx2<const N: usize>(
    survivors: &mut [[i32; 64]; 2],
    front: &mut usize,
    signs: &Signs,
    coded: &[Soft],
    decisions: &mut Vec<u64>,
) {
    let (chunks, _) = coded.as_chunks::<N>();
    let steps = chunks.len();
    decisions.reserve(steps);
    let base = decisions.len();
    let dst = decisions.spare_capacity_mut().as_mut_ptr().cast::<u64>();
    let mut cur = *front;
    for (idx, chunk) in chunks.iter().enumerate() {
        let [even, odd] = survivors;
        let (metrics, next) = if cur == 0 {
            (&*even, odd)
        } else {
            (&*odd, even)
        };
        let values = std::array::from_fn(|i| i32::from(chunk[i]));
        let word = unsafe { step_avx2::<N>(metrics, next, signs, values) };
        unsafe { dst.add(idx).write(word) };
        cur ^= 1;
    }
    unsafe { decisions.set_len(base + steps) };
    *front = cur;
}

#[cfg(target_arch = "x86_64")]
pub(super) fn run_steps<const N: usize>(
    survivors: &mut [[i32; 64]; 2],
    front: &mut usize,
    signs: &Signs,
    coded: &[Soft],
    decisions: &mut Vec<u64>,
) {
    if std::arch::is_x86_feature_detected!("avx512f") {
        unsafe { run_steps_avx512::<N>(survivors, front, signs, coded, decisions) };
    } else if std::arch::is_x86_feature_detected!("avx2") {
        unsafe { run_steps_avx2::<N>(survivors, front, signs, coded, decisions) };
    } else {
        let (chunks, _) = coded.as_chunks::<N>();
        decisions.reserve(chunks.len());
        let mut cur = *front;
        for chunk in chunks {
            let [even, odd] = survivors;
            let (metrics, next) = if cur == 0 {
                (&*even, odd)
            } else {
                (&*odd, even)
            };
            let values = std::array::from_fn(|i| i32::from(chunk[i]));
            decisions.push(step_sse2::<N>(metrics, next, signs, values));
            cur ^= 1;
        }
        *front = cur;
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

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
pub(super) fn run_steps<const N: usize>(
    survivors: &mut [[i32; 64]; 2],
    front: &mut usize,
    signs: &Signs,
    coded: &[Soft],
    decisions: &mut Vec<u64>,
) {
    let (chunks, _) = coded.as_chunks::<N>();
    decisions.reserve(chunks.len());
    let mut cur = *front;
    for chunk in chunks {
        let [even, odd] = survivors;
        let (metrics, next) = if cur == 0 {
            (&*even, odd)
        } else {
            (&*odd, even)
        };
        let values = std::array::from_fn(|i| i32::from(chunk[i]));
        decisions.push(step::<N>(metrics, next, signs, values));
        cur ^= 1;
    }
    *front = cur;
}
