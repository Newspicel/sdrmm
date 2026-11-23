use super::{
    matmul::{HalfMatrix, matmul},
    mel::Features,
    tensor::add_rows,
};

/// A 3x3 stride-2 convolution's weights, `[channel][row][col]`, and its bias.
pub(super) struct Conv3 {
    pub weight: Vec<f32>,
    pub bias: Vec<f32>,
}

pub(super) struct Stage {
    pub depthwise: Conv3,
    pub pointwise: HalfMatrix,
    pub pointwise_bias: Vec<f32>,
}

/// The convolutional front of the encoder: eight times fewer frames, each a model-width vector.
pub(super) struct Subsampling {
    pub channels: usize,
    pub first: Conv3,
    pub stages: Vec<Stage>,
    pub linear: HalfMatrix,
    pub linear_bias: Vec<f32>,
}

/// Activations laid out `[time][freq][channel]`.
struct Plane {
    data: Vec<f32>,
    time: usize,
    freq: usize,
    channels: usize,
}

const fn halved(len: usize) -> usize {
    len.div_ceil(2)
}

impl Subsampling {
    /// The subsampled frames and how many of them the clip fills.
    pub fn run(
        &self,
        bytes: &[u8],
        features: &Features,
        bands: usize,
        threads: usize,
    ) -> (Vec<f32>, usize) {
        let mut valid = halved(features.valid);
        let mut plane = self.first_conv(features, bands);
        plane.mask(valid);
        plane.relu();
        for stage in &self.stages {
            valid = halved(valid);
            plane = plane.depthwise(&stage.depthwise);
            plane.mask(valid);
            plane = plane.pointwise(bytes, &stage.pointwise, &stage.pointwise_bias, threads);
            plane.mask(valid);
            plane.relu();
        }
        let flat = plane.channel_major();
        let width = self.linear.cols;
        let mut out = vec![0.0; plane.time * width];
        matmul(bytes, &self.linear, &flat, plane.time, &mut out, threads);
        add_rows(&mut out, &self.linear_bias);
        out.truncate(valid * width);
        (out, valid)
    }

    fn first_conv(&self, features: &Features, bands: usize) -> Plane {
        let (time, freq, channels) = (halved(features.count), halved(bands), self.channels);
        let mut data = vec![0.0; time * freq * channels];
        let at = |t: isize, f: isize| -> f32 {
            if t < 0 || f < 0 || t as usize >= features.count || f as usize >= bands {
                0.0
            } else {
                features.frames[t as usize * bands + f as usize]
            }
        };
        for t in 0..time {
            for f in 0..freq {
                let patch: [f32; 9] = std::array::from_fn(|i| {
                    at(
                        2 * t as isize + (i / 3) as isize - 1,
                        2 * f as isize + (i % 3) as isize - 1,
                    )
                });
                let out = &mut data[(t * freq + f) * channels..(t * freq + f + 1) * channels];
                for (c, value) in out.iter_mut().enumerate() {
                    let weights = &self.first.weight[c * 9..c * 9 + 9];
                    *value = self.first.bias[c]
                        + weights.iter().zip(&patch).map(|(w, x)| w * x).sum::<f32>();
                }
            }
        }
        Plane {
            data,
            time,
            freq,
            channels,
        }
    }
}

impl Plane {
    fn mask(&mut self, valid: usize) {
        let row = self.freq * self.channels;
        if valid < self.time {
            self.data[valid * row..].fill(0.0);
        }
    }

    fn relu(&mut self) {
        for value in &mut self.data {
            *value = value.max(0.0);
        }
    }

    fn depthwise(&self, conv: &Conv3) -> Self {
        let (time, freq, channels) = (halved(self.time), halved(self.freq), self.channels);
        let mut data = vec![0.0; time * freq * channels];
        for t in 0..time {
            for f in 0..freq {
                let out = &mut data[(t * freq + f) * channels..(t * freq + f + 1) * channels];
                out.copy_from_slice(&conv.bias);
                for i in 0..9 {
                    let (st, sf) = (
                        (2 * t + i / 3).wrapping_sub(1),
                        (2 * f + i % 3).wrapping_sub(1),
                    );
                    if st >= self.time || sf >= self.freq {
                        continue;
                    }
                    let input = &self.data[(st * self.freq + sf) * channels..][..channels];
                    for (c, (value, &x)) in out.iter_mut().zip(input).enumerate() {
                        *value += conv.weight[c * 9 + i] * x;
                    }
                }
            }
        }
        Self {
            data,
            time,
            freq,
            channels,
        }
    }

    fn pointwise(&self, bytes: &[u8], matrix: &HalfMatrix, bias: &[f32], threads: usize) -> Self {
        let rows = self.time * self.freq;
        let mut data = vec![0.0; rows * matrix.cols];
        matmul(bytes, matrix, &self.data, rows, &mut data, threads);
        add_rows(&mut data, bias);
        Self {
            data,
            time: self.time,
            freq: self.freq,
            channels: matrix.cols,
        }
    }

    /// Each time step as `[channel][freq]`, the order the projection was trained on.
    fn channel_major(&self) -> Vec<f32> {
        let mut flat = vec![0.0; self.data.len()];
        let row = self.freq * self.channels;
        for t in 0..self.time {
            for f in 0..self.freq {
                for c in 0..self.channels {
                    flat[t * row + c * self.freq + f] = self.data[t * row + f * self.channels + c];
                }
            }
        }
        flat
    }
}
