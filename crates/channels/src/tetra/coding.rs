use std::sync::LazyLock;

use sdrmm_dsp::crc16_msb_bits;

pub(crate) const CRC_POLY: u16 = 0x1021;
pub(crate) const CRC_RESIDUE: u16 = 0x1D0F;
pub(crate) const CRC_BITS: usize = 16;
pub(crate) const TAIL_BITS: usize = 4;
pub(crate) const MOTHER_RATE: usize = 4;

pub(crate) const SB1_SCRAMBLING: u32 = 3;

pub(crate) const GENERATORS: [u8; MOTHER_RATE] = [0b1_0011, 0b1_1101, 0b1_0111, 0b1_1011];

const STATES: usize = 16;
const PUNCTURE_2_3: [usize; 4] = [0, 1, 2, 5];
const PUNCTURE_PERIOD: usize = 8;
const PUNCTURE_KEPT: usize = 3;

pub(crate) const MAX_TYPE2_BITS: usize = 288;
pub(crate) const MAX_TYPE5_BITS: usize = 432;

pub(crate) type Soft = i16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) type5: usize,
    pub(crate) type1: usize,
    pub(crate) interleave: usize,
}

impl Block {
    pub(crate) const fn type2(self) -> usize {
        self.type1 + CRC_BITS + TAIL_BITS
    }
}

pub(crate) const BSCH: Block = Block {
    type5: 120,
    type1: 60,
    interleave: 11,
};

pub(crate) const HALF_SLOT: Block = Block {
    type5: 216,
    type1: 124,
    interleave: 101,
};

pub(crate) const FULL_SLOT: Block = Block {
    type5: 432,
    type1: 268,
    interleave: 103,
};

pub(crate) const AACH_BITS: usize = 30;
pub(crate) const AACH_INFO_BITS: usize = 14;

pub(crate) fn scrambling_code(mcc: u16, mnc: u16, colour: u8) -> u32 {
    let seed =
        u32::from(colour & 0x3F) | u32::from(mnc & 0x3FFF) << 6 | u32::from(mcc & 0x3FF) << 20;
    seed << 2 | SB1_SCRAMBLING
}

pub(crate) fn scrambling_sequence(init: u32, out: &mut [bool]) {
    let mut lfsr = init;
    for bit in out {
        let feedback = [32, 26, 23, 22, 16, 12, 11, 10, 8, 7, 5, 4, 2, 1]
            .iter()
            .fold(0, |acc, &tap| acc ^ lfsr >> (32 - tap))
            & 1;
        lfsr = lfsr >> 1 | feedback << 31;
        *bit = feedback == 1;
    }
}

pub(crate) fn interleaved_position(k: usize, a: usize, i: usize) -> usize {
    (a * (i + 1)) % k
}

pub(crate) fn punctured_position(j: usize) -> usize {
    let group = j / PUNCTURE_KEPT;
    PUNCTURE_PERIOD * group + PUNCTURE_2_3[j - PUNCTURE_KEPT * group + 1] - 1
}

pub(crate) fn conv_output(register: u8) -> [bool; MOTHER_RATE] {
    GENERATORS.map(|g| (register & g).count_ones() % 2 == 1)
}

static OUTPUTS: LazyLock<[[Soft; MOTHER_RATE]; 32]> = LazyLock::new(|| {
    std::array::from_fn(|register| conv_output(register as u8).map(|bit| if bit { 1 } else { -1 }))
});

pub(crate) struct BlockDecoder {
    deinterleaved: [Soft; MAX_TYPE5_BITS],
    mother: [Soft; MAX_TYPE2_BITS * MOTHER_RATE],
    decisions: [u16; MAX_TYPE2_BITS],
    type2: [bool; MAX_TYPE2_BITS],
}

impl BlockDecoder {
    pub(crate) fn new() -> Self {
        Self {
            deinterleaved: [0; MAX_TYPE5_BITS],
            mother: [0; MAX_TYPE2_BITS * MOTHER_RATE],
            decisions: [0; MAX_TYPE2_BITS],
            type2: [false; MAX_TYPE2_BITS],
        }
    }

