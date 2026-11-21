use std::ops::Range;

use super::range::fill_prefix;
use super::{CfarSpec, MAX_GUARD, MAX_PAD, MAX_TRAIN_DOPPLER, MAX_TRAIN_RANGE};
use crate::radar::RadarDspError;
use crate::radar::strongest::Strongest;
use crate::radar::threshold::{CfarStatistic, os_order};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hit {
    pub row: u32,
    pub gate: u32,
    pub power: f32,
    pub noise: f32,
}

/// Cell-averaging, greatest-of or order-statistic CFAR over a range-Doppler map, along range
/// only or across a range-Doppler plane.
pub struct Cfar {
    pub(super) spec: CfarSpec,
    pub(super) gates: usize,
    pub(super) rows: usize,
    pub(super) prefix: Vec<f64>,
    pub(super) table: Vec<f64>,
    pub(super) scratch: Vec<f32>,
}

impl Cfar {
    pub fn new(spec: CfarSpec, gates: usize, rows: usize) -> Result<Self, RadarDspError> {
        if !spec.valid() || gates == 0 || rows == 0 {
            return Err(RadarDspError::Setting);
        }
        let widest = CfarSpec {
            plane: true,
            guard_range: MAX_GUARD,
            train_range: MAX_TRAIN_RANGE,
            guard_doppler: MAX_GUARD,
            train_doppler: MAX_TRAIN_DOPPLER,
            ..spec
        };
        Ok(Self {
            spec,
            gates,
            rows,
            prefix: vec![0.0; gates + 1],
            table: vec![0.0; (rows + 2 * MAX_PAD + 1) * (gates + 1)],
            scratch: Vec::with_capacity(widest.cells()),
        })
    }

    pub fn set_spec(&mut self, spec: CfarSpec) -> Result<(), RadarDspError> {
        if !spec.valid() {
            return Err(RadarDspError::Setting);
        }
        self.spec = spec;
        Ok(())
    }

    #[must_use]
    pub const fn spec(&self) -> CfarSpec {
        self.spec
    }

    pub fn detect(
        &mut self,
        power: &[f32],
        report_rows: Range<usize>,
        out: &mut Vec<Hit>,
        capacity: usize,
    ) -> Result<usize, RadarDspError> {
        out.clear();
        if power.len() < self.rows * self.gates {
            return Err(RadarDspError::Shape);
        }
        out.reserve(capacity);
        let mut keep = Strongest::new(capacity);
        let plane_os = self.spec.plane && matches!(self.spec.stat, CfarStatistic::Os { .. });
        if self.spec.plane {
            self.build_table(power);
        }
        let rows = report_rows.start.min(self.rows)..report_rows.end.min(self.rows);
        for row in rows {
            if self.spec.is_clutter(row, self.rows) {
                continue;
            }
            if plane_os {
                self.detect_plane_os_row(power, row, out, &mut keep);
                continue;
            }
            let line = &power[row * self.gates..(row + 1) * self.gates];
            if !self.spec.plane {
                fill_prefix(&mut self.prefix, line);
                self.detect_range_row(line, row, out, &mut keep);
                continue;
            }
            for (gate, &cell) in line.iter().enumerate().skip(self.spec.min_gate) {
                let (noise, factor) = self.plane_noise(power, row, gate);
                if is_hit(cell, noise, factor, self.spec.min_snr) {
                    let hit = Hit {
                        row: row as u32,
                        gate: gate as u32,
                        power: cell,
                        noise,
                    };
                    keep.offer(out, hit, |hit| hit.power);
                }
            }
        }
        out.sort_unstable_by_key(|hit| (hit.row, hit.gate));
        Ok(keep.dropped())
    }
}

#[inline]
pub(super) fn is_hit(cell: f32, noise: f32, factor: f32, min_snr: f32) -> bool {
    noise > 0.0 && cell > factor * noise && cell >= min_snr * noise
}

