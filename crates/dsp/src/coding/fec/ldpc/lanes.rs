const LIMIT: i16 = 16_383;

/// The check-node update a [`super::QuasiCyclic`] decoder runs. Every kernel is bit exact
/// with [`Kernel::Scalar`]; the others process 8, 16 or 32 lanes at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kernel {
    Scalar,
    Vector,
    #[cfg(target_arch = "x86_64")]
    Wide,
    #[cfg(target_arch = "x86_64")]
    Wide512,
}

impl Kernel {
    #[must_use]
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx512bw")
            && std::arch::is_x86_feature_detected!("avx2")
        {
            return Self::Wide512;
        }
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            return Self::Wide;
        }
        Self::Vector
    }

    pub(super) fn update(
        self,
        stride: usize,
        gathered: &mut [i16],
        extrinsic: &mut [i16],
        messages: &mut [i16],
    ) -> u32 {
        let rows = messages.len() - messages.len() % stride;
        let messages = &mut messages[..rows];
        let gathered = &mut gathered[..rows];
        let extrinsic = &mut extrinsic[..rows];
        match self {
            Self::Scalar => scalar(stride, gathered, extrinsic, messages),
            Self::Vector => vector(stride, gathered, extrinsic, messages),
            #[cfg(target_arch = "x86_64")]
            Self::Wide => unsafe { wide(stride, gathered, extrinsic, messages) },
            #[cfg(target_arch = "x86_64")]
            Self::Wide512 => unsafe { wide_avx512(stride, gathered, extrinsic, messages) },
        }
    }
}

const fn scale(magnitude: i16) -> i16 {
    let scaled = magnitude - (magnitude >> 3);
    if scaled < LIMIT { scaled } else { LIMIT }
}

fn scalar(stride: usize, gathered: &mut [i16], extrinsic: &mut [i16], messages: &mut [i16]) -> u32 {
    let mut unsatisfied = 0;
    for lane in 0..stride {
        let mut first = i16::MAX;
        let mut second = i16::MAX;
        let mut sign = 0i16;
        let mut parity = 0i16;
        for at in (lane..messages.len()).step_by(stride) {
            let total = gathered[at];
            let value = total.saturating_sub(messages[at]);
            extrinsic[at] = value;
            sign ^= value;
            parity ^= total;
            let magnitude = value.saturating_abs();
            second = second.min(first.max(magnitude));
            first = first.min(magnitude);
        }
        unsatisfied += u32::from(parity < 0);
        let near = scale(first);
        let far = scale(second);
        for at in (lane..messages.len()).step_by(stride) {
            let value = extrinsic[at];
            let magnitude = if value.saturating_abs() == first {
                far
            } else {
                near
            };
            let flip = (sign ^ value) >> 15;
            let message = (magnitude ^ flip) - flip;
            gathered[at] = message.saturating_sub(messages[at]);
            messages[at] = message;
        }
    }
    unsatisfied
}