    pub(crate) fn decode(
        &mut self,
        block: Block,
        type5: &[Soft],
        scrambling: &[bool],
    ) -> Option<&[bool]> {
        let type2 = block.type2();
        if type5.len() != block.type5 || scrambling.len() < block.type5 || type2 > MAX_TYPE2_BITS {
            return None;
        }
        for i in 0..block.type5 {
            let k = interleaved_position(block.type5, block.interleave, i);
            let soft = type5[k];
            self.deinterleaved[i] = if scrambling[k] { -soft } else { soft };
        }
        let mother = &mut self.mother[..type2 * MOTHER_RATE];
        mother.fill(0);
        for (j, &soft) in self.deinterleaved[..block.type5].iter().enumerate() {
            if let Some(slot) = mother.get_mut(punctured_position(j)) {
                *slot = soft;
            }
        }
        viterbi(
            mother,
            &mut self.decisions[..type2],
            &mut self.type2[..type2],
        );
        let checked = &self.type2[..block.type1 + CRC_BITS];
        (crc16_msb_bits(CRC_POLY, 0xFFFF, checked) == CRC_RESIDUE)
            .then(|| &self.type2[..block.type1])
    }
}

fn viterbi(mother: &[Soft], decisions: &mut [u16], out: &mut [bool]) {
    let outputs = &*OUTPUTS;
    let mut metrics = [i32::MIN / 4; STATES];
    metrics[0] = 0;
    let mut next = [0i32; STATES];
    for (step, received) in mother.as_chunks::<MOTHER_RATE>().0.iter().enumerate() {
        let received = received.map(i32::from);
        let mut chosen = 0u16;
        for (state, metric) in next.iter_mut().enumerate() {
            let input = state & 1;
            let mut best = (i32::MIN, false);
            for high in [false, true] {
                let previous = state >> 1 | usize::from(high) << 3;
                let register = previous << 1 | input;
                let branch: i32 = outputs[register]
                    .iter()
                    .zip(received)
                    .map(|(&sign, r)| i32::from(sign) * r)
                    .sum();
                let candidate = metrics[previous].saturating_add(branch);
                if candidate > best.0 {
                    best = (candidate, high);
                }
            }
            *metric = best.0;
            chosen |= u16::from(best.1) << state;
        }
        if let Some(slot) = decisions.get_mut(step) {
            *slot = chosen;
        }
        metrics = next;
    }
    let mut state = 0usize;
    for step in (0..out.len()).rev() {
        out[step] = state & 1 == 1;
        let high = decisions[step] >> state & 1 == 1;
        state = state >> 1 | usize::from(high) << 3;
    }
}

const RM_GENERATOR: [u16; AACH_INFO_BITS] = [
    0b1001_1011_0110_0000,
    0b0010_1101_1110_0000,
    0b1111_1100_0010_0000,
    0b1110_0000_0011_1100,
    0b1001_1000_0011_1010,
    0b0101_0100_0011_0110,
    0b0010_1100_0010_1110,
    0b1111_1111_1101_1111,
    0b1000_0011_0011_1001,
    0b0100_0010_1011_0101,
    0b0010_0001_1010_1101,
    0b0001_0010_0111_0011,
    0b0000_1001_0110_1011,
    0b0000_0100_1110_0111,
];

