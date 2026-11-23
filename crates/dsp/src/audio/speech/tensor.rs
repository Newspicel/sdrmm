use crate::math::vector::Vector;

const LAYER_NORM_EPS: f32 = 1e-5;

/// A layer norm's gain and bias over rows of `width` values.
pub(super) struct Norm {
    pub weight: Vec<f32>,
    pub bias: Vec<f32>,
}

impl Norm {
    pub fn apply(&self, input: &[f32], out: &mut [f32]) {
        let width = self.weight.len();
        for (row, dst) in input.chunks_exact(width).zip(out.chunks_exact_mut(width)) {
            let mean = row.iter().sum::<f32>() / width as f32;
            let var = row.iter().map(|&x| (x - mean) * (x - mean)).sum::<f32>() / width as f32;
            let scale = (var + LAYER_NORM_EPS).sqrt().recip();
            for (((o, &x), &w), &b) in dst.iter_mut().zip(row).zip(&self.weight).zip(&self.bias) {
                *o = (x - mean) * scale * w + b;
            }
        }
    }
}

pub(super) fn silu(vector: Vector, values: &mut [f32], scratch: &mut Vec<f32>) {
    scratch.clear();
    scratch.extend_from_slice(values);
    vector.sigmoid(scratch);
    for (value, gate) in values.iter_mut().zip(scratch.iter()) {
        *value *= gate;
    }
}

pub(super) fn add_rows(values: &mut [f32], bias: &[f32]) {
    for row in values.chunks_exact_mut(bias.len()) {
        for (value, &b) in row.iter_mut().zip(bias) {
            *value += b;
        }
    }
}

pub(super) fn add_scaled(into: &mut [f32], scale: f32, from: &[f32]) {
    for (dst, &x) in into.iter_mut().zip(from) {
        *dst += scale * x;
    }
}

pub(super) fn softmax(row: &mut [f32]) {
    let peak = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0;
    for value in row.iter_mut() {
        *value = (*value - peak).exp();
        sum += *value;
    }
    let scale = sum.recip();
    for value in row.iter_mut() {
        *value *= scale;
    }
}

pub(super) fn argmax(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .fold((0, f32::NEG_INFINITY), |best, (index, &value)| {
            if value > best.1 { (index, value) } else { best }
        })
        .0
}
