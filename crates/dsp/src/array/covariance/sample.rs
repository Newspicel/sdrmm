use num_complex::Complex;

use super::{
    CovarianceError,
    hermitian::{set_mirrored, write_identity},
};
#[cfg(target_arch = "aarch64")]
use crate::math::complex::widen;
#[cfg(not(target_arch = "aarch64"))]
use crate::math::complex::{cross_sum, sum_norm_sqr};
use crate::math::linalg::{CMat, LinalgError, MAX_ORDER};

pub struct SampleCovariance {
    order: usize,
    sum: [Complex<f64>; MAX_ORDER * MAX_ORDER],
    weight: f64,
    weight_sq: f64,
}

impl SampleCovariance {
    pub fn new(order: usize) -> Result<Self, CovarianceError> {
        if order == 0 || order > MAX_ORDER {
            return Err(LinalgError::Order(order).into());
        }
        Ok(Self {
            order,
            sum: [Complex::new(0.0, 0.0); MAX_ORDER * MAX_ORDER],
            weight: 0.0,
            weight_sq: 0.0,
        })
    }

    #[must_use]
    pub const fn order(&self) -> usize {
        self.order
    }

    pub fn reset(&mut self) {
        self.sum.fill(Complex::new(0.0, 0.0));
        self.weight = 0.0;
        self.weight_sq = 0.0;
    }

    pub fn decay(&mut self, factor: f32) {
        let factor = f64::from(factor);
        for value in &mut self.sum {
            *value *= factor;
        }
        self.weight *= factor;
        self.weight_sq *= factor * factor;
    }

    pub fn accumulate(&mut self, lanes: &[&[Complex<f32>]]) -> usize {
        const TILE: usize = 1024;
        if lanes.len() != self.order {
            return 0;
        }
        let len = lanes.iter().map(|lane| lane.len()).min().unwrap_or(0);
        let n = self.order;
        let mut offset = 0;
        while offset < len {
            let end = (offset + TILE).min(len);
            #[cfg(target_arch = "aarch64")]
            accumulate_tile_neon(&mut self.sum, n, lanes, offset, end);
            #[cfg(not(target_arch = "aarch64"))]
            {
                for (i, left) in lanes.iter().enumerate() {
                    let l = &left[offset..end];
                    let diag = sum_norm_sqr(l);
                    self.sum[i * n + i].re += f64::from(diag);
                    for (j, right) in lanes.iter().enumerate().skip(i + 1) {
                        self.sum[i * n + j] += cross_sum(l, &right[offset..end]);
                    }
                }
            }
            offset = end;
        }
        self.weight += len as f64;
        self.weight_sq += len as f64;
        len
    }

    #[must_use]
    pub const fn weight(&self) -> f64 {
        self.weight
    }

    #[must_use]
    pub fn effective_snapshots(&self) -> f64 {
        if self.weight_sq > 0.0 {
            self.weight * self.weight / self.weight_sq
        } else {
            0.0
        }
    }

    pub fn matrix(&self, out: &mut CMat) -> bool {
        let n = self.order;
        if out.resize(n).is_err() {
            return false;
        }
        if self.weight <= 0.0 {
            write_identity(out);
            return false;
        }
        let scale = 1.0 / self.weight;
        for i in 0..n {
            for j in i..n {
                set_mirrored(out, i, j, self.sum[i * n + j] * scale);
            }
        }
        true
    }
}

#[cfg(target_arch = "aarch64")]
fn accumulate_tile_neon(
    sum: &mut [Complex<f64>],
    n: usize,
    lanes: &[&[Complex<f32>]],
    offset: usize,
    end: usize,
) {
    let blocks = (end - offset) / 4;
    let tail_start = offset + blocks * 4;
    for i in 0..n {
        let row = &mut sum[i * n..(i + 1) * n];
        let diag = add_partners(row, lanes, i, i + 1, offset, blocks);
        row[i].re += f64::from(diag);
        for j in (i + 5..n).step_by(4) {
            add_partners(row, lanes, i, j, offset, blocks);
        }
        for t in tail_start..end {
            let a = widen(lanes[i][t]);
            row[i].re += a.norm_sqr();
            for j in i + 1..n {
                row[j] += a * widen(lanes[j][t]).conj();
            }
        }
    }
}

