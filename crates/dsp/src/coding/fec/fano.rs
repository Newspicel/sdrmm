//! Fano sequential decoding of a rate 1/n convolutional code with up to 32 bits of memory.

/// The rate 1/n code a [`Fano`] decoder searches: one output bit per polynomial, each the
/// parity of the shift register masked by it, newest bit lowest. `tail` zero bits flush the
/// register after `data` bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FanoCode<'a> {
    pub polynomials: &'a [u32],
    pub data: usize,
    pub tail: usize,
}

impl FanoCode<'_> {
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.data + self.tail
    }

    #[must_use]
    pub const fn coded(&self) -> usize {
        self.depth() * self.polynomials.len()
    }

    /// The coded bits of `data`, written to `out`, which must hold [`FanoCode::coded`] bits.
    pub fn encode(&self, data: impl IntoIterator<Item = bool>, out: &mut [u8]) {
        let mut data = data.into_iter();
        let mut register = 0u32;
        for step in out.chunks_exact_mut(self.polynomials.len()) {
            register = (register << 1) | u32::from(data.next().unwrap_or(false));
            for (bit, &polynomial) in step.iter_mut().zip(self.polynomials) {
                *bit = parity(register & polynomial);
            }
        }
    }

    /// The Fano metric of one received bit with log-likelihood ratio `llr`, positive for a 1,
    /// as `[metric if 0 was sent, metric if 1 was sent]`, in bits less the code rate.
    #[must_use]
    pub fn metric(&self, llr: f32) -> [f32; 2] {
        let rate = 1.0 / self.polynomials.len() as f32;
        let one = std::f32::consts::LOG2_E * -(-llr).exp().ln_1p();
        let zero = std::f32::consts::LOG2_E * -llr.exp().ln_1p();
        [1.0 + zero - rate, 1.0 + one - rate]
    }
}

fn parity(value: u32) -> u8 {
    (value.count_ones() & 1) as u8
}

#[derive(Clone, Copy, Default)]
struct Node {
    gamma: f32,
    register: u32,
    metrics: [f32; 2],
    bits: [u32; 2],
    choice: usize,
}

/// A Fano sequential decoder. It holds its search path, so decoding never allocates.
pub struct Fano<'a> {
    code: FanoCode<'a>,
    nodes: Vec<Node>,
}

impl<'a> Fano<'a> {
    #[must_use]
    pub fn new(code: FanoCode<'a>) -> Self {
        Self {
            nodes: vec![Node::default(); code.depth() + 1],
            code,
        }
    }

    /// Searches for the path through `metrics`, one `[0 sent, 1 sent]` pair per coded bit,
    /// stepping the threshold by `delta`. `false` when `max_cycles` forward and backward
    /// moves found no complete path or `metrics` is too short.
    pub fn decode(&mut self, metrics: &[[f32; 2]], delta: f32, max_cycles: usize) -> bool {
        if metrics.len() < self.code.coded() {
            return false;
        }
        let depth_limit = self.code.depth();
        self.branches(0, 0, metrics);
        let mut depth = 0;
        let mut threshold = 0.0f32;
        for _ in 0..max_cycles {
            let node = self.nodes[depth];
            let next = node.gamma + node.metrics[node.choice];
            if next >= threshold {
                if node.gamma < threshold + delta {
                    while next >= threshold + delta {
                        threshold += delta;
                    }
                }
                depth += 1;
                self.nodes[depth].gamma = next;
                self.nodes[depth].register = (node.register << 1) | node.bits[node.choice];
                if depth == depth_limit {
                    return true;
                }
                self.branches(depth, self.nodes[depth].register, metrics);
                continue;
            }
            loop {
                if depth == 0 || self.nodes[depth - 1].gamma < threshold {
                    threshold -= delta;
                    self.nodes[depth].choice = 0;
                    break;
                }
                depth -= 1;
                if self.nodes[depth].choice == 0 && self.nodes[depth].metrics[1].is_finite() {
                    self.nodes[depth].choice = 1;
                    break;
                }
            }
        }
        false
    }

