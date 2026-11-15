mod butterfly;
mod puncture;
mod stream;
mod trellis;
mod viterbi;

use butterfly::Signs;
pub use puncture::{Depuncturer, depuncture, puncture};
pub use stream::StreamViterbiK7;
pub use viterbi::ViterbiK7;

const MEMORY: u32 = 6;
pub const STATES: usize = 1 << MEMORY;
const MAX_OUTPUTS: usize = 8;
const STATE_MASK: usize = STATES - 1;
const REGISTERS: usize = 2 * STATES;
/// The vector butterfly step holds only for generators tapping both ends of the register.
const OUTER_TAPS: u16 = 1 << MEMORY | 1;

#[derive(Clone, Debug)]
pub struct ConvCode {
    outputs: usize,
    branch: [u8; REGISTERS],
    signs: Option<Signs>,
}

impl ConvCode {
    #[must_use]
    pub fn new(polys: &[u16]) -> Self {
        assert!(
            (1..=MAX_OUTPUTS).contains(&polys.len()),
            "a K=7 code needs between 1 and {MAX_OUTPUTS} generators"
        );
        let mut branch = [0u8; REGISTERS];
        for (register, slot) in branch.iter_mut().enumerate() {
            let mut mask = 0u8;
            for (index, &poly) in polys.iter().enumerate() {
                if (register as u16 & poly).count_ones() % 2 == 1 {
                    mask |= 1 << index;
                }
            }
            *slot = mask;
        }
        let signs = polys
            .iter()
            .all(|poly| poly & OUTER_TAPS == OUTER_TAPS)
            .then(|| {
                std::array::from_fn(|bit| {
                    std::array::from_fn(|state| {
                        if branch[2 * state] & (1 << bit) == 0 {
                            -1
                        } else {
                            0
                        }
                    })
                })
            });
        Self {
            outputs: polys.len(),
            signs,
            branch,
        }
    }

    #[must_use]
    pub const fn outputs(&self) -> usize {
        self.outputs
    }

    fn register(state: usize, input: bool) -> usize {
        usize::from(input) << MEMORY | state
    }

    const fn advance(state: usize, input: bool) -> usize {
        state >> 1 | (input as usize) << (MEMORY - 1)
    }