#[cfg(target_arch = "aarch64")]
fn vector(stride: usize, gathered: &mut [i16], extrinsic: &mut [i16], messages: &mut [i16]) -> u32 {
    use std::arch::aarch64::{
        vaddq_s16, vaddvq_s16, vbslq_s16, vceqq_s16, vdupq_n_s16, veorq_s16, vld1q_s16, vmaxq_s16,
        vminq_s16, vqabsq_s16, vqsubq_s16, vshrq_n_s16, vst1q_s16, vsubq_s16,
    };
    let rows = messages.len();
    let gathered = gathered.as_mut_ptr();
    let extrinsic = extrinsic.as_mut_ptr();
    let messages = messages.as_mut_ptr();
    unsafe {
        let limit = vdupq_n_s16(LIMIT);
        let mut count0 = vdupq_n_s16(0);
        let mut count1 = vdupq_n_s16(0);
        let mut lane = 0;
        while lane + 16 <= stride {
            let mut first0 = vdupq_n_s16(i16::MAX);
            let mut second0 = first0;
            let mut sign0 = vdupq_n_s16(0);
            let mut parity0 = sign0;
            let mut first1 = first0;
            let mut second1 = first0;
            let mut sign1 = sign0;
            let mut parity1 = sign0;
            for at in (lane..rows).step_by(stride) {
                let total0 = vld1q_s16(gathered.add(at));
                let total1 = vld1q_s16(gathered.add(at + 8));
                let value0 = vqsubq_s16(total0, vld1q_s16(messages.add(at)));
                let value1 = vqsubq_s16(total1, vld1q_s16(messages.add(at + 8)));
                vst1q_s16(extrinsic.add(at), value0);
                vst1q_s16(extrinsic.add(at + 8), value1);
                sign0 = veorq_s16(sign0, value0);
                sign1 = veorq_s16(sign1, value1);
                parity0 = veorq_s16(parity0, total0);
                parity1 = veorq_s16(parity1, total1);
                let mag0 = vqabsq_s16(value0);
                let mag1 = vqabsq_s16(value1);
                second0 = vminq_s16(second0, vmaxq_s16(first0, mag0));
                first0 = vminq_s16(first0, mag0);
                second1 = vminq_s16(second1, vmaxq_s16(first1, mag1));
                first1 = vminq_s16(first1, mag1);
            }
            count0 = vsubq_s16(count0, vshrq_n_s16::<15>(parity0));
            count1 = vsubq_s16(count1, vshrq_n_s16::<15>(parity1));
            let near0 = vminq_s16(vsubq_s16(first0, vshrq_n_s16::<3>(first0)), limit);
            let far0 = vminq_s16(vsubq_s16(second0, vshrq_n_s16::<3>(second0)), limit);
            let near1 = vminq_s16(vsubq_s16(first1, vshrq_n_s16::<3>(first1)), limit);
            let far1 = vminq_s16(vsubq_s16(second1, vshrq_n_s16::<3>(second1)), limit);
            for at in (lane..rows).step_by(stride) {
                let value0 = vld1q_s16(extrinsic.add(at));
                let value1 = vld1q_s16(extrinsic.add(at + 8));
                let mag0 = vbslq_s16(vceqq_s16(vqabsq_s16(value0), first0), far0, near0);
                let mag1 = vbslq_s16(vceqq_s16(vqabsq_s16(value1), first1), far1, near1);
                let flip0 = vshrq_n_s16::<15>(veorq_s16(sign0, value0));
                let flip1 = vshrq_n_s16::<15>(veorq_s16(sign1, value1));
                let msg0 = vsubq_s16(veorq_s16(mag0, flip0), flip0);
                let msg1 = vsubq_s16(veorq_s16(mag1, flip1), flip1);
                let prev0 = vld1q_s16(messages.add(at));
                let prev1 = vld1q_s16(messages.add(at + 8));
                vst1q_s16(gathered.add(at), vqsubq_s16(msg0, prev0));
                vst1q_s16(gathered.add(at + 8), vqsubq_s16(msg1, prev1));
                vst1q_s16(messages.add(at), msg0);
                vst1q_s16(messages.add(at + 8), msg1);
            }
            lane += 16;
        }
        while lane + 8 <= stride {
            let mut first = vdupq_n_s16(i16::MAX);
            let mut second = first;
            let mut sign = vdupq_n_s16(0);
            let mut parity = sign;
            for at in (lane..rows).step_by(stride) {
                let total = vld1q_s16(gathered.add(at));
                let value = vqsubq_s16(total, vld1q_s16(messages.add(at)));
                vst1q_s16(extrinsic.add(at), value);
                sign = veorq_s16(sign, value);
                parity = veorq_s16(parity, total);
                let magnitude = vqabsq_s16(value);
                second = vminq_s16(second, vmaxq_s16(first, magnitude));
                first = vminq_s16(first, magnitude);
            }
            count0 = vsubq_s16(count0, vshrq_n_s16::<15>(parity));
            let near = vminq_s16(vsubq_s16(first, vshrq_n_s16::<3>(first)), limit);
            let far = vminq_s16(vsubq_s16(second, vshrq_n_s16::<3>(second)), limit);
            for at in (lane..rows).step_by(stride) {
                let value = vld1q_s16(extrinsic.add(at));
                let magnitude = vbslq_s16(vceqq_s16(vqabsq_s16(value), first), far, near);
                let flip = vshrq_n_s16::<15>(veorq_s16(sign, value));
                let message = vsubq_s16(veorq_s16(magnitude, flip), flip);
                let previous = vld1q_s16(messages.add(at));
                vst1q_s16(gathered.add(at), vqsubq_s16(message, previous));
                vst1q_s16(messages.add(at), message);
            }
            lane += 8;
        }
        u32::from(vaddvq_s16(vaddq_s16(count0, count1)).unsigned_abs())
    }
}