    /// The data bits of the last successful [`Fano::decode`], first bit first.
    pub fn data(&self) -> impl Iterator<Item = bool> + '_ {
        self.nodes[1..=self.code.data]
            .iter()
            .map(|node| node.register & 1 == 1)
    }

    fn branches(&mut self, depth: usize, register: u32, metrics: &[[f32; 2]]) {
        let polynomials = self.code.polynomials;
        let score = |bit: u32| {
            let register = (register << 1) | bit;
            polynomials
                .iter()
                .enumerate()
                .map(|(output, &polynomial)| {
                    metrics[polynomials.len() * depth + output]
                        [usize::from(parity(register & polynomial))]
                })
                .sum::<f32>()
        };
        let (zero, one) = (score(0), score(1));
        let node = &mut self.nodes[depth];
        node.choice = 0;
        if depth >= self.code.data {
            node.metrics = [zero, f32::NEG_INFINITY];
            node.bits = [0, 1];
        } else if zero >= one {
            node.metrics = [zero, one];
            node.bits = [0, 1];
        } else {
            node.metrics = [one, zero];
            node.bits = [1, 0];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYLAND_LUSHBAUGH: [u32; 2] = [0xf2d0_5351, 0xe461_3c47];
    const CODE: FanoCode<'static> = FanoCode {
        polynomials: &LAYLAND_LUSHBAUGH,
        data: 50,
        tail: 31,
    };

    fn data(seed: u64) -> Vec<bool> {
        (0..CODE.data)
            .map(|bit| seed >> (bit % 64) & 1 == 1)
            .collect()
    }

    fn metrics(coded: &[u8], llr: impl Fn(usize) -> f32) -> Vec<[f32; 2]> {
        coded
            .iter()
            .enumerate()
            .map(|(index, &bit)| {
                let sign = if bit == 1 { 1.0 } else { -1.0 };
                CODE.metric(sign * llr(index))
            })
            .collect()
    }

    fn coded(data: &[bool]) -> Vec<u8> {
        let mut coded = vec![0; CODE.coded()];
        CODE.encode(data.iter().copied(), &mut coded);
        coded
    }

    #[test]
    fn the_encoder_flushes_with_zeros() {
        let mut out = vec![0; CODE.coded()];
        CODE.encode([true], &mut out);
        assert_eq!(&out[..2], &[1, 1]);
        assert_eq!(out.len(), 162);
        assert!(out[124..].iter().all(|&bit| bit == 0));
    }

    #[test]
    fn a_clean_word_decodes() {
        let data = data(0x2_3456_789a_bcde);
        let mut fano = Fano::new(CODE);
        assert!(fano.decode(&metrics(&coded(&data), |_| 4.0), 1.0, 10_000));
        assert!(fano.data().eq(data.iter().copied()));
    }

    #[test]
    fn scattered_errors_are_corrected() {
        let data = data(0x1_f00d_cafe_beef);
        let flipped = [3, 17, 40, 41, 77, 100, 130, 155];
        let received = metrics(&coded(&data), |index| {
            if flipped.contains(&index) { -1.5 } else { 2.0 }
        });
        let mut fano = Fano::new(CODE);
        assert!(fano.decode(&received, 1.0, 100_000));
        assert!(fano.data().eq(data.iter().copied()));
    }

    #[test]
    fn too_few_cycles_or_bits_are_refused() {
        let received = metrics(&coded(&data(7)), |_| 4.0);
        let mut fano = Fano::new(CODE);
        assert!(!fano.decode(&received, 1.0, CODE.depth() - 1));
        assert!(!fano.decode(&received[..10], 1.0, 10_000));
        assert!(fano.decode(&received, 1.0, CODE.depth()));
    }

    #[test]
    fn the_metric_rewards_agreement() {
        let [zero, one] = CODE.metric(3.0);
        assert!(one > 0.0 && zero < 0.0);
        assert_eq!(CODE.metric(0.0), [-0.5, -0.5]);
    }
}
