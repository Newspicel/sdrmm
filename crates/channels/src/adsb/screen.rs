use sdrmm_modem::ppm::PpmDemod;

use super::{PHASE_TABLES, PREAMBLE_PULSES};

pub(super) type Phases = u8;

const _: () = assert!(PHASE_TABLES <= Phases::BITS as usize);

const TILE: usize = 256;
const SCREENED_GAPS: [usize; 4] = [4, 5, 12, 14];

struct Stream {
    weights: Vec<f32>,
    reach: usize,
    values: Vec<f32>,
}

impl Stream {
    fn fill(&mut self, mag: &[f32], from: usize, n: usize) -> Option<()> {
        let len = n + self.reach;
        let samples = mag.get(from..from + len + self.weights.len().saturating_sub(1))?;
        let values = self.values.get_mut(..len)?;
        match *self.weights.as_slice() {
            [w0, w1] => fill_2(values, samples, w0, w1),
            [w0, w1, w2] => fill_3(values, samples, w0, w1, w2),
            _ => {
                values.fill(0.0);
                for (k, &w) in self.weights.iter().enumerate() {
                    for (v, &x) in values.iter_mut().zip(&samples[k..]) {
                        *v = sdrmm_dsp::madd(w, x, *v);
                    }
                }
            }
        }
        Some(())
    }
}