#[cfg(target_arch = "x86_64")]
fn vector(stride: usize, gathered: &mut [i16], extrinsic: &mut [i16], messages: &mut [i16]) -> u32 {
    use std::arch::x86_64::{
        _mm_and_si128, _mm_andnot_si128, _mm_cmpeq_epi16, _mm_loadu_si128, _mm_max_epi16,
        _mm_min_epi16, _mm_or_si128, _mm_set1_epi16, _mm_setzero_si128, _mm_srai_epi16,
        _mm_storeu_si128, _mm_sub_epi16, _mm_subs_epi16, _mm_xor_si128,
    };
    let rows = messages.len();
    let gathered = gathered.as_mut_ptr();
    let extrinsic = extrinsic.as_mut_ptr();
    let messages = messages.as_mut_ptr();
    unsafe {
        let zero = _mm_setzero_si128();
        let limit = _mm_set1_epi16(LIMIT);
        let mut count = zero;
        for lane in (0..stride).step_by(8) {
            let mut first = _mm_set1_epi16(i16::MAX);
            let mut second = first;
            let mut sign = zero;
            let mut parity = zero;
            for at in (lane..rows).step_by(stride) {
                let total = _mm_loadu_si128(gathered.add(at).cast());
                let value = _mm_subs_epi16(total, _mm_loadu_si128(messages.add(at).cast()));
                _mm_storeu_si128(extrinsic.add(at).cast(), value);
                sign = _mm_xor_si128(sign, value);
                parity = _mm_xor_si128(parity, total);
                let magnitude = _mm_max_epi16(value, _mm_subs_epi16(zero, value));
                second = _mm_min_epi16(second, _mm_max_epi16(first, magnitude));
                first = _mm_min_epi16(first, magnitude);
            }
            count = _mm_sub_epi16(count, _mm_srai_epi16::<15>(parity));
            let near = _mm_min_epi16(_mm_sub_epi16(first, _mm_srai_epi16::<3>(first)), limit);
            let far = _mm_min_epi16(_mm_sub_epi16(second, _mm_srai_epi16::<3>(second)), limit);
            for at in (lane..rows).step_by(stride) {
                let value = _mm_loadu_si128(extrinsic.add(at).cast());
                let magnitude = _mm_max_epi16(value, _mm_subs_epi16(zero, value));
                let tied = _mm_cmpeq_epi16(magnitude, first);
                let chosen = _mm_or_si128(_mm_and_si128(tied, far), _mm_andnot_si128(tied, near));
                let flip = _mm_srai_epi16::<15>(_mm_xor_si128(sign, value));
                let message = _mm_sub_epi16(_mm_xor_si128(chosen, flip), flip);
                let previous = _mm_loadu_si128(messages.add(at).cast());
                _mm_storeu_si128(gathered.add(at).cast(), _mm_subs_epi16(message, previous));
                _mm_storeu_si128(messages.add(at).cast(), message);
            }
        }
        let mut lanes = [0i16; 8];
        _mm_storeu_si128(lanes.as_mut_ptr().cast(), count);
        lanes
            .iter()
            .map(|&lane| u32::from(lane.unsigned_abs()))
            .sum()
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn wide(stride: usize, gathered: &mut [i16], extrinsic: &mut [i16], messages: &mut [i16]) -> u32 {
    use std::arch::x86_64::{
        _mm256_add_epi16, _mm256_blendv_epi8, _mm256_cmpeq_epi16, _mm256_loadu_si256,
        _mm256_max_epi16, _mm256_min_epi16, _mm256_set1_epi16, _mm256_setzero_si256,
        _mm256_srai_epi16, _mm256_storeu_si256, _mm256_sub_epi16, _mm256_subs_epi16,
        _mm256_xor_si256,
    };
    let rows = messages.len();
    let gathered = gathered.as_mut_ptr();
    let extrinsic = extrinsic.as_mut_ptr();
    let messages = messages.as_mut_ptr();
    unsafe {
        let zero = _mm256_setzero_si256();
        let limit = _mm256_set1_epi16(LIMIT);
        let mut count0 = zero;
        let mut count1 = zero;
        let mut lane = 0;
        while lane + 32 <= stride {
            let mut first0 = _mm256_set1_epi16(i16::MAX);
            let mut second0 = first0;
            let mut sign0 = zero;
            let mut parity0 = zero;
            let mut first1 = first0;
            let mut second1 = first0;
            let mut sign1 = zero;
            let mut parity1 = zero;
            for at in (lane..rows).step_by(stride) {
                let total0 = _mm256_loadu_si256(gathered.add(at).cast());
                let total1 = _mm256_loadu_si256(gathered.add(at + 16).cast());
                let value0 = _mm256_subs_epi16(total0, _mm256_loadu_si256(messages.add(at).cast()));
                let value1 =
                    _mm256_subs_epi16(total1, _mm256_loadu_si256(messages.add(at + 16).cast()));
                _mm256_storeu_si256(extrinsic.add(at).cast(), value0);
                _mm256_storeu_si256(extrinsic.add(at + 16).cast(), value1);
                sign0 = _mm256_xor_si256(sign0, value0);
                sign1 = _mm256_xor_si256(sign1, value1);
                parity0 = _mm256_xor_si256(parity0, total0);
                parity1 = _mm256_xor_si256(parity1, total1);
                let mag0 = _mm256_max_epi16(value0, _mm256_subs_epi16(zero, value0));
                let mag1 = _mm256_max_epi16(value1, _mm256_subs_epi16(zero, value1));
                second0 = _mm256_min_epi16(second0, _mm256_max_epi16(first0, mag0));
                first0 = _mm256_min_epi16(first0, mag0);
                second1 = _mm256_min_epi16(second1, _mm256_max_epi16(first1, mag1));
                first1 = _mm256_min_epi16(first1, mag1);
            }
            count0 = _mm256_sub_epi16(count0, _mm256_srai_epi16::<15>(parity0));
            count1 = _mm256_sub_epi16(count1, _mm256_srai_epi16::<15>(parity1));
            let near0 = _mm256_min_epi16(
                _mm256_sub_epi16(first0, _mm256_srai_epi16::<3>(first0)),
                limit,
            );
            let far0 = _mm256_min_epi16(
                _mm256_sub_epi16(second0, _mm256_srai_epi16::<3>(second0)),
                limit,
            );
            let near1 = _mm256_min_epi16(
                _mm256_sub_epi16(first1, _mm256_srai_epi16::<3>(first1)),
                limit,
            );
            let far1 = _mm256_min_epi16(
                _mm256_sub_epi16(second1, _mm256_srai_epi16::<3>(second1)),
                limit,
            );
            for at in (lane..rows).step_by(stride) {
                let value0 = _mm256_loadu_si256(extrinsic.add(at).cast());
                let value1 = _mm256_loadu_si256(extrinsic.add(at + 16).cast());
                let mag0 = _mm256_max_epi16(value0, _mm256_subs_epi16(zero, value0));
                let mag1 = _mm256_max_epi16(value1, _mm256_subs_epi16(zero, value1));
                let chosen0 = _mm256_blendv_epi8(near0, far0, _mm256_cmpeq_epi16(mag0, first0));
                let chosen1 = _mm256_blendv_epi8(near1, far1, _mm256_cmpeq_epi16(mag1, first1));
                let flip0 = _mm256_srai_epi16::<15>(_mm256_xor_si256(sign0, value0));
                let flip1 = _mm256_srai_epi16::<15>(_mm256_xor_si256(sign1, value1));
                let msg0 = _mm256_sub_epi16(_mm256_xor_si256(chosen0, flip0), flip0);
                let msg1 = _mm256_sub_epi16(_mm256_xor_si256(chosen1, flip1), flip1);
                let prev0 = _mm256_loadu_si256(messages.add(at).cast());
                let prev1 = _mm256_loadu_si256(messages.add(at + 16).cast());
                _mm256_storeu_si256(gathered.add(at).cast(), _mm256_subs_epi16(msg0, prev0));
                _mm256_storeu_si256(gathered.add(at + 16).cast(), _mm256_subs_epi16(msg1, prev1));
                _mm256_storeu_si256(messages.add(at).cast(), msg0);
                _mm256_storeu_si256(messages.add(at + 16).cast(), msg1);
            }
            lane += 32;
        }
        while lane + 16 <= stride {
            let mut first = _mm256_set1_epi16(i16::MAX);
            let mut second = first;
            let mut sign = zero;
            let mut parity = zero;
            for at in (lane..rows).step_by(stride) {
                let total = _mm256_loadu_si256(gathered.add(at).cast());
                let value = _mm256_subs_epi16(total, _mm256_loadu_si256(messages.add(at).cast()));
                _mm256_storeu_si256(extrinsic.add(at).cast(), value);
                sign = _mm256_xor_si256(sign, value);
                parity = _mm256_xor_si256(parity, total);
                let magnitude = _mm256_max_epi16(value, _mm256_subs_epi16(zero, value));
                second = _mm256_min_epi16(second, _mm256_max_epi16(first, magnitude));
                first = _mm256_min_epi16(first, magnitude);
            }
            count0 = _mm256_sub_epi16(count0, _mm256_srai_epi16::<15>(parity));
            let near = _mm256_min_epi16(
                _mm256_sub_epi16(first, _mm256_srai_epi16::<3>(first)),
                limit,
            );
            let far = _mm256_min_epi16(
                _mm256_sub_epi16(second, _mm256_srai_epi16::<3>(second)),
                limit,
            );
            for at in (lane..rows).step_by(stride) {
                let value = _mm256_loadu_si256(extrinsic.add(at).cast());
                let magnitude = _mm256_max_epi16(value, _mm256_subs_epi16(zero, value));
                let tied = _mm256_cmpeq_epi16(magnitude, first);
                let chosen = _mm256_blendv_epi8(near, far, tied);
                let flip = _mm256_srai_epi16::<15>(_mm256_xor_si256(sign, value));
                let message = _mm256_sub_epi16(_mm256_xor_si256(chosen, flip), flip);
                let previous = _mm256_loadu_si256(messages.add(at).cast());
                _mm256_storeu_si256(
                    gathered.add(at).cast(),
                    _mm256_subs_epi16(message, previous),
                );
                _mm256_storeu_si256(messages.add(at).cast(), message);
            }
            lane += 16;
        }
        let mut lanes = [0i16; 16];
        _mm256_storeu_si256(lanes.as_mut_ptr().cast(), _mm256_add_epi16(count0, count1));
        lanes
            .iter()
            .map(|&lane| u32::from(lane.unsigned_abs()))
            .sum()
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512bw,avx2")]
fn wide_avx512(
    stride: usize,
    gathered: &mut [i16],
    extrinsic: &mut [i16],
    messages: &mut [i16],
) -> u32 {
    use std::arch::x86_64::{
        _mm256_blendv_epi8, _mm256_cmpeq_epi16, _mm256_loadu_si256, _mm256_max_epi16,
        _mm256_min_epi16, _mm256_set1_epi16, _mm256_setzero_si256, _mm256_srai_epi16,
        _mm256_storeu_si256, _mm256_sub_epi16, _mm256_subs_epi16, _mm256_xor_si256,
        _mm512_cmpeq_epi16_mask, _mm512_loadu_si512, _mm512_mask_blend_epi16, _mm512_max_epi16,
        _mm512_min_epi16, _mm512_set1_epi16, _mm512_setzero_si512, _mm512_srai_epi16,
        _mm512_storeu_si512, _mm512_sub_epi16, _mm512_subs_epi16, _mm512_xor_si512,
    };
    let rows = messages.len();
    let gathered = gathered.as_mut_ptr();
    let extrinsic = extrinsic.as_mut_ptr();
    let messages = messages.as_mut_ptr();
    unsafe {
        let zero = _mm512_setzero_si512();
        let limit = _mm512_set1_epi16(LIMIT);
        let mut count = zero;
        let mut lane = 0;
        while lane + 32 <= stride {
            let mut first = _mm512_set1_epi16(i16::MAX);
            let mut second = first;
            let mut sign = zero;
            let mut parity = zero;
            for at in (lane..rows).step_by(stride) {
                let total = _mm512_loadu_si512(gathered.add(at).cast());
                let value = _mm512_subs_epi16(total, _mm512_loadu_si512(messages.add(at).cast()));
                _mm512_storeu_si512(extrinsic.add(at).cast(), value);
                sign = _mm512_xor_si512(sign, value);
                parity = _mm512_xor_si512(parity, total);
                let magnitude = _mm512_max_epi16(value, _mm512_subs_epi16(zero, value));
                second = _mm512_min_epi16(second, _mm512_max_epi16(first, magnitude));
                first = _mm512_min_epi16(first, magnitude);
            }
            count = _mm512_sub_epi16(count, _mm512_srai_epi16::<15>(parity));
            let near = _mm512_min_epi16(
                _mm512_sub_epi16(first, _mm512_srai_epi16::<3>(first)),
                limit,
            );
            let far = _mm512_min_epi16(
                _mm512_sub_epi16(second, _mm512_srai_epi16::<3>(second)),
                limit,
            );
            for at in (lane..rows).step_by(stride) {
                let value = _mm512_loadu_si512(extrinsic.add(at).cast());
                let magnitude = _mm512_max_epi16(value, _mm512_subs_epi16(zero, value));
                let tied = _mm512_cmpeq_epi16_mask(magnitude, first);
                let chosen = _mm512_mask_blend_epi16(tied, near, far);
                let flip = _mm512_srai_epi16::<15>(_mm512_xor_si512(sign, value));
                let message = _mm512_sub_epi16(_mm512_xor_si512(chosen, flip), flip);
                let previous = _mm512_loadu_si512(messages.add(at).cast());
                _mm512_storeu_si512(
                    gathered.add(at).cast(),
                    _mm512_subs_epi16(message, previous),
                );
                _mm512_storeu_si512(messages.add(at).cast(), message);
            }
            lane += 32;
        }
        let mut total_unsat: u32 = {
            let mut lanes = [0i16; 32];
            _mm512_storeu_si512(lanes.as_mut_ptr().cast(), count);
            lanes.iter().map(|&l| u32::from(l.unsigned_abs())).sum()
        };
        if lane + 16 <= stride {
            let zero256 = _mm256_setzero_si256();
            let limit256 = _mm256_set1_epi16(LIMIT);
            let mut first = _mm256_set1_epi16(i16::MAX);
            let mut second = first;
            let mut sign = zero256;
            let mut parity = zero256;
            for at in (lane..rows).step_by(stride) {
                let total = _mm256_loadu_si256(gathered.add(at).cast());
                let value = _mm256_subs_epi16(total, _mm256_loadu_si256(messages.add(at).cast()));
                _mm256_storeu_si256(extrinsic.add(at).cast(), value);
                sign = _mm256_xor_si256(sign, value);
                parity = _mm256_xor_si256(parity, total);
                let magnitude = _mm256_max_epi16(value, _mm256_subs_epi16(zero256, value));
                second = _mm256_min_epi16(second, _mm256_max_epi16(first, magnitude));
                first = _mm256_min_epi16(first, magnitude);
            }
            let tail_count = _mm256_sub_epi16(zero256, _mm256_srai_epi16::<15>(parity));
            let near = _mm256_min_epi16(
                _mm256_sub_epi16(first, _mm256_srai_epi16::<3>(first)),
                limit256,
            );
            let far = _mm256_min_epi16(
                _mm256_sub_epi16(second, _mm256_srai_epi16::<3>(second)),
                limit256,
            );
            for at in (lane..rows).step_by(stride) {
                let value = _mm256_loadu_si256(extrinsic.add(at).cast());
                let magnitude = _mm256_max_epi16(value, _mm256_subs_epi16(zero256, value));
                let tied = _mm256_cmpeq_epi16(magnitude, first);
                let chosen = _mm256_blendv_epi8(near, far, tied);
                let flip = _mm256_srai_epi16::<15>(_mm256_xor_si256(sign, value));
                let message = _mm256_sub_epi16(_mm256_xor_si256(chosen, flip), flip);
                let previous = _mm256_loadu_si256(messages.add(at).cast());
                _mm256_storeu_si256(
                    gathered.add(at).cast(),
                    _mm256_subs_epi16(message, previous),
                );
                _mm256_storeu_si256(messages.add(at).cast(), message);
            }
            let mut lanes = [0i16; 16];
            _mm256_storeu_si256(lanes.as_mut_ptr().cast(), tail_count);
            total_unsat += lanes
                .iter()
                .map(|&l| u32::from(l.unsigned_abs()))
                .sum::<u32>();
        }
        total_unsat
    }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
fn vector(stride: usize, gathered: &mut [i16], extrinsic: &mut [i16], messages: &mut [i16]) -> u32 {
    scalar(stride, gathered, extrinsic, messages)
}
