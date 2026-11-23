use super::{
    matmul::{HalfMatrix, matmul},
    tensor::softmax,
};
use crate::math::vector::{GemmShape, Vector};

/// Multi-head self-attention with Transformer-XL relative positions, as the `FastConformer` uses.
pub(super) struct Attention {
    pub query: HalfMatrix,
    pub key: HalfMatrix,
    pub value: HalfMatrix,
    pub output: HalfMatrix,
    pub position: HalfMatrix,
    pub bias_u: Vec<f32>,
    pub bias_v: Vec<f32>,
    pub heads: usize,
}

struct Projected<'a> {
    query: &'a [f32],
    key: &'a [f32],
    value: &'a [f32],
    position: &'a [f32],
    time: usize,
    width: usize,
}

impl Attention {
    /// Attends `time` normalised frames to each other, `positions` holding the `2 * time - 1`
    /// relative position encodings from `time - 1` down to `1 - time`.
    pub fn run(
        &self,
        bytes: &[u8],
        vector: Vector,
        input: &[f32],
        positions: &[f32],
        time: usize,
        threads: usize,
    ) -> Vec<f32> {
        let width = self.query.cols;
        let project = |matrix: &HalfMatrix, from: &[f32], rows: usize| {
            let mut out = vec![0.0; rows * width];
            matmul(bytes, matrix, from, rows, &mut out, threads);
            out
        };
        let query = project(&self.query, input, time);
        let key = project(&self.key, input, time);
        let value = project(&self.value, input, time);
        let position = project(&self.position, positions, 2 * time - 1);
        let projected = Projected {
            query: &query,
            key: &key,
            value: &value,
            position: &position,
            time,
            width,
        };
        let context = self.heads_context(vector, &projected, threads);
        project(&self.output, &context, time)
    }

    fn heads_context(&self, vector: Vector, projected: &Projected<'_>, threads: usize) -> Vec<f32> {
        let (time, width) = (projected.time, projected.width);
        let head_dim = width / self.heads;
        let per_worker = self.heads.div_ceil(threads.clamp(1, self.heads));
        let mut context = vec![0.0; time * width];
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..self.heads)
                .step_by(per_worker)
                .map(|first| {
                    scope.spawn(move || {
                        (first..(first + per_worker).min(self.heads))
                            .map(|head| (head, self.head(vector, projected, head)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for worker in workers {
                let done = match worker.join() {
                    Ok(done) => done,
                    Err(panic) => std::panic::resume_unwind(panic),
                };
                for (head, out) in done {
                    for (t, row) in out.chunks_exact(head_dim).enumerate() {
                        context[t * width + head * head_dim..][..head_dim].copy_from_slice(row);
                    }
                }
            }
        });
        context
    }

    fn head(&self, vector: Vector, p: &Projected<'_>, head: usize) -> Vec<f32> {
        let (time, width) = (p.time, p.width);
        let dim = width / self.heads;
        let span = head * dim..(head + 1) * dim;
        let positions = 2 * time - 1;
        let column = |from: &[f32], rows: usize, bias: Option<&[f32]>| {
            let mut out = vec![0.0; rows * dim];
            for (r, row) in out.chunks_exact_mut(dim).enumerate() {
                row.copy_from_slice(&from[r * width + span.start..r * width + span.end]);
                if let Some(bias) = bias {
                    for (value, &b) in row.iter_mut().zip(&bias[span.clone()]) {
                        *value += b;
                    }
                }
            }
            out
        };
        let query_u = column(p.query, time, Some(&self.bias_u));
        let query_v = column(p.query, time, Some(&self.bias_v));
        let key_t = transpose(&column(p.key, time, None), time, dim);
        let position_t = transpose(&column(p.position, positions, None), positions, dim);
        let value = column(p.value, time, None);
        let mut content = vec![0.0; time * time];
        vector.gemm(
            GemmShape {
                rows: time,
                depth: dim,
                cols: time,
            },
            &query_u,
            &key_t,
            &mut content,
        );
        let mut relative = vec![0.0; time * positions];
        vector.gemm(
            GemmShape {
                rows: time,
                depth: dim,
                cols: positions,
            },
            &query_v,
            &position_t,
            &mut relative,
        );
        let scale = (dim as f32).sqrt().recip();
        for (i, row) in content.chunks_exact_mut(time).enumerate() {
            let shifted = &relative[i * positions + time - 1 - i..][..time];
            for (score, &rel) in row.iter_mut().zip(shifted) {
                *score = (*score + rel) * scale;
            }
            softmax(row);
        }
        let mut out = vec![0.0; time * dim];
        vector.gemm(
            GemmShape {
                rows: time,
                depth: time,
                cols: dim,
            },
            &content,
            &value,
            &mut out,
        );
        out
    }
}

fn transpose(input: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    let mut out = vec![0.0; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            out[c * rows + r] = input[r * cols + c];
        }
    }
    out
}

/// Sinusoidal encodings for relative positions `time - 1` down to `1 - time`, sine and cosine
/// interleaved per frequency.
pub(super) fn relative_positions(time: usize, width: usize) -> Vec<f32> {
    let rows = 2 * time - 1;
    let mut out = vec![0.0; rows * width];
    for (p, row) in out.chunks_exact_mut(width).enumerate() {
        let position = (time as f64 - 1.0) - p as f64;
        for (i, pair) in row.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let inv_freq = 1.0 / 10000f64.powf(2.0 * i as f64 / width as f64);
            let angle = f64::from(position as f32 * inv_freq as f32);
            pair[0] = angle.sin() as f32;
            pair[1] = angle.cos() as f32;
        }
    }
    out
}