fn fill_2(values: &mut [f32], samples: &[f32], w0: f32, w1: f32) {
    #[cfg(target_arch = "aarch64")]
    let done = {
        use std::arch::aarch64::{
            vdupq_n_f32, vextq_f32, vfmaq_f32, vld1q_f32, vmulq_f32, vst1q_f32,
        };
        let n = values.len() & !7;
        let mut done = 0;
        unsafe {
            let sp = samples.as_ptr();
            let dp = values.as_mut_ptr();
            let vw0 = vdupq_n_f32(w0);
            let vw1 = vdupq_n_f32(w1);
            if n > 0 {
                let mut cur = vld1q_f32(sp);
                while done < n {
                    let mid = vld1q_f32(sp.add(done + 4));
                    let next = vld1q_f32(sp.add(done + 8));
                    let s0 = vextq_f32::<1>(cur, mid);
                    let s1 = vextq_f32::<1>(mid, next);
                    vst1q_f32(dp.add(done), vfmaq_f32(vmulq_f32(cur, vw0), s0, vw1));
                    vst1q_f32(dp.add(done + 4), vfmaq_f32(vmulq_f32(mid, vw0), s1, vw1));
                    cur = next;
                    done += 8;
                }
            }
            while done + 4 <= values.len() {
                let x0 = vld1q_f32(sp.add(done));
                let x1 = vld1q_f32(sp.add(done + 1));
                vst1q_f32(dp.add(done), vfmaq_f32(vmulq_f32(x0, vw0), x1, vw1));
                done += 4;
            }
        }
        done
    };
    #[cfg(target_arch = "x86_64")]
    let done = if std::arch::is_x86_feature_detected!("avx2")
        && std::arch::is_x86_feature_detected!("fma")
    {
        unsafe { fill_2_avx2(values, samples, w0, w1) }
    } else {
        fill_2_sse2(values, samples, w0, w1)
    };
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    let done = 0;
    for ((v, &x0), &x1) in values[done..]
        .iter_mut()
        .zip(&samples[done..])
        .zip(&samples[done + 1..])
    {
        *v = sdrmm_dsp::madd(w1, x1, w0 * x0);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn fill_2_avx2(values: &mut [f32], samples: &[f32], w0: f32, w1: f32) -> usize {
    use std::arch::x86_64::{
        _mm256_fmadd_ps, _mm256_loadu_ps, _mm256_mul_ps, _mm256_set1_ps, _mm256_storeu_ps,
    };
    let n = values.len() & !15;
    let sp = samples.as_ptr();
    let dp = values.as_mut_ptr();
    let vw0 = _mm256_set1_ps(w0);
    let vw1 = _mm256_set1_ps(w1);
    let mut done = 0;
    while done < n {
        unsafe {
            let a0 = _mm256_loadu_ps(sp.add(done));
            let a1 = _mm256_loadu_ps(sp.add(done + 1));
            let b0 = _mm256_loadu_ps(sp.add(done + 8));
            let b1 = _mm256_loadu_ps(sp.add(done + 9));
            _mm256_storeu_ps(
                dp.add(done),
                _mm256_fmadd_ps(a1, vw1, _mm256_mul_ps(a0, vw0)),
            );
            _mm256_storeu_ps(
                dp.add(done + 8),
                _mm256_fmadd_ps(b1, vw1, _mm256_mul_ps(b0, vw0)),
            );
        }
        done += 16;
    }
    while done + 8 <= values.len() {
        unsafe {
            let a0 = _mm256_loadu_ps(sp.add(done));
            let a1 = _mm256_loadu_ps(sp.add(done + 1));
            _mm256_storeu_ps(
                dp.add(done),
                _mm256_fmadd_ps(a1, vw1, _mm256_mul_ps(a0, vw0)),
            );
        }
        done += 8;
    }
    done
}

#[cfg(target_arch = "x86_64")]
fn fill_2_sse2(values: &mut [f32], samples: &[f32], w0: f32, w1: f32) -> usize {
    use std::arch::x86_64::{_mm_add_ps, _mm_loadu_ps, _mm_mul_ps, _mm_set1_ps, _mm_storeu_ps};
    let n = values.len() & !3;
    let mut done = 0;
    unsafe {
        let sp = samples.as_ptr();
        let dp = values.as_mut_ptr();
        let vw0 = _mm_set1_ps(w0);
        let vw1 = _mm_set1_ps(w1);
        while done < n {
            let a0 = _mm_loadu_ps(sp.add(done));
            let a1 = _mm_loadu_ps(sp.add(done + 1));
            _mm_storeu_ps(
                dp.add(done),
                _mm_add_ps(_mm_mul_ps(a0, vw0), _mm_mul_ps(a1, vw1)),
            );
            done += 4;
        }
    }
    done
}

fn fill_3(values: &mut [f32], samples: &[f32], w0: f32, w1: f32, w2: f32) {
    #[cfg(target_arch = "aarch64")]
    let done = {
        use std::arch::aarch64::{
            vdupq_n_f32, vextq_f32, vfmaq_f32, vld1q_f32, vmulq_f32, vst1q_f32,
        };
        let n = values.len() & !7;
        let mut done = 0;
        unsafe {
            let sp = samples.as_ptr();
            let dp = values.as_mut_ptr();
            let vw0 = vdupq_n_f32(w0);
            let vw1 = vdupq_n_f32(w1);
            let vw2 = vdupq_n_f32(w2);
            if n > 0 {
                let mut cur = vld1q_f32(sp);
                while done < n {
                    let mid = vld1q_f32(sp.add(done + 4));
                    let next = vld1q_f32(sp.add(done + 8));
                    let a1 = vextq_f32::<1>(cur, mid);
                    let a2 = vextq_f32::<2>(cur, mid);
                    let b1 = vextq_f32::<1>(mid, next);
                    let b2 = vextq_f32::<2>(mid, next);
                    let r0 = vfmaq_f32(vfmaq_f32(vmulq_f32(cur, vw0), a1, vw1), a2, vw2);
                    let r1 = vfmaq_f32(vfmaq_f32(vmulq_f32(mid, vw0), b1, vw1), b2, vw2);
                    vst1q_f32(dp.add(done), r0);
                    vst1q_f32(dp.add(done + 4), r1);
                    cur = next;
                    done += 8;
                }
            }
            while done + 4 <= values.len() {
                let a0 = vld1q_f32(sp.add(done));
                let a1 = vld1q_f32(sp.add(done + 1));
                let a2 = vld1q_f32(sp.add(done + 2));
                let r = vfmaq_f32(vfmaq_f32(vmulq_f32(a0, vw0), a1, vw1), a2, vw2);
                vst1q_f32(dp.add(done), r);
                done += 4;
            }
        }
        done
    };
    #[cfg(target_arch = "x86_64")]
    let done = if std::arch::is_x86_feature_detected!("avx2")
        && std::arch::is_x86_feature_detected!("fma")
    {
        unsafe { fill_3_avx2(values, samples, w0, w1, w2) }
    } else {
        fill_3_sse2(values, samples, w0, w1, w2)
    };
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    let done = 0;
    for (((v, &x0), &x1), &x2) in values[done..]
        .iter_mut()
        .zip(&samples[done..])
        .zip(&samples[done + 1..])
        .zip(&samples[done + 2..])
    {
        *v = sdrmm_dsp::madd(w2, x2, sdrmm_dsp::madd(w1, x1, w0 * x0));
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn fill_3_avx2(values: &mut [f32], samples: &[f32], w0: f32, w1: f32, w2: f32) -> usize {
    use std::arch::x86_64::{
        _mm256_fmadd_ps, _mm256_loadu_ps, _mm256_mul_ps, _mm256_set1_ps, _mm256_storeu_ps,
    };
    let n = values.len() & !15;
    let sp = samples.as_ptr();
    let dp = values.as_mut_ptr();
    let vw0 = _mm256_set1_ps(w0);
    let vw1 = _mm256_set1_ps(w1);
    let vw2 = _mm256_set1_ps(w2);
    let mut done = 0;
    while done < n {
        unsafe {
            let a0 = _mm256_loadu_ps(sp.add(done));
            let a1 = _mm256_loadu_ps(sp.add(done + 1));
            let a2 = _mm256_loadu_ps(sp.add(done + 2));
            let b0 = _mm256_loadu_ps(sp.add(done + 8));
            let b1 = _mm256_loadu_ps(sp.add(done + 9));
            let b2 = _mm256_loadu_ps(sp.add(done + 10));
            let r0 = _mm256_fmadd_ps(a2, vw2, _mm256_fmadd_ps(a1, vw1, _mm256_mul_ps(a0, vw0)));
            let r1 = _mm256_fmadd_ps(b2, vw2, _mm256_fmadd_ps(b1, vw1, _mm256_mul_ps(b0, vw0)));
            _mm256_storeu_ps(dp.add(done), r0);
            _mm256_storeu_ps(dp.add(done + 8), r1);
        }
        done += 16;
    }
    while done + 8 <= values.len() {
        unsafe {
            let a0 = _mm256_loadu_ps(sp.add(done));
            let a1 = _mm256_loadu_ps(sp.add(done + 1));
            let a2 = _mm256_loadu_ps(sp.add(done + 2));
            let r = _mm256_fmadd_ps(a2, vw2, _mm256_fmadd_ps(a1, vw1, _mm256_mul_ps(a0, vw0)));
            _mm256_storeu_ps(dp.add(done), r);
        }
        done += 8;
    }
    done
}

#[cfg(target_arch = "x86_64")]
fn fill_3_sse2(values: &mut [f32], samples: &[f32], w0: f32, w1: f32, w2: f32) -> usize {
    use std::arch::x86_64::{_mm_add_ps, _mm_loadu_ps, _mm_mul_ps, _mm_set1_ps, _mm_storeu_ps};
    let n = values.len() & !3;
    let mut done = 0;
    unsafe {
        let sp = samples.as_ptr();
        let dp = values.as_mut_ptr();
        let vw0 = _mm_set1_ps(w0);
        let vw1 = _mm_set1_ps(w1);
        let vw2 = _mm_set1_ps(w2);
        while done < n {
            let a0 = _mm_loadu_ps(sp.add(done));
            let a1 = _mm_loadu_ps(sp.add(done + 1));
            let a2 = _mm_loadu_ps(sp.add(done + 2));
            let r = _mm_add_ps(
                _mm_add_ps(_mm_mul_ps(a0, vw0), _mm_mul_ps(a1, vw1)),
                _mm_mul_ps(a2, vw2),
            );
            _mm_storeu_ps(dp.add(done), r);
            done += 4;
        }
    }
    done
}

#[derive(Clone, Copy)]
struct ChipRef {
    stream: usize,
    start: usize,
}

struct PhaseChips {
    pulses: [ChipRef; PREAMBLE_PULSES.len()],
    gaps: [ChipRef; SCREENED_GAPS.len()],
}

impl PhaseChips {
    fn mark(&self, streams: &[Stream], phase: usize, hits: &mut [Phases]) -> bool {
        let len = hits.len();
        let (Some(p), Some(g)) = (
            slices(&self.pulses, streams, len),
            slices(&self.gaps, streams, len),
        ) else {
            for hit in &mut *hits {
                *hit |= 1 << phase;
            }
            return !hits.is_empty();
        };
        let bit = 1u8 << phase;
        let mut any_hit = false;
        #[cfg(target_arch = "aarch64")]
        let mut i = {
            use std::arch::aarch64::{
                vaddq_f32, vandq_u32, vcgtq_f32, vcltq_f32, vdupq_n_f32, vld1q_f32, vmaxq_f32,
                vmaxvq_u32, vminq_f32, vmulq_f32, vorrq_u32, vst1q_u32,
            };
            let n8 = len & !7;
            let mut i = 0;
            unsafe {
                let scale = vdupq_n_f32(0.125);
                let (p0, p1, p2, p3) = (p[0].as_ptr(), p[1].as_ptr(), p[2].as_ptr(), p[3].as_ptr());
                let (g0, g1, g2, g3) = (g[0].as_ptr(), g[1].as_ptr(), g[2].as_ptr(), g[3].as_ptr());
                while i < n8 {
                    let vp0a = vld1q_f32(p0.add(i));
                    let vp1a = vld1q_f32(p1.add(i));
                    let vp2a = vld1q_f32(p2.add(i));
                    let vp3a = vld1q_f32(p3.add(i));
                    let weak_a = vminq_f32(vminq_f32(vp0a, vp1a), vminq_f32(vp2a, vp3a));
                    let thresh_a = vmulq_f32(
                        vaddq_f32(vaddq_f32(vp0a, vp1a), vaddq_f32(vp2a, vp3a)),
                        scale,
                    );
                    let ga = vmaxq_f32(vld1q_f32(g0.add(i)), vld1q_f32(g1.add(i)));
                    let ma = vandq_u32(vcgtq_f32(weak_a, thresh_a), vcltq_f32(ga, thresh_a));

                    let vp0b = vld1q_f32(p0.add(i + 4));
                    let vp1b = vld1q_f32(p1.add(i + 4));
                    let vp2b = vld1q_f32(p2.add(i + 4));
                    let vp3b = vld1q_f32(p3.add(i + 4));
                    let weak_b = vminq_f32(vminq_f32(vp0b, vp1b), vminq_f32(vp2b, vp3b));
                    let thresh_b = vmulq_f32(
                        vaddq_f32(vaddq_f32(vp0b, vp1b), vaddq_f32(vp2b, vp3b)),
                        scale,
                    );
                    let gb = vmaxq_f32(vld1q_f32(g0.add(i + 4)), vld1q_f32(g1.add(i + 4)));
                    let mb = vandq_u32(vcgtq_f32(weak_b, thresh_b), vcltq_f32(gb, thresh_b));

                    if vmaxvq_u32(vorrq_u32(ma, mb)) != 0 {
                        let ga2 = vmaxq_f32(vld1q_f32(g2.add(i)), vld1q_f32(g3.add(i)));
                        let gb2 = vmaxq_f32(vld1q_f32(g2.add(i + 4)), vld1q_f32(g3.add(i + 4)));
                        let fa = vandq_u32(ma, vcltq_f32(ga2, thresh_a));
                        let fb = vandq_u32(mb, vcltq_f32(gb2, thresh_b));
                        if vmaxvq_u32(vorrq_u32(fa, fb)) != 0 {
                            any_hit = true;
                            let mut lanes = [0u32; 8];
                            vst1q_u32(lanes.as_mut_ptr(), fa);
                            vst1q_u32(lanes.as_mut_ptr().add(4), fb);
                            for (k, &lane) in lanes.iter().enumerate() {
                                if lane != 0 {
                                    hits[i + k] |= bit;
                                }
                            }
                        }
                    }
                    i += 8;
                }
            }
            i
        };
        #[cfg(target_arch = "x86_64")]
        let mut i = if std::arch::is_x86_feature_detected!("avx2") {
            let (done, found) = unsafe { mark_avx2(p, g, bit, hits) };
            any_hit |= found;
            done
        } else {
            let (done, found) = mark_sse2(p, g, bit, hits);
            any_hit |= found;
            done
        };
        #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
        let mut i = 0;
        while i < len {
            let weakest = p[0][i].min(p[1][i]).min(p[2][i]).min(p[3][i]);
            let threshold = (p[0][i] + p[1][i] + p[2][i] + p[3][i]) * 0.125;
            if weakest > threshold {
                let loudest_gap = g[0][i].max(g[1][i]).max(g[2][i]).max(g[3][i]);
                if loudest_gap < threshold {
                    hits[i] |= bit;
                    any_hit = true;
                }
            }
            i += 1;
        }
        any_hit
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn mark_avx2(p: [&[f32]; 4], g: [&[f32]; 4], bit: u8, hits: &mut [Phases]) -> (usize, bool) {
    use std::arch::x86_64::{
        _CMP_GT_OQ, _CMP_LT_OQ, _mm256_add_ps, _mm256_and_ps, _mm256_cmp_ps, _mm256_loadu_ps,
        _mm256_max_ps, _mm256_min_ps, _mm256_movemask_ps, _mm256_mul_ps, _mm256_set1_ps,
    };
    let len = hits.len();
    let n8 = len & !7;
    let scale = _mm256_set1_ps(0.125);
    let (p0, p1, p2, p3) = (p[0].as_ptr(), p[1].as_ptr(), p[2].as_ptr(), p[3].as_ptr());
    let (g0, g1, g2, g3) = (g[0].as_ptr(), g[1].as_ptr(), g[2].as_ptr(), g[3].as_ptr());
    let mut i = 0;
    let mut any_hit = false;
    while i < n8 {
        let (vp0, vp1, vp2, vp3, vg0, vg1) = unsafe {
            (
                _mm256_loadu_ps(p0.add(i)),
                _mm256_loadu_ps(p1.add(i)),
                _mm256_loadu_ps(p2.add(i)),
                _mm256_loadu_ps(p3.add(i)),
                _mm256_loadu_ps(g0.add(i)),
                _mm256_loadu_ps(g1.add(i)),
            )
        };
        let weakest = _mm256_min_ps(_mm256_min_ps(vp0, vp1), _mm256_min_ps(vp2, vp3));
        let threshold = _mm256_mul_ps(
            _mm256_add_ps(_mm256_add_ps(vp0, vp1), _mm256_add_ps(vp2, vp3)),
            scale,
        );
        let g01 = _mm256_max_ps(vg0, vg1);
        let m01 = _mm256_and_ps(
            _mm256_cmp_ps::<_CMP_GT_OQ>(weakest, threshold),
            _mm256_cmp_ps::<_CMP_LT_OQ>(g01, threshold),
        );
        if _mm256_movemask_ps(m01) != 0 {
            let (vg2, vg3) = unsafe { (_mm256_loadu_ps(g2.add(i)), _mm256_loadu_ps(g3.add(i))) };
            let g23 = _mm256_max_ps(vg2, vg3);
            let m = _mm256_and_ps(m01, _mm256_cmp_ps::<_CMP_LT_OQ>(g23, threshold));
            let mut mask = _mm256_movemask_ps(m) as u32;
            if mask != 0 {
                any_hit = true;
                while mask != 0 {
                    let lane = mask.trailing_zeros() as usize;
                    hits[i + lane] |= bit;
                    mask &= mask - 1;
                }
            }
        }
        i += 8;
    }
    (i, any_hit)
}

#[cfg(target_arch = "x86_64")]
fn mark_sse2(p: [&[f32]; 4], g: [&[f32]; 4], bit: u8, hits: &mut [Phases]) -> (usize, bool) {
    use std::arch::x86_64::{
        _mm_add_ps, _mm_and_ps, _mm_cmpgt_ps, _mm_cmplt_ps, _mm_loadu_ps, _mm_max_ps, _mm_min_ps,
        _mm_movemask_ps, _mm_mul_ps, _mm_set1_ps,
    };
    let len = hits.len();
    let n4 = len & !3;
    let mut i = 0;
    let mut any_hit = false;
    unsafe {
        let scale = _mm_set1_ps(0.125);
        let (p0, p1, p2, p3) = (p[0].as_ptr(), p[1].as_ptr(), p[2].as_ptr(), p[3].as_ptr());
        let (g0, g1, g2, g3) = (g[0].as_ptr(), g[1].as_ptr(), g[2].as_ptr(), g[3].as_ptr());
        while i < n4 {
            let vp0 = _mm_loadu_ps(p0.add(i));
            let vp1 = _mm_loadu_ps(p1.add(i));
            let vp2 = _mm_loadu_ps(p2.add(i));
            let vp3 = _mm_loadu_ps(p3.add(i));
            let weakest = _mm_min_ps(_mm_min_ps(vp0, vp1), _mm_min_ps(vp2, vp3));
            let threshold = _mm_mul_ps(
                _mm_add_ps(_mm_add_ps(vp0, vp1), _mm_add_ps(vp2, vp3)),
                scale,
            );
            let g01 = _mm_max_ps(_mm_loadu_ps(g0.add(i)), _mm_loadu_ps(g1.add(i)));
            let m01 = _mm_and_ps(
                _mm_cmpgt_ps(weakest, threshold),
                _mm_cmplt_ps(g01, threshold),
            );
            if _mm_movemask_ps(m01) != 0 {
                let g23 = _mm_max_ps(_mm_loadu_ps(g2.add(i)), _mm_loadu_ps(g3.add(i)));
                let m = _mm_and_ps(m01, _mm_cmplt_ps(g23, threshold));
                let mut mask = _mm_movemask_ps(m) as u32;
                if mask != 0 {
                    any_hit = true;
                    while mask != 0 {
                        let lane = mask.trailing_zeros() as usize;
                        hits[i + lane] |= bit;
                        mask &= mask - 1;
                    }
                }
            }
            i += 4;
        }
    }
    (i, any_hit)
}

fn slices<'a, const N: usize>(
    chips: &[ChipRef; N],
    streams: &'a [Stream],
    len: usize,
) -> Option<[&'a [f32]; N]> {
    let mut out: [&'a [f32]; N] = [&[]; N];
    for (tile, chip) in out.iter_mut().zip(chips) {
        *tile = streams
            .get(chip.stream)?
            .values
            .get(chip.start..chip.start + len)?;
    }
    Some(out)
}

pub(super) struct PreambleScreen {
    streams: Vec<Stream>,
    phases: Vec<PhaseChips>,
    hits: Vec<(usize, Phases)>,
}

impl PreambleScreen {
    pub(super) fn new(receivers: &[PpmDemod]) -> Self {
        let mut streams: Vec<Stream> = Vec::new();
        let phases = receivers
            .iter()
            .map(|receiver| {
                let mut chip = |slot| share(&mut streams, receiver, slot);
                PhaseChips {
                    pulses: PREAMBLE_PULSES.map(&mut chip),
                    gaps: SCREENED_GAPS.map(&mut chip),
                }
            })
            .collect();
        for stream in &mut streams {
            stream.values = vec![0.0; TILE + stream.reach];
        }
        Self {
            streams,
            phases,
            hits: Vec::new(),
        }
    }

    pub(super) fn scan(&mut self, mag: &[f32], positions: usize) {
        self.hits.clear();
        let mut tile_hits = [0 as Phases; TILE];
        let mut from = 0;
        while from < positions {
            let len = (positions - from).min(TILE);
            if self
                .streams
                .iter_mut()
                .any(|stream| stream.fill(mag, from, len).is_none())
            {
                for offset in 0..len {
                    self.hits.push((from + offset, Phases::MAX));
                }
                from += len;
                continue;
            }
            let buf = &mut tile_hits[..len];
            buf.fill(0);
            let mut any = false;
            for (phase, chips) in self.phases.iter().enumerate() {
                any |= chips.mark(&self.streams, phase, buf);
            }
            if any {
                for (offset, &mask) in buf.iter().enumerate() {
                    if mask != 0 {
                        self.hits.push((from + offset, mask));
                    }
                }
            }
            from += len;
        }
    }

    pub(super) fn next(&self, from: usize) -> Option<(usize, Phases)> {
        let index = self.hits.partition_point(|&(pos, _)| pos < from);
        self.hits.get(index).copied()
    }
}

fn share(streams: &mut Vec<Stream>, receiver: &PpmDemod, slot: usize) -> ChipRef {
    let (start, weights) = receiver.grid().slot_weights(slot);
    let index = streams
        .iter()
        .position(|stream| stream.weights == weights)
        .unwrap_or_else(|| {
            streams.push(Stream {
                weights: weights.to_vec(),
                reach: 0,
                values: Vec::new(),
            });
            streams.len() - 1
        });
    if let Some(stream) = streams.get_mut(index) {
        stream.reach = stream.reach.max(start);
    }
    ChipRef {
        stream: index,
        start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adsb::{INPUT_RATE_HZ, phase_tables, preamble_ok},
        synth::adsb::{me_identification, squitter, transmission_at_phase},
    };
    use sdrmm_modem::ppm::magnitudes;
    use sdrmm_modem::synth::add_noise;

    #[test]
    fn every_position_a_phase_accepts_survives_the_screen() {
        let receivers = phase_tables(INPUT_RATE_HZ);
        let span = receivers.iter().map(|r| r.grid().span()).max().unwrap();
        let mut screen = PreambleScreen::new(&receivers);
        let frames: Vec<Vec<u8>> = (0..40)
            .map(|k| squitter(0xA0_0000 + k, me_identification("SCREEN")))
            .collect();
        for (seed, amp) in [(1, 0.02), (2, 0.15), (3, 0.3)] {
            let phase = f64::from(seed) / 7.0;
            let mut iq = transmission_at_phase(&frames, 40.0, 0.5, INPUT_RATE_HZ, phase);
            add_noise(&mut iq, seed, amp);
            let mut mag = Vec::new();
            magnitudes(&iq, &mut mag);
            let positions = mag.len() + 1 - span;
            screen.scan(&mag, positions);
            let mut accepted = 0;
            let mut screened = 0;
            for at in 0..positions {
                let kept = match screen.next(at) {
                    Some((hit, phases)) if hit == at => phases,
                    _ => 0,
                };
                for (phase, r) in receivers.iter().enumerate() {
                    let exact = preamble_ok(r, &mag[at..at + r.grid().span()]);
                    assert!(
                        !exact || kept >> phase & 1 == 1,
                        "seed {seed}: {at} lost phase {phase}"
                    );
                    accepted += usize::from(exact);
                }
                screened += usize::from(kept != 0);
            }
            assert!(accepted > 0, "seed {seed}: no preamble");
            assert!(
                screened * 50 < positions,
                "seed {seed}: {screened} of {positions} kept"
            );
        }
    }

    #[test]
    fn a_tile_without_samples_is_kept_whole() {
        let receivers = phase_tables(INPUT_RATE_HZ);
        let mut screen = PreambleScreen::new(&receivers);
        screen.scan(&[0.0; 4], 3);
        assert_eq!(screen.next(0), Some((0, Phases::MAX)));
        assert_eq!(screen.next(2), Some((2, Phases::MAX)));
        assert_eq!(screen.next(3), None);
    }
}
