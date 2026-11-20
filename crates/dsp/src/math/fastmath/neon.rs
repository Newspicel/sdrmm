use std::arch::aarch64::{
    float32x4_t, vabsq_f32, vaddq_f32, vandq_u32, vbicq_u32, vbslq_f32, vceqq_f32, vcgtq_f32,
    vcltq_f32, vdivq_f32, vdupq_n_f32, vdupq_n_u32, vextq_f32, vfmaq_f32, vld2q_f32, vmulq_f32,
    vorrq_u32, vreinterpretq_f32_u32, vreinterpretq_u32_f32, vst1q_f32, vsubq_f32,
};

use num_complex::Complex;

use super::atan::ATAN_UNIT;

#[inline(always)]
fn atan2(y: float32x4_t, x: float32x4_t) -> float32x4_t {
    unsafe {
        let ax = vabsq_f32(x);
        let ay = vabsq_f32(y);
        let steep = vcgtq_f32(ay, ax);
        let lo = vbslq_f32(steep, ax, ay);
        let hi = vbslq_f32(steep, ay, ax);
        let level = vceqq_f32(ax, ay);
        let ratio = vbslq_f32(level, vdupq_n_f32(1.0), vdivq_f32(lo, hi));
        let s = vmulq_f32(ratio, ratio);
        let s2 = vmulq_f32(s, s);
        let s4 = vmulq_f32(s2, s2);
        let p01 = vfmaq_f32(vdupq_n_f32(ATAN_UNIT[0]), s, vdupq_n_f32(ATAN_UNIT[1]));
        let p23 = vfmaq_f32(vdupq_n_f32(ATAN_UNIT[2]), s, vdupq_n_f32(ATAN_UNIT[3]));
        let p45 = vfmaq_f32(vdupq_n_f32(ATAN_UNIT[4]), s, vdupq_n_f32(ATAN_UNIT[5]));
        let p03 = vfmaq_f32(p01, s2, p23);
        let p46 = vfmaq_f32(p45, s2, vdupq_n_f32(ATAN_UNIT[6]));
        let mut angle = vmulq_f32(vfmaq_f32(p03, s4, p46), ratio);
        let quarter = vsubq_f32(vdupq_n_f32(std::f32::consts::FRAC_PI_2), angle);
        angle = vbslq_f32(steep, quarter, angle);
        let behind = vcltq_f32(x, vdupq_n_f32(0.0));
        let half = vsubq_f32(vdupq_n_f32(std::f32::consts::PI), angle);
        angle = vbslq_f32(behind, half, angle);
        let origin = vceqq_f32(vaddq_f32(ax, ay), vdupq_n_f32(0.0));
        angle = vbslq_f32(origin, vdupq_n_f32(0.0), angle);
        let sign_mask = vdupq_n_u32(0x8000_0000);
        vreinterpretq_f32_u32(vorrq_u32(
            vbicq_u32(vreinterpretq_u32_f32(angle), sign_mask),
            vandq_u32(vreinterpretq_u32_f32(y), sign_mask),
        ))
    }
}

/// Writes `arg(iq[k + 1] * conj(iq[k])) * scale` to `out[k]` for as many whole groups of eight
/// as fit, shifting each loaded quad into the next step with `vextq_f32`.
pub(super) unsafe fn phase_diffs(
    iq: &[Complex<f32>],
    scale: f32,
    out: *mut f32,
    len: usize,
) -> usize {
    let available = len.min(iq.len().saturating_sub(1));
    if available < 4 {
        return 0;
    }
    let count = available & !3;
    unsafe {
        let sp = iq.as_ptr().cast::<f32>();
        let v_scale = vdupq_n_f32(scale);
        let mut prev = vld2q_f32(sp);
        let mut start = 0;
        while start + 8 <= count {
            let next0 = vld2q_f32(sp.add((start + 4) * 2));
            let next1 = vld2q_f32(sp.add((start + 8) * 2));
            let now0_re = vextq_f32::<1>(prev.0, next0.0);
            let now0_im = vextq_f32::<1>(prev.1, next0.1);
            let now1_re = vextq_f32::<1>(next0.0, next1.0);
            let now1_im = vextq_f32::<1>(next0.1, next1.1);

            let re0 = vaddq_f32(vmulq_f32(now0_re, prev.0), vmulq_f32(now0_im, prev.1));
            let im0 = vsubq_f32(vmulq_f32(now0_im, prev.0), vmulq_f32(now0_re, prev.1));
            let re1 = vaddq_f32(vmulq_f32(now1_re, next0.0), vmulq_f32(now1_im, next0.1));
            let im1 = vsubq_f32(vmulq_f32(now1_im, next0.0), vmulq_f32(now1_re, next0.1));

            vst1q_f32(out.add(start), vmulq_f32(atan2(im0, re0), v_scale));
            vst1q_f32(out.add(start + 4), vmulq_f32(atan2(im1, re1), v_scale));
            prev = next1;
            start += 8;
        }
        while start + 4 <= count {
            let now = vld2q_f32(sp.add((start + 1) * 2));
            let re = vaddq_f32(vmulq_f32(now.0, prev.0), vmulq_f32(now.1, prev.1));
            let im = vsubq_f32(vmulq_f32(now.1, prev.0), vmulq_f32(now.0, prev.1));
            vst1q_f32(out.add(start), vmulq_f32(atan2(im, re), v_scale));
            start += 4;
            if start < count {
                prev = vld2q_f32(sp.add(start * 2));
            }
        }
    }
    count
}
