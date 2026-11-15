use num_complex::Complex;

use super::gray;
use crate::constellation::{Constellation, squared_distance};

const MAX_PASSES: usize = 64;

const MIN_IMPROVEMENT: f64 = 1e-9;

/// Grid that point coordinates and angles are rounded to before sorting, so points on one row
/// or ring compare equal.
const SORT_QUANTUM: f64 = 1e4;

/// A labelling found by pairwise swaps from the best of four Gray seeds, weighting each label
/// pair by how likely noise is to confuse its points.
#[must_use]
pub fn label_by_descent(points: &[Complex<f32>]) -> Vec<u32> {
    let weights = pair_weights(points);
    let mut best: Option<(f64, Vec<u32>)> = None;
    for order in seed_orders(points) {
        let labels = descend(seed_labels(&order), &weights, points.len());
        let cost = total_cost(&labels, &weights);
        if best.as_ref().is_none_or(|(c, _)| cost < *c) {
            best = Some((cost, labels));
        }
    }
    best.map_or_else(|| (0..points.len() as u32).collect(), |(_, labels)| labels)
}

#[must_use]
pub fn gray_penalty(c: &Constellation) -> f64 {
    let weights = pair_weights(c.points());
    let total: f64 = weights.iter().sum::<f64>() / 2.0;
    total_cost(c.labels(), &weights) / total
}

pub(super) fn pair_weights(points: &[Complex<f32>]) -> Vec<f64> {
    let n = points.len();
    let d2 = |a: usize, b: usize| squared_distance(points[a], points[b]);
    let mut min = f64::INFINITY;
    for i in 0..n {
        for j in (i + 1)..n {
            min = min.min(d2(i, j));
        }
    }
    let mut w = vec![0.0f64; n * n];
    for i in 0..n {
        for j in (i + 1)..n {
            let v = (-d2(i, j) / min).exp();
            w[i * n + j] = v;
            w[j * n + i] = v;
        }
    }
    w
}

pub(super) fn seed_orders(points: &[Complex<f32>]) -> Vec<Vec<usize>> {
    let natural: Vec<usize> = (0..points.len()).collect();
    let mut angular = natural.clone();
    angular.sort_by_key(|&i| {
        let p = points[i];
        (
            quantise(f64::from(p.im).atan2(f64::from(p.re))),
            quantise(f64::from(p.norm())),
        )
    });
    let rows = snake(points, &natural, true);
    let columns = snake(points, &natural, false);
    vec![natural, rows, columns, angular]
}

/// Points sorted row by row, every other row reversed, so neighbours in the order are
/// neighbours on the plane.
fn snake(points: &[Complex<f32>], natural: &[usize], horizontal: bool) -> Vec<usize> {
    let n = points.len();
    let key = |i: usize| -> (i64, i64) {
        let (re, im) = (
            quantise(f64::from(points[i].re)),
            quantise(f64::from(points[i].im)),
        );
        if horizontal { (im, re) } else { (re, im) }
    };
    let mut order = natural.to_vec();
    order.sort_by_key(|&i| key(i));
    let mut out: Vec<usize> = Vec::with_capacity(n);
    let (mut row_start, mut rows) = (0usize, 0usize);
    while row_start < n {
        let major = key(order[row_start]).0;
        let mut row_end = row_start;
        while row_end < n && key(order[row_end]).0 == major {
            row_end += 1;
        }
        if rows % 2 == 1 {
            out.extend(order[row_start..row_end].iter().rev());
        } else {
            out.extend(&order[row_start..row_end]);
        }
        row_start = row_end;
        rows += 1;
    }
    out
}

fn quantise(x: f64) -> i64 {
    (x * SORT_QUANTUM).round() as i64
}

pub(super) fn seed_labels(order: &[usize]) -> Vec<u32> {
    let mut labels = vec![0u32; order.len()];
    for (i, &pos) in order.iter().enumerate() {
        labels[pos] = gray(i as u32);
    }
    labels
}

fn descend(mut labels: Vec<u32>, weights: &[f64], n: usize) -> Vec<u32> {
    for _ in 0..MAX_PASSES {
        let mut improved = false;
        for i in 0..n {
            for j in (i + 1)..n {
                let before =
                    point_cost(&labels, weights, n, i) + point_cost(&labels, weights, n, j);
                labels.swap(i, j);
                let after = point_cost(&labels, weights, n, i) + point_cost(&labels, weights, n, j);
                if after + MIN_IMPROVEMENT < before {
                    improved = true;
                } else {
                    labels.swap(i, j);
                }
            }
        }
        if !improved {
            break;
        }
    }
    labels
}

fn point_cost(labels: &[u32], weights: &[f64], n: usize, i: usize) -> f64 {
    (0..n)
        .map(|j| f64::from((labels[i] ^ labels[j]).count_ones()) * weights[i * n + j])
        .sum()
}

pub(super) fn total_cost(labels: &[u32], weights: &[f64]) -> f64 {
    let n = labels.len();
    (0..n)
        .map(|i| point_cost(labels, weights, n, i))
        .sum::<f64>()
        / 2.0
}