/// Adds lane `i` against up to four lanes from `first` into `row`, and returns lane `i`'s power.
#[cfg(target_arch = "aarch64")]
fn add_partners(
    row: &mut [Complex<f64>],
    lanes: &[&[Complex<f32>]],
    i: usize,
    first: usize,
    offset: usize,
    blocks: usize,
) -> f32 {
    match lanes.len() - first {
        0 => add_partner_group::<0>(row, lanes, i, first, offset, blocks),
        1 => add_partner_group::<1>(row, lanes, i, first, offset, blocks),
        2 => add_partner_group::<2>(row, lanes, i, first, offset, blocks),
        3 => add_partner_group::<3>(row, lanes, i, first, offset, blocks),
        _ => add_partner_group::<4>(row, lanes, i, first, offset, blocks),
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn add_partner_group<const K: usize>(
    row: &mut [Complex<f64>],
    lanes: &[&[Complex<f32>]],
    i: usize,
    first: usize,
    offset: usize,
    blocks: usize,
) -> f32 {
    use std::arch::aarch64::{vaddvq_f32, vdupq_n_f32, vfmaq_f32, vfmsq_f32, vld2q_f32};
    let left = lanes[i][offset..offset + 4 * blocks].as_ptr().cast::<f32>();
    let rights: [*const f32; K] = std::array::from_fn(|k| {
        lanes[first + k][offset..offset + 4 * blocks]
            .as_ptr()
            .cast()
    });
    unsafe {
        let mut diag = vdupq_n_f32(0.0);
        let mut re = [vdupq_n_f32(0.0); K];
        let mut im = [vdupq_n_f32(0.0); K];
        for block in 0..blocks {
            let at = 8 * block;
            let a = vld2q_f32(left.add(at));
            diag = vfmaq_f32(vfmaq_f32(diag, a.0, a.0), a.1, a.1);
            for k in 0..K {
                let b = vld2q_f32(rights[k].add(at));
                re[k] = vfmaq_f32(vfmaq_f32(re[k], a.0, b.0), a.1, b.1);
                im[k] = vfmsq_f32(vfmaq_f32(im[k], a.1, b.0), a.0, b.1);
            }
        }
        for k in 0..K {
            row[first + k] +=
                Complex::new(f64::from(vaddvq_f32(re[k])), f64::from(vaddvq_f32(im[k])));
        }
        vaddvq_f32(diag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::covariance::testing::random_lanes;
    use crate::math::complex::widen;

    #[test]
    fn upper_triangle_accumulation_matches_the_outer_product() {
        for order in [2, 5, 9, 16] {
            let lanes = random_lanes(order, 1003, 11);
            let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
            let mut covariance = SampleCovariance::new(order).unwrap();
            assert_eq!(covariance.accumulate(&views[..order - 1]), 0);
            assert_eq!(covariance.accumulate(&views), 1003);
            let mut matrix = CMat::zeros(1).unwrap();
            assert!(covariance.matrix(&mut matrix));
            for i in 0..order {
                for j in 0..order {
                    let want: Complex<f64> = lanes[i]
                        .iter()
                        .zip(&lanes[j])
                        .map(|(a, b)| widen(a * b.conj()))
                        .sum::<Complex<f64>>()
                        / 1003.0;
                    let got = widen(matrix.get(i, j));
                    assert!(
                        (got - want).norm() <= 1e-4 * want.norm().max(1e-3),
                        "order {order}: {i},{j}"
                    );
                }
            }
        }
    }

    #[test]
    fn effective_snapshots_follow_decay() {
        let lanes = random_lanes(2, 1000, 3);
        let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
        let mut covariance = SampleCovariance::new(2).unwrap();
        covariance.accumulate(&views);
        assert!((covariance.effective_snapshots() - 1000.0).abs() < 1e-9);
        covariance.decay(0.5);
        covariance.accumulate(&views);
        assert!((covariance.weight() - 1500.0).abs() < 1e-9);
        assert!((covariance.effective_snapshots() - 1800.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_average_reports_the_identity() {
        let mut covariance = SampleCovariance::new(3).unwrap();
        let mut matrix = CMat::zeros(1).unwrap();
        assert!(!covariance.matrix(&mut matrix));
        assert_eq!(matrix, CMat::identity(3).unwrap());
        assert_eq!(covariance.effective_snapshots(), 0.0);
        let lanes = random_lanes(3, 16, 5);
        let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
        covariance.accumulate(&views);
        covariance.reset();
        assert!(!covariance.matrix(&mut matrix));
        assert!(SampleCovariance::new(0).is_err());
        assert!(SampleCovariance::new(17).is_err());
    }
}