#[inline]
pub(super) fn os_probe(cell: f32, alpha: f32, min_snr: f32) -> f32 {
    let divisor = alpha.max(min_snr);
    (cell / divisor).next_up().next_up()
}

pub(super) fn count_gt(values: &[f32], threshold: f32) -> usize {
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{
            vaddvq_u32, vcgtq_f32, vdupq_n_f32, vdupq_n_u32, vld1q_f32, vsubq_u32,
        };
        let (quads, tail) = values.as_chunks::<4>();
        let vec_count = unsafe {
            let limit = vdupq_n_f32(threshold);
            let mut acc = vdupq_n_u32(0);
            for quad in quads {
                let mask = vcgtq_f32(vld1q_f32(quad.as_ptr()), limit);
                acc = vsubq_u32(acc, mask);
            }
            vaddvq_u32(acc) as usize
        };
        vec_count + tail.iter().filter(|&&v| v > threshold).count()
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            unsafe { count_gt_avx2(values, threshold) }
        } else {
            count_gt_sse2(values, threshold)
        }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        values.iter().filter(|&&v| v > threshold).count()
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn count_gt_avx2(values: &[f32], threshold: f32) -> usize {
    use std::arch::x86_64::{
        _CMP_GT_OQ, _mm_add_epi32, _mm_storeu_si128, _mm256_castps_si256, _mm256_castsi256_si128,
        _mm256_cmp_ps, _mm256_extracti128_si256, _mm256_loadu_ps, _mm256_set1_ps,
        _mm256_setzero_si256, _mm256_sub_epi32,
    };
    let (eights, tail) = values.as_chunks::<8>();
    let limit = _mm256_set1_ps(threshold);
    let mut acc = _mm256_setzero_si256();
    for eight in eights {
        let v = unsafe { _mm256_loadu_ps(eight.as_ptr()) };
        let mask = _mm256_castps_si256(_mm256_cmp_ps::<_CMP_GT_OQ>(v, limit));
        acc = _mm256_sub_epi32(acc, mask);
    }
    let hi = _mm256_extracti128_si256::<1>(acc);
    let lo = _mm256_castsi256_si128(acc);
    let sum = _mm_add_epi32(lo, hi);
    let mut lanes = [0u32; 4];
    unsafe { _mm_storeu_si128(lanes.as_mut_ptr().cast(), sum) };
    let vec_count = (lanes[0] + lanes[1] + lanes[2] + lanes[3]) as usize;
    vec_count + tail.iter().filter(|&&v| v > threshold).count()
}

#[cfg(target_arch = "x86_64")]
fn count_gt_sse2(values: &[f32], threshold: f32) -> usize {
    use std::arch::x86_64::{
        _mm_castps_si128, _mm_cmpgt_ps, _mm_loadu_ps, _mm_set1_ps, _mm_setzero_si128,
        _mm_storeu_si128, _mm_sub_epi32,
    };
    let (quads, tail) = values.as_chunks::<4>();
    let vec_count = unsafe {
        let limit = _mm_set1_ps(threshold);
        let mut acc = _mm_setzero_si128();
        for quad in quads {
            let mask = _mm_castps_si128(_mm_cmpgt_ps(_mm_loadu_ps(quad.as_ptr()), limit));
            acc = _mm_sub_epi32(acc, mask);
        }
        let mut lanes = [0u32; 4];
        _mm_storeu_si128(lanes.as_mut_ptr().cast(), acc);
        (lanes[0] + lanes[1] + lanes[2] + lanes[3]) as usize
    };
    vec_count + tail.iter().filter(|&&v| v > threshold).count()
}

pub(super) fn order_statistic(cells: &mut [f32], rank: f32) -> f32 {
    if cells.is_empty() {
        return 0.0;
    }
    let order = os_order(rank, cells.len()) - 1;
    let (_, value, _) = cells.select_nth_unstable_by(order, f32::total_cmp);
    *value
}
