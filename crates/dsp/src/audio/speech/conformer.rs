use super::{
    attention::Attention,
    matmul::{HalfMatrix, matmul},
    tensor::{Norm, add_scaled, silu},
};
use crate::math::vector::Vector;

pub(super) struct FeedForward {
    pub up: HalfMatrix,
    pub down: HalfMatrix,
}

/// The convolution module: pointwise to a gated pair, depthwise in time, pointwise back.
pub(super) struct ConvModule {
    pub expand: HalfMatrix,
    pub depthwise: Vec<f32>,
    pub depthwise_bias: Vec<f32>,
    pub kernel: usize,
    pub project: HalfMatrix,
}

pub(super) struct Block {
    pub feed_forward1: FeedForward,
    pub attention: Attention,
    pub conv: ConvModule,
    pub feed_forward2: FeedForward,
    pub norm_feed_forward1: Norm,
    pub norm_attention: Norm,
    pub norm_conv: Norm,
    pub norm_feed_forward2: Norm,
    pub norm_out: Norm,
}

/// What every layer of one pass shares.
pub(super) struct Pass<'a> {
    pub bytes: &'a [u8],
    pub vector: Vector,
    pub positions: &'a [f32],
    pub time: usize,
    pub threads: usize,
}

impl Block {
    pub fn run(&self, pass: &Pass<'_>, hidden: &mut [f32]) {
        let mut normed = vec![0.0; hidden.len()];
        self.norm_feed_forward1.apply(hidden, &mut normed);
        add_scaled(hidden, 0.5, &self.feed_forward1.run(pass, &normed));
        self.norm_attention.apply(hidden, &mut normed);
        let attended = self.attention.run(
            pass.bytes,
            pass.vector,
            &normed,
            pass.positions,
            pass.time,
            pass.threads,
        );
        add_scaled(hidden, 1.0, &attended);
        self.norm_conv.apply(hidden, &mut normed);
        add_scaled(hidden, 1.0, &self.conv.run(pass, &normed));
        self.norm_feed_forward2.apply(hidden, &mut normed);
        add_scaled(hidden, 0.5, &self.feed_forward2.run(pass, &normed));
        normed.copy_from_slice(hidden);
        self.norm_out.apply(&normed, hidden);
    }
}

impl FeedForward {
    fn run(&self, pass: &Pass<'_>, input: &[f32]) -> Vec<f32> {
        let mut inner = vec![0.0; pass.time * self.up.cols];
        matmul(
            pass.bytes,
            &self.up,
            input,
            pass.time,
            &mut inner,
            pass.threads,
        );
        silu(pass.vector, &mut inner, &mut Vec::new());
        let mut out = vec![0.0; pass.time * self.down.cols];
        matmul(
            pass.bytes,
            &self.down,
            &inner,
            pass.time,
            &mut out,
            pass.threads,
        );
        out
    }
}

impl ConvModule {
    fn run(&self, pass: &Pass<'_>, input: &[f32]) -> Vec<f32> {
        let width = self.project.depth;
        let mut pair = vec![0.0; pass.time * 2 * width];
        matmul(
            pass.bytes,
            &self.expand,
            input,
            pass.time,
            &mut pair,
            pass.threads,
        );
        let mut gated = vec![0.0; pass.time * width];
        let mut gates = Vec::with_capacity(width);
        for (out, both) in gated
            .chunks_exact_mut(width)
            .zip(pair.chunks_exact(2 * width))
        {
            gates.clear();
            gates.extend_from_slice(&both[width..]);
            pass.vector.sigmoid(&mut gates);
            for ((value, &x), &gate) in out.iter_mut().zip(&both[..width]).zip(&gates) {
                *value = x * gate;
            }
        }
        let mut mixed = self.depthwise_time(&gated, pass.time, width);
        silu(pass.vector, &mut mixed, &mut Vec::new());
        let mut out = vec![0.0; pass.time * width];
        matmul(
            pass.bytes,
            &self.project,
            &mixed,
            pass.time,
            &mut out,
            pass.threads,
        );
        out
    }

    fn depthwise_time(&self, input: &[f32], time: usize, width: usize) -> Vec<f32> {
        let half = self.kernel / 2;
        let mut out = vec![0.0; time * width];
        for (t, row) in out.chunks_exact_mut(width).enumerate() {
            row.copy_from_slice(&self.depthwise_bias);
            for k in 0..self.kernel {
                let Some(source) = (t + k).checked_sub(half).filter(|&s| s < time) else {
                    continue;
                };
                let line = &input[source * width..(source + 1) * width];
                for (c, (value, &x)) in row.iter_mut().zip(line).enumerate() {
                    *value += self.depthwise[c * self.kernel + k] * x;
                }
            }
        }
        out
    }
}