pub(crate) fn rm_parity(info: u16) -> u16 {
    RM_GENERATOR
        .iter()
        .enumerate()
        .filter(|&(row, _)| info >> (AACH_INFO_BITS - 1 - row) & 1 == 1)
        .fold(0, |acc, (_, &parity)| acc ^ parity)
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn rm_encode(info: u16) -> u32 {
    u32::from(info & 0x3FFF) << 16 | u32::from(rm_parity(info))
}

const RM_CORRECTABLE: u32 = 3;

static RM_SYNDROMES: LazyLock<Vec<u32>> = LazyLock::new(|| {
    let mut table = vec![u32::MAX; 1 << 16];
    let mut patterns = vec![0u32];
    for weight in 1..=RM_CORRECTABLE {
        let previous: Vec<u32> = patterns
            .iter()
            .copied()
            .filter(|p| p.count_ones() == weight - 1)
            .collect();
        for pattern in previous {
            let lowest = if pattern == 0 {
                30
            } else {
                pattern.trailing_zeros()
            };
            patterns.extend((0..lowest).map(|bit| pattern | 1 << bit));
        }
    }
    for pattern in patterns {
        let slot = &mut table[usize::from(rm_syndrome(pattern))];
        if *slot == u32::MAX || slot.count_ones() > pattern.count_ones() {
            *slot = pattern;
        }
    }
    table
});

fn rm_syndrome(word: u32) -> u16 {
    rm_parity((word >> 16) as u16 & 0x3FFF) ^ word as u16
}

pub(crate) fn rm_decode(word: u32) -> Option<(u16, u32)> {
    let error = *RM_SYNDROMES.get(usize::from(rm_syndrome(word)))?;
    (error != u32::MAX).then(|| (((word ^ error) >> 16) as u16 & 0x3FFF, error.count_ones()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(type2: &[bool]) -> Vec<bool> {
        let mut register = 0u8;
        let mut out = Vec::new();
        for &bit in type2 {
            register = (register << 1 | u8::from(bit)) & 0x1F;
            out.extend(conv_output(register));
        }
        out
    }

    fn message(block: Block, seed: u32) -> Vec<bool> {
        let mut state = seed | 1;
        let mut bits: Vec<bool> = (0..block.type1)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state & 1 == 1
            })
            .collect();
        let crc = !crc16_msb_bits(CRC_POLY, 0xFFFF, &bits);
        bits.extend((0..16).rev().map(|i| crc >> i & 1 == 1));
        bits.extend([false; TAIL_BITS]);
        bits
    }

    fn type5(block: Block, type2: &[bool], scrambling: &[bool]) -> Vec<Soft> {
        let mother = encode(type2);
        let type3: Vec<bool> = (0..block.type5)
            .map(|j| mother[punctured_position(j)])
            .collect();
        let mut type4 = vec![false; block.type5];
        for (i, &bit) in type3.iter().enumerate() {
            type4[interleaved_position(block.type5, block.interleave, i)] = bit;
        }
        type4
            .iter()
            .zip(scrambling)
            .map(|(&bit, &s)| if bit ^ s { 64 } else { -64 })
            .collect()
    }

    #[test]
    fn the_crc_residue_accepts_an_intact_block() {
        let bits = message(BSCH, 7);
        assert_eq!(crc16_msb_bits(CRC_POLY, 0xFFFF, &bits[..76]), CRC_RESIDUE);
    }

    #[test]
    fn every_block_size_round_trips_through_scrambling_and_errors() {
        let mut scrambling = [false; MAX_TYPE5_BITS];
        scrambling_sequence(scrambling_code(262, 1010, 17), &mut scrambling);
        let mut decoder = BlockDecoder::new();
        for (block, seed) in [(BSCH, 3), (HALF_SLOT, 5), (FULL_SLOT, 9)] {
            let type2 = message(block, seed);
            let mut soft = type5(block, &type2, &scrambling);
            for i in (0..block.type5).step_by(17) {
                let k = interleaved_position(block.type5, block.interleave, i);
                soft[k] = -soft[k];
            }
            let decoded = decoder.decode(block, &soft, &scrambling).expect("decodes");
            assert_eq!(decoded, &type2[..block.type1]);
        }
    }

    #[test]
    fn a_wrecked_block_fails_its_crc() {
        let scrambling = [false; MAX_TYPE5_BITS];
        let type2 = message(HALF_SLOT, 11);
        let mut soft = type5(HALF_SLOT, &type2, &scrambling);
        for k in (0..soft.len()).step_by(3) {
            soft[k] = -soft[k];
        }
        assert!(
            BlockDecoder::new()
                .decode(HALF_SLOT, &soft, &scrambling)
                .is_none()
        );
    }

    #[test]
    fn rate_two_thirds_keeps_three_of_every_eight_mother_bits() {
        let kept: Vec<usize> = (0..6).map(punctured_position).collect();
        assert_eq!(kept, [0, 1, 4, 8, 9, 12]);
    }

    #[test]
    fn the_sb1_scrambler_matches_the_published_lfsr() {
        let mut bits = [false; 8];
        scrambling_sequence(SB1_SCRAMBLING, &mut bits);
        let mut lfsr = SB1_SCRAMBLING;
        for &bit in &bits {
            let taps = [32u32, 26, 23, 22, 16, 12, 11, 10, 8, 7, 5, 4, 2, 1];
            let feedback = taps.iter().map(|&t| lfsr >> (32 - t) & 1).sum::<u32>() & 1;
            lfsr = lfsr >> 1 | feedback << 31;
            assert_eq!(bit, feedback == 1);
        }
    }

    #[test]
    fn the_reed_muller_code_has_distance_eight() {
        let distance = (1u16..1 << 14)
            .map(|info| rm_encode(info).count_ones())
            .min()
            .unwrap();
        assert!(distance >= 7, "minimum distance {distance}");
    }

    #[test]
    fn the_reed_muller_decoder_corrects_three_errors() {
        for info in [0u16, 0x1234, 0x3FFF, 0x2AAA] {
            let word = rm_encode(info) ^ (1 << 3 | 1 << 17 | 1 << 29);
            assert_eq!(rm_decode(word), Some((info, 3)));
            assert_eq!(rm_decode(rm_encode(info)), Some((info, 0)));
        }
    }
}
