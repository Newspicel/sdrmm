use sdrmm_dsp::{ConvCode, ERASURE, Soft, ViterbiK7};

use super::layout::{BLOCK, BLOCKS, MAIN_BITS, P1_BITS, PIDS_BITS};

/// The rate 1/3 mother code, punctured to 2/5 by dropping one bit of every six.
const MOTHER: [u16; 3] = [0o133, 0o171, 0o165];
const PUNCTURED: usize = 5;
const PERIOD: usize = 6;
const WRAP: usize = 32;
/// The primary main interleaver's partitions, columns per partition and blocks per frame.
const PARTITIONS: usize = 20;
const COLUMNS: usize = 36;
/// The order the interleaver fills the twenty partitions in (NRSC-5 1011s table 9-2).
const ORDER: [usize; PARTITIONS] = [
    10, 2, 18, 6, 14, 8, 16, 0, 12, 4, 11, 3, 19, 7, 15, 9, 17, 1, 13, 5,
];
pub const P1_CODED: usize = P1_BITS * PUNCTURED / 2;
pub const PIDS_CODED: usize = PIDS_BITS * PUNCTURED / 2;

/// Deinterleaves and decodes the P1 logical channel and the PIDS frames riding with it in the
/// primary main sidebands.
pub struct Decoder {
    frame: Vec<Soft>,
    started: bool,
    mother: Vec<Soft>,
    viterbi: ViterbiK7,
    decoded: Vec<bool>,
    pub p1: Vec<u8>,
    pub pids: [u8; PIDS_BITS],
    pub p1_errors: f32,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            frame: vec![0; MAIN_BITS * BLOCKS],
            started: false,
            mother: Vec::with_capacity(P1_BITS * 3),
            viterbi: ViterbiK7::with_capacity(ConvCode::new(&MOTHER), P1_BITS + 2 * WRAP),
            decoded: Vec::with_capacity(P1_BITS),
            p1: vec![0; P1_BITS],
            pids: [0; PIDS_BITS],
            p1_errors: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.started = false;
    }

    /// Stores block `block`'s soft bits and decodes its PIDS frame into `pids`; returns
    /// whether the block completed a P1 frame, now in `p1`.
    pub fn push(&mut self, block: usize, soft: &[Soft]) -> bool {
        self.frame[block * MAIN_BITS..(block + 1) * MAIN_BITS].copy_from_slice(soft);
        self.decode_pids(block);
        if block == 0 {
            self.started = true;
        }
        if self.started && block + 1 == BLOCKS {
            self.decode_p1();
            return true;
        }
        false
    }

    fn decode_pids(&mut self, block: usize) {
        self.mother.clear();
        for i in block * PIDS_CODED..(block + 1) * PIDS_CODED {
            let k = (i / PARTITIONS) % (PIDS_CODED / PARTITIONS) + P1_CODED / (PARTITIONS * BLOCKS);
            let value = self.frame[cell(block, ORDER[i % PARTITIONS], k)];
            push_depunctured(&mut self.mother, value);
        }
        self.decoded.clear();
        self.viterbi
            .decode_tail_biting(&self.mother, WRAP, &mut self.decoded);
        let mut scrambler = Scrambler::new();
        for (bit, &decoded) in self.pids.iter_mut().zip(&self.decoded) {
            *bit = u8::from(decoded) ^ scrambler.next_bit();
        }
    }

    fn decode_p1(&mut self) {
        self.mother.clear();
        for i in 0..P1_CODED {
            let partition = ORDER[i % PARTITIONS];
            let block = (i / PARTITIONS + partition * 7) % BLOCKS;
            let k = i / (PARTITIONS * BLOCKS);
            let value = self.frame[cell(block, partition, k)];
            push_depunctured(&mut self.mother, value);
        }
        self.decoded.clear();
        self.viterbi
            .decode_tail_biting(&self.mother, WRAP, &mut self.decoded);
        self.p1_errors = coded_errors(&self.mother, &self.decoded) as f32 / P1_CODED as f32;
        let mut scrambler = Scrambler::new();
        for (bit, &decoded) in self.p1.iter_mut().zip(&self.decoded) {
            *bit = u8::from(decoded) ^ scrambler.next_bit();
        }
    }
}

/// Where interleaver index `k` of `partition` sits in a block's soft bits.
fn cell(block: usize, partition: usize, k: usize) -> usize {
    let row = (k * 11) % BLOCK;
    let column = (k * 11 + k / (BLOCK * 9)) % COLUMNS;
    block * MAIN_BITS + row * PARTITIONS * COLUMNS + partition * COLUMNS + column
}

fn push_depunctured(mother: &mut Vec<Soft>, value: Soft) {
    mother.push(value);
    if mother.len() % PERIOD == PUNCTURED {
        mother.push(ERASURE);
    }
}

/// Coded bits that disagree with the decoded bits encoded again, the channel's bit errors.
fn coded_errors(mother: &[Soft], decoded: &[bool]) -> usize {
    let len = decoded.len();
    if len < 7 {
        return 0;
    }
    let mut register = decoded[len - 6..]
        .iter()
        .enumerate()
        .fold(0u16, |acc, (i, &bit)| acc | u16::from(bit) << (1 + i));
    let mut errors = 0;
    for (i, &bit) in decoded.iter().enumerate() {
        register = register >> 1 | u16::from(bit) << 6;
        for (j, poly) in MOTHER.iter().enumerate() {
            let value = mother[3 * i + j];
            let expected = (register & poly).count_ones() % 2 == 1;
            if value != ERASURE && (value > 0) != expected {
                errors += 1;
            }
        }
    }
    errors
}

/// The 11-stage descrambler each P1 and PIDS frame starts afresh.
struct Scrambler {
    state: u16,
}

impl Scrambler {
    const fn new() -> Self {
        Self { state: 0x3ff }
    }

    fn next_bit(&mut self) -> u8 {
        let bit = ((self.state >> 9) ^ self.state) & 1;
        self.state = (self.state | bit << 11) >> 1;
        bit as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interleaver_fills_every_soft_bit_of_a_frame_once() {
        let mut seen = vec![false; MAIN_BITS * BLOCKS];
        for i in 0..P1_CODED {
            let partition = ORDER[i % PARTITIONS];
            let block = (i / PARTITIONS + partition * 7) % BLOCKS;
            let at = cell(block, partition, i / (PARTITIONS * BLOCKS));
            assert!(!seen[at], "P1 index {i} reused {at}");
            seen[at] = true;
        }
        for block in 0..BLOCKS {
            for i in block * PIDS_CODED..(block + 1) * PIDS_CODED {
                let k =
                    (i / PARTITIONS) % (PIDS_CODED / PARTITIONS) + P1_CODED / (PARTITIONS * BLOCKS);
                let at = cell(block, ORDER[i % PARTITIONS], k);
                assert!(!seen[at], "PIDS index {i} reused {at}");
                seen[at] = true;
            }
        }
        assert!(seen.iter().all(|&used| used));
    }

    #[test]
    fn the_scrambler_is_a_maximal_length_sequence() {
        let mut scrambler = Scrambler::new();
        let bits: Vec<u8> = (0..2 * 2047).map(|_| scrambler.next_bit()).collect();
        assert_eq!(&bits[..8], &[0, 1, 1, 0, 0, 1, 1, 0]);
        assert_eq!(bits[..2047], bits[2047..]);
        assert!((1..2047).all(|period| bits[..64] != bits[period..period + 64]));
    }
}