    pub fn encode(&self, bits: &[bool], out: &mut Vec<bool>) {
        let mut state = 0usize;
        for &bit in bits {
            let mask = self.branch[Self::register(state, bit)];
            for index in 0..self.outputs {
                out.push(mask >> index & 1 == 1);
            }
            state = Self::advance(state, bit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{
            conv::{CONFIDENT, ERASURE, Soft, soft},
            test_data::random_bits as message,
        },
        trellis::Trellis,
        *,
    };
    use crate::math::rng::Rng;

    const DVB_S: [u16; 2] = [0o171, 0o133];
    const DAB: [u16; 4] = [0o133, 0o171, 0o145, 0o133];

    fn softs(bits: &[bool]) -> Vec<Soft> {
        bits.iter().copied().map(soft).collect()
    }

    fn tailed(len: usize, seed: u64) -> Vec<bool> {
        let mut bits = message(len, seed);
        bits.extend([false; 6]);
        bits
    }

    fn assert_butterflies_match_scalar(polys: &[u16]) {
        let mut scalar_code = ConvCode::new(polys);
        scalar_code.signs = None;
        let fast_code = ConvCode::new(polys);
        assert!(fast_code.signs.is_some());
        let mut fast = Trellis::new();
        fast.open();
        let mut scalar = Trellis::new();
        scalar.open();
        let mut rng = Rng::mixed(0x91ab21);
        let mut symbol = vec![0 as Soft; polys.len()];
        for _ in 0..4096 {
            for value in &mut symbol {
                *value = rng.bits() as Soft;
            }
            assert_eq!(
                fast.step(&fast_code, &symbol),
                scalar.step(&scalar_code, &symbol)
            );
            fast.normalize();
            scalar.normalize();
            assert_eq!(fast.metrics(), scalar.metrics());
        }
    }

    #[test]
    fn vector_butterflies_match_scalar_survivors_with_extreme_soft_values() {
        assert_butterflies_match_scalar(&DVB_S);
        assert_butterflies_match_scalar(&[0o133, 0o171, 0o165]);
        assert_butterflies_match_scalar(&DAB);
    }

    #[test]
    fn the_generator_taps_match_the_dvb_s_reference_response() {
        let code = ConvCode::new(&DVB_S);
        let mut coded = Vec::new();
        let mut impulse = vec![true];
        impulse.extend([false; 6]);
        code.encode(&impulse, &mut coded);
        let first: Vec<bool> = coded.iter().step_by(2).copied().collect();
        let second: Vec<bool> = coded.iter().skip(1).step_by(2).copied().collect();
        assert_eq!(first, [true, true, true, true, false, false, true]);
        assert_eq!(second, [true, false, true, true, false, true, true]);
    }

    #[test]
    fn a_terminated_rate_one_half_frame_round_trips() {
        let code = ConvCode::new(&DVB_S);
        let bits = tailed(128, 7);
        let mut coded = Vec::new();
        code.encode(&bits, &mut coded);
        let mut out = Vec::new();
        ViterbiK7::new(code).decode_tailed(&softs(&coded), &mut out);
        assert_eq!(out, bits);
    }

    #[test]
    fn the_rate_one_quarter_mother_code_repairs_scattered_errors() {
        let code = ConvCode::new(&DAB);
        let bits = tailed(96, 11);
        let mut coded = Vec::new();
        code.encode(&bits, &mut coded);
        let mut received = softs(&coded);
        for position in (7..received.len()).step_by(11) {
            received[position] = -received[position];
        }
        let mut out = Vec::new();
        ViterbiK7::new(code).decode_tailed(&received, &mut out);
        assert_eq!(out, bits);
    }

    #[test]
    fn puncturing_to_rate_three_quarters_still_decodes() {
        let pattern = [true, true, true, false, false, true];
        let code = ConvCode::new(&DVB_S);
        let bits = tailed(192, 13);
        let mut coded = Vec::new();
        code.encode(&bits, &mut coded);
        let mut sent = Vec::new();
        puncture(&coded, &pattern, &mut sent);
        assert_eq!(sent.len() * 3, coded.len() * 2);
        let mut received = Vec::new();
        depuncture(&softs(&sent), &pattern, &mut received);
        assert_eq!(received.len(), coded.len());
        let mut out = Vec::new();
        ViterbiK7::new(code).decode_tailed(&received, &mut out);
        assert_eq!(out, bits);
    }

    #[test]
    fn depuncturing_marks_the_dropped_positions_as_erasures() {
        let pattern = [true, false];
        let mut out = Vec::new();
        depuncture(&[CONFIDENT, -CONFIDENT], &pattern, &mut out);
        assert_eq!(out, [CONFIDENT, ERASURE, -CONFIDENT, ERASURE]);
    }

    #[test]
    fn the_stateful_depuncturer_matches_the_block_one_across_chunks() {
        let pattern = [true, true, false, true, true, false];
        let sent: Vec<Soft> = (0..96).map(|index| soft(index % 3 == 0)).collect();
        let mut block = Vec::new();
        depuncture(&sent, &pattern, &mut block);
        let mut streamed = Vec::new();
        let mut depuncturer = Depuncturer::new(&pattern);
        for chunk in sent.chunks(7) {
            depuncturer.process(chunk, &mut streamed);
        }
        assert_eq!(streamed, block[..streamed.len()]);
        assert!(
            block[streamed.len()..]
                .iter()
                .all(|&value| value == ERASURE)
        );
    }

    #[test]
    fn the_streaming_decoder_matches_the_block_decoder() {
        let code = ConvCode::new(&DVB_S);
        let bits = message(4_000, 29);
        let mut coded = Vec::new();
        code.encode(&bits, &mut coded);
        let received = softs(&coded);
        let mut stream = StreamViterbiK7::new(code, 96);
        let mut out = Vec::new();
        for block in received.chunks(512) {
            stream.push(block, &mut out);
        }
        stream.flush(&mut out);
        assert_eq!(out.len(), bits.len());
        assert_eq!(out[64..3_900], bits[64..3_900]);
    }

    #[test]
    fn the_streaming_decoder_carries_a_split_symbol_across_chunks() {
        let code = ConvCode::new(&DAB);
        let bits = message(2_000, 37);
        let mut coded = Vec::new();
        code.encode(&bits, &mut coded);
        let received = softs(&coded);
        let mut whole = StreamViterbiK7::new(ConvCode::new(&DAB), 96);
        let mut expected = Vec::new();
        whole.push(&received, &mut expected);
        whole.flush(&mut expected);
        let mut split = StreamViterbiK7::new(code, 96);
        let mut out = Vec::new();
        for chunk in received.chunks(37) {
            split.push(chunk, &mut out);
        }
        split.flush(&mut out);
        assert_eq!(out, expected);
        assert_eq!(out[16..1_900], bits[16..1_900]);
    }

    #[test]
    fn the_streaming_decoder_recovers_after_a_burst() {
        let code = ConvCode::new(&DVB_S);
        let bits = message(4_000, 31);
        let mut coded = Vec::new();
        code.encode(&bits, &mut coded);
        let mut received = softs(&coded);
        for value in &mut received[1_000..1_040] {
            *value = -*value;
        }
        let mut stream = StreamViterbiK7::new(code, 96);
        let mut out = Vec::new();
        stream.push(&received, &mut out);
        stream.flush(&mut out);
        assert_eq!(out[2_000..3_900], bits[2_000..3_900]);
    }
}
