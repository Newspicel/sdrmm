use std::sync::LazyLock;

use super::air::{BODY_DIBITS, ISCH_DIBITS, PACKET_DIBITS, SLOTS};
use super::mac::{Layout, PARITY, RS_LENGTH};

const SEED_BITS: usize = 44;
const SEED_MASK: u64 = (1 << SEED_BITS) - 1;
const SUPERFRAME_BITS: usize = SLOTS * PACKET_DIBITS * 2;
const SLOT_BITS: usize = PACKET_DIBITS * 2;
/// The sequence restarts each superframe half an ISCH before the first body.
const FIRST_BODY_BIT: usize = ISCH_DIBITS;
/// The register taps of x^44 + x^40 + x^35 + x^29 + x^24 + x^10 + 1, shifting left.
const TAPS: [usize; 6] = [43, 33, 19, 14, 8, 3];

/// The site a Phase 2 carrier scrambles with: the PN44 seed is WACN, system and NAC side by side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) wacn: u32,
    pub(crate) system: u16,
    pub(crate) nac: u16,
}

impl Identity {
    pub(crate) fn seed(self) -> u64 {
        let seed = u64::from(self.wacn & 0xF_FFFF) << 24
            | u64::from(self.system & 0xFFF) << 12
            | u64::from(self.nac & 0xFFF);
        if seed == 0 { SEED_MASK } else { seed }
    }

    fn of_seed(seed: u64) -> Self {
        Self {
            wacn: (seed >> 24) as u32 & 0xF_FFFF,
            system: (seed >> 12) as u16 & 0xFFF,
            nac: seed as u16 & 0xFFF,
        }
    }
}

struct Lfsr(u64);

impl Lfsr {
    fn next(&mut self) -> u8 {
        let out = (self.0 >> 43) as u8 & 1;
        let feedback = TAPS
            .iter()
            .fold(0u64, |acc, &tap| acc ^ (self.0 >> tap & 1));
        self.0 = (self.0 << 1 | feedback) & SEED_MASK;
        out
    }
}

/// The scrambling dibit of every body dibit in every timeslot of a superframe.
pub(crate) struct Masks {
    identity: Identity,
    dibits: [u8; SLOTS * BODY_DIBITS],
}

impl Masks {
    pub(crate) fn new(identity: Identity) -> Self {
        let mut lfsr = Lfsr(identity.seed());
        let mut sequence = [0u8; SUPERFRAME_BITS];
        for bit in &mut sequence {
            *bit = lfsr.next();
        }
        let mut dibits = [0u8; SLOTS * BODY_DIBITS];
        for (slot, body) in dibits
            .as_chunks_mut::<BODY_DIBITS>()
            .0
            .iter_mut()
            .enumerate()
        {
            let start = FIRST_BODY_BIT + slot * SLOT_BITS;
            for (dibit, pair) in body.iter_mut().zip(sequence[start..].as_chunks::<2>().0) {
                *dibit = pair[0] << 1 | pair[1];
            }
        }
        Self { identity, dibits }
    }

    pub(crate) fn identity(&self) -> Identity {
        self.identity
    }

    pub(crate) fn body(&self, slot: usize) -> &[u8] {
        &self.dibits[slot * BODY_DIBITS..(slot + 1) * BODY_DIBITS]
    }
}

/// Each superframe bit of the sequence as the set of seed bits it sums, so a burst's keystream
/// is linear in the unknown seed.
static KEYSTREAM_BASIS: LazyLock<Box<[u64]>> = LazyLock::new(|| {
    let mut state: [u64; SEED_BITS] = std::array::from_fn(|bit| 1 << bit);
    let mut basis = vec![0u64; SUPERFRAME_BITS].into_boxed_slice();
    for bit in &mut basis {
        *bit = state[SEED_BITS - 1];
        let feedback = TAPS.iter().fold(0, |acc, &tap| acc ^ state[tap]);
        state.rotate_right(1);
        state[0] = feedback;
    }
    basis
});

const CONSTANT: u32 = 127;

/// GF(64) on x^6 + x + 1: `EXP[i]` is x^i.
static EXP: [u8; 63] = gf64_powers();

const fn gf64_powers() -> [u8; 63] {
    let mut table = [0u8; 63];
    let mut value = 1u8;
    let mut index = 0;
    while index < 63 {
        table[index] = value;
        value <<= 1;
        if value & 0x40 != 0 {
            value ^= 0x43;
        }
        index += 1;
    }
    table
}

fn gf64_mul_power(value: u8, power: usize) -> u8 {
    let mut product = 0u8;
    for bit in 0..6 {
        if value >> bit & 1 == 1 {
            product ^= EXP[(bit + power) % 63];
        }
    }
    product
}

