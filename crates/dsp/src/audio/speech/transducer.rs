use super::{matmul::HalfMatrix, tensor::argmax};
use crate::math::vector::{GemmShape, Vector};

/// Tokens one frame may emit before decoding moves on regardless, so a stuck joint cannot loop.
const MAX_SYMBOLS_PER_FRAME: usize = 10;

/// One LSTM layer: `[input; hidden] x weight`, gates in input, forget, cell, output order.
pub(super) struct Lstm {
    pub weight: Vec<f32>,
    pub bias: Vec<f32>,
}

/// The prediction network and joint of a token-and-duration transducer, decoded greedily.
pub(super) struct Transducer {
    pub embedding: HalfMatrix,
    pub layers: Vec<Lstm>,
    pub hidden: usize,
    pub projector: Vec<f32>,
    pub projector_bias: Vec<f32>,
    pub joint: Vec<f32>,
    pub joint_bias: Vec<f32>,
    pub blank: usize,
    pub durations: Vec<usize>,
}

struct State {
    hidden: Vec<Vec<f32>>,
    cell: Vec<Vec<f32>>,
    out: Vec<f32>,
}

impl Transducer {
    fn outputs(&self) -> usize {
        self.blank + 1 + self.durations.len()
    }

    /// The tokens for `time` projected encoder frames of `hidden` values each.
    pub fn decode(&self, bytes: &[u8], vector: Vector, encoded: &[f32], time: usize) -> Vec<usize> {
        let hidden = self.hidden;
        let mut state = State {
            hidden: vec![vec![0.0; hidden]; self.layers.len()],
            cell: vec![vec![0.0; hidden]; self.layers.len()],
            out: vec![0.0; hidden],
        };
        self.step(bytes, vector, self.blank, &mut state);
        let mut tokens = Vec::new();
        let (mut frame, mut at_frame) = (0, 0);
        let mut joint_in = vec![0.0; hidden];
        let mut logits = vec![0.0; self.outputs()];
        while frame < time {
            for ((value, &e), &d) in joint_in
                .iter_mut()
                .zip(&encoded[frame * hidden..(frame + 1) * hidden])
                .zip(&state.out)
            {
                *value = (e + d).max(0.0);
            }
            vector.gemm(
                GemmShape {
                    rows: 1,
                    depth: hidden,
                    cols: self.outputs(),
                },
                &joint_in,
                &self.joint,
                &mut logits,
            );
            for (value, &b) in logits.iter_mut().zip(&self.joint_bias) {
                *value += b;
            }
            let token = argmax(&logits[..=self.blank]);
            let mut skip = self.durations[argmax(&logits[self.blank + 1..])];
            if token == self.blank {
                skip = skip.max(1);
            } else {
                tokens.push(token);
                self.step(bytes, vector, token, &mut state);
                at_frame += 1;
                if skip == 0 && at_frame >= MAX_SYMBOLS_PER_FRAME {
                    skip = 1;
                }
            }
            if skip > 0 {
                at_frame = 0;
            }
            frame += skip;
        }
        tokens
    }

    fn step(&self, bytes: &[u8], vector: Vector, token: usize, state: &mut State) {
        let hidden = self.hidden;
        let mut x = vec![0.0; hidden];
        self.embedding.row(bytes, token, &mut x);
        let mut joined = vec![0.0; 2 * hidden];
        let mut gates = vec![0.0; 4 * hidden];
        for (layer, lstm) in self.layers.iter().enumerate() {
            joined[..hidden].copy_from_slice(&x);
            joined[hidden..].copy_from_slice(&state.hidden[layer]);
            vector.gemm(
                GemmShape {
                    rows: 1,
                    depth: 2 * hidden,
                    cols: 4 * hidden,
                },
                &joined,
                &lstm.weight,
                &mut gates,
            );
            for (gate, &b) in gates.iter_mut().zip(&lstm.bias) {
                *gate += b;
            }
            let (sigmoids, rest) = gates.split_at_mut(2 * hidden);
            let (cell_in, output) = rest.split_at_mut(hidden);
            vector.sigmoid(sigmoids);
            vector.tanh(cell_in);
            vector.sigmoid(output);
            let (input, forget) = sigmoids.split_at(hidden);
            let cell = &mut state.cell[layer];
            for i in 0..hidden {
                cell[i] = forget[i] * cell[i] + input[i] * cell_in[i];
            }
            x.copy_from_slice(cell);
            vector.tanh(&mut x);
            for (value, &o) in x.iter_mut().zip(output.iter()) {
                *value *= o;
            }
            state.hidden[layer].copy_from_slice(&x);
        }
        vector.gemm(
            GemmShape {
                rows: 1,
                depth: hidden,
                cols: hidden,
            },
            &x,
            &self.projector,
            &mut state.out,
        );
        for (value, &b) in state.out.iter_mut().zip(&self.projector_bias) {
            *value += b;
        }
    }
}
