use sdrmm_dsp::{Soft, Viterbi5};

use super::burst::{BLOCK_BURSTS, BURST_DATA, SYNC_DATA};

pub(crate) const G0: u8 = 0b1_1001;
pub(crate) const G1: u8 = 0b1_1011;
pub(crate) const BLOCK_BYTES: usize = 23;
pub(crate) const BLOCK_CODED: usize = 456;
pub(crate) const BLOCK_INFO: usize = 184;
pub(crate) const FIRE_PARITY: usize = 40;
const BLOCK_TAIL: usize = 4;
const BLOCK_DECODED: usize = BLOCK_INFO + FIRE_PARITY + BLOCK_TAIL;
const FIRE_POLY: u64 = 0x0_0482_0009;
const FIRE_DEGREE: u32 = 40;
pub(crate) const SCH_INFO: usize = 25;
pub(crate) const SCH_PARITY: usize = 10;
const SCH_POLY: u64 = 0x175;
const SCH_DEGREE: u32 = 10;
const SOFT_SCALE: f32 = 16.0;
const SOFT_LIMIT: f32 = 2_000.0;

pub(crate) struct Decoder {
    viterbi: Viterbi5,
    soft: Vec<Soft>,
    bits: Vec<bool>,
}

impl Decoder {
    pub(crate) fn new() -> Self {
        Self {
            viterbi: Viterbi5::with_generators(G0, G1),
            soft: Vec::with_capacity(BLOCK_CODED),
            bits: Vec::with_capacity(BLOCK_DECODED),
        }
    }

    pub(crate) fn block(
        &mut self,
        bursts: &[[f32; BURST_DATA]; BLOCK_BURSTS],
    ) -> Option<[u8; BLOCK_BYTES]> {
        self.soft.clear();
        self.soft.extend((0..BLOCK_CODED).map(|k| {
            let (burst, position) = interleaved_position(k);
            to_soft(bursts[burst][position])
        }));
        self.bits.clear();
        self.viterbi.decode(&self.soft, &mut self.bits);
        let checked = &self.bits[..BLOCK_INFO + FIRE_PARITY];
        if remainder(checked, FIRE_POLY, FIRE_DEGREE) != ones(FIRE_DEGREE) {
            return None;
        }
        let mut out = [0u8; BLOCK_BYTES];
        for (i, &bit) in self.bits[..BLOCK_INFO].iter().enumerate() {
            out[i / 8] |= u8::from(bit) << (i % 8);
        }
        Some(out)
    }

    pub(crate) fn sync(&mut self, coded: &[f32; 2 * SYNC_DATA]) -> Option<[bool; SCH_INFO]> {
        self.soft.clear();
        self.soft.extend(coded.iter().map(|&llr| to_soft(llr)));
        self.bits.clear();
        self.viterbi.decode(&self.soft, &mut self.bits);
        if remainder(&self.bits[..SCH_INFO + SCH_PARITY], SCH_POLY, SCH_DEGREE) != ones(SCH_DEGREE)
        {
            return None;
        }
        let mut info = [false; SCH_INFO];
        info.copy_from_slice(&self.bits[..SCH_INFO]);
        Some(info)
    }
}

pub(crate) fn interleaved_position(k: usize) -> (usize, usize) {
    (k % 4, 2 * ((49 * k) % 57) + (k % 8) / 4)
}

fn to_soft(llr: f32) -> Soft {
    (-llr * SOFT_SCALE).clamp(-SOFT_LIMIT, SOFT_LIMIT) as Soft
}

fn ones(degree: u32) -> u64 {
    (1u64 << degree) - 1
}

pub(crate) fn remainder(bits: &[bool], poly: u64, degree: u32) -> u64 {
    let top = 1u64 << degree;
    let mut register = 0u64;
    for &bit in bits {
        register = register << 1 | u64::from(bit);
        if register & top != 0 {
            register ^= poly | top;
        }
    }
    register
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn fire_parity(info: &[bool]) -> u64 {
    let mut shifted: Vec<bool> = info.to_vec();
    shifted.extend(std::iter::repeat_n(false, FIRE_PARITY));
    remainder(&shifted, FIRE_POLY, FIRE_DEGREE) ^ ones(FIRE_DEGREE)
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn sync_parity(info: &[bool]) -> u64 {
    let mut shifted: Vec<bool> = info.to_vec();
    shifted.extend(std::iter::repeat_n(false, SCH_PARITY));
    remainder(&shifted, SCH_POLY, SCH_DEGREE) ^ ones(SCH_DEGREE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fire_generator_is_the_product_of_its_two_factors() {
        let a = (1u128 << 23) | 1;
        let b = (1u128 << 17) | (1 << 3) | 1;
        let mut product = 0u128;
        for shift in 0..24 {
            if a >> shift & 1 == 1 {
                product ^= b << shift;
            }
        }
        assert_eq!(product, u128::from(FIRE_POLY) | (1 << 40));
    }

    #[test]
    fn interleaving_visits_every_burst_position_once() {
        let mut seen = [[false; BURST_DATA]; BLOCK_BURSTS];
        for k in 0..BLOCK_CODED {
            let (burst, position) = interleaved_position(k);
            assert!(!seen[burst][position]);
            seen[burst][position] = true;
        }
    }

    #[test]
    fn parity_appended_to_its_message_leaves_an_all_ones_remainder() {
        let info: Vec<bool> = (0..BLOCK_INFO).map(|k| k % 3 == 0 || k % 7 == 1).collect();
        let parity = fire_parity(&info);
        let mut word = info.clone();
        word.extend((0..FIRE_PARITY).rev().map(|k| parity >> k & 1 == 1));
        assert_eq!(remainder(&word, FIRE_POLY, FIRE_DEGREE), ones(FIRE_DEGREE));
    }
}