/// Recovers the seed a scrambled signalling burst was sent with, from the burst alone: the
/// received bits less a keystream linear in the seed must be a codeword, which gives the
/// Reed-Solomon syndromes as linear equations in the seed bits and the punctured symbols.
/// `None` when the burst holds bit errors or does not pin the seed.
pub(crate) fn solve_identity(body: &[u8], slot: usize, layout: &Layout) -> Option<Identity> {
    let punctured = (RS_LENGTH - layout.first - layout.present) * 6;
    let symbols = code_symbols(body, slot, layout, punctured);
    let mut pivots = [0u128; CONSTANT as usize];
    for power in 1..=PARITY {
        let mut syndrome = [0u128; 6];
        for (index, symbol) in symbols.iter().enumerate() {
            let exponent = power * (RS_LENGTH - 1 - index) % 63;
            for (bit, &form) in symbol.iter().enumerate() {
                if form == 0 {
                    continue;
                }
                let term = gf64_mul_power(1 << bit, exponent);
                for (out, row) in syndrome.iter_mut().enumerate() {
                    if term >> out & 1 == 1 {
                        *row ^= form;
                    }
                }
            }
        }
        for row in syndrome {
            if !reduce(&mut pivots, row, punctured + SEED_BITS) {
                return None;
            }
        }
    }
    seed_of(&pivots, punctured).map(Identity::of_seed)
}

/// Each code bit as an affine form: the received bit as the constant, the seed bits from
/// `punctured` up and the unknown punctured bits below.
fn code_symbols(
    body: &[u8],
    slot: usize,
    layout: &Layout,
    punctured: usize,
) -> [[u128; 6]; RS_LENGTH] {
    let base = FIRST_BODY_BIT + slot * SLOT_BITS;
    let mut symbols = [[0u128; 6]; RS_LENGTH];
    let received = layout.dibits().flat_map(|at| {
        let dibit = body[at];
        [(dibit >> 1, base + 2 * at), (dibit & 1, base + 2 * at + 1)]
    });
    for (index, (bit, position)) in received.enumerate() {
        let form = u128::from(bit) << CONSTANT | u128::from(KEYSTREAM_BASIS[position]) << punctured;
        symbols[layout.first + index / 6][5 - index % 6] = form;
    }
    let unknown = layout.first + layout.present;
    for (offset, symbol) in symbols[unknown..].iter_mut().enumerate() {
        for (bit, form) in symbol.iter_mut().enumerate() {
            *form = 1 << (offset * 6 + bit);
        }
    }
    symbols
}

/// Folds a row into the echelon `pivots`; false when it contradicts them.
fn reduce(pivots: &mut [u128; CONSTANT as usize], mut row: u128, unknowns: usize) -> bool {
    for (column, &pivot) in pivots.iter().enumerate().take(unknowns) {
        if row >> column & 1 == 1 && pivot != 0 {
            row ^= pivot;
        }
    }
    let lowest = (row & ((1 << unknowns) - 1)).trailing_zeros() as usize;
    if lowest >= unknowns {
        return row >> CONSTANT == 0;
    }
    pivots[lowest] = row;
    true
}

fn seed_of(pivots: &[u128; CONSTANT as usize], punctured: usize) -> Option<u64> {
    let mut seed = 0u64;
    for bit in (0..SEED_BITS).rev() {
        let row = pivots[punctured + bit];
        if row == 0 {
            return None;
        }
        let known = (row >> punctured) as u64 & SEED_MASK & !(1 << bit) & seed;
        let value = (row >> CONSTANT) as u64 ^ u64::from(known.count_ones() & 1);
        seed |= value << bit;
    }
    (seed != 0).then_some(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_matches_the_register_it_is_built_from() {
        let seed = Identity {
            wacn: 0xBEE00,
            system: 0x164,
            nac: 0x164,
        }
        .seed();
        let mut lfsr = Lfsr(seed);
        for &basis in KEYSTREAM_BASIS.iter() {
            let expected = (basis & seed).count_ones() as u8 & 1;
            assert_eq!(lfsr.next(), expected);
        }
    }

    #[test]
    fn the_first_bits_out_are_the_seed_itself() {
        let mut lfsr = Lfsr(0x0BEE_0016_4164);
        let first = (0..44).fold(0u64, |word, _| word << 1 | u64::from(lfsr.next()));
        assert_eq!(first, 0x0BEE_0016_4164);
    }

    #[test]
    fn an_all_zero_identity_seeds_all_ones() {
        let zero = Identity {
            wacn: 0,
            system: 0,
            nac: 0,
        };
        assert_eq!(zero.seed(), SEED_MASK);
    }
}
