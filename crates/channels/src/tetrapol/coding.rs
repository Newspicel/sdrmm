#[cfg(any(test, feature = "synth"))]
mod encode;

use sdrmm_wire::TetrapolBand;

#[cfg(any(test, feature = "synth"))]
pub(crate) use encode::{encode_data, encode_voice, frame_bits};

pub(crate) const FRAME_BITS: usize = 160;
pub(crate) const SYNC_BITS: usize = 8;
pub(crate) const SYNC: [u8; SYNC_BITS] = [0, 1, 1, 0, 0, 0, 1, 0];
pub(crate) const PAYLOAD_BITS: usize = FRAME_BITS - SYNC_BITS;
pub(crate) const BLOCK_BYTES: usize = 8;
pub(crate) const SCRAMBLING_CODES: usize = 128;

const FIRST_STEPS: usize = 26;
const SECOND_STEPS: usize = 50;
const FIRST_CODED: usize = 2 * FIRST_STEPS;
const DATA_FIELD_BITS: usize = 74;
const VOICE_CRC: u32 = 0b1011;
const DATA_CRC: u32 = 0b10_0101;
const SCRAMBLER_PERIOD: usize = 127;
const STATES: usize = 4;

const UHF_DATA: [u8; PAYLOAD_BITS] = [
    1, 77, 38, 114, 20, 96, 59, 135, 3, 79, 41, 117, 23, 99, 62, 138, 5, 81, 44, 120, 26, 102, 65,
    141, 8, 84, 47, 123, 29, 105, 68, 144, 11, 87, 50, 126, 32, 108, 71, 147, 14, 90, 53, 129, 35,
    111, 74, 150, 17, 93, 56, 132, 37, 112, 76, 148, 2, 88, 40, 115, 19, 97, 58, 133, 4, 75, 43,
    118, 22, 100, 61, 136, 7, 85, 46, 121, 25, 103, 64, 139, 10, 82, 49, 124, 28, 106, 67, 142, 13,
    91, 52, 127, 31, 109, 73, 145, 16, 94, 55, 130, 34, 113, 70, 151, 0, 80, 39, 116, 21, 95, 57,
    134, 6, 78, 42, 119, 24, 98, 60, 137, 9, 83, 45, 122, 27, 101, 63, 140, 12, 86, 48, 125, 30,
    104, 66, 143, 15, 89, 51, 128, 33, 107, 69, 146, 18, 92, 54, 131, 36, 110, 72, 149,
];

const BIT_REVERSE: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

const PRECODED: [bool; PAYLOAD_BITS] = precoded();

const SCRAMBLER: [u8; SCRAMBLER_PERIOD] = scrambler();

const fn precoded() -> [bool; PAYLOAD_BITS] {
    let mut table = [false; PAYLOAD_BITS];
    let mut j = 7;
    while j < PAYLOAD_BITS {
        table[j] = true;
        j += if j == 76 { 7 } else { 3 };
    }
    table
}

const fn scrambler() -> [u8; SCRAMBLER_PERIOD] {
    let mut s = [1u8; SCRAMBLER_PERIOD];
    let mut k = 7;
    while k < SCRAMBLER_PERIOD {
        s[k] = s[k - 1] ^ s[k - 7];
        k += 1;
    }
    s
}

pub(crate) fn scrambling_bit(code: u8, k: usize) -> u8 {
    if code == 0 {
        0
    } else {
        SCRAMBLER[(k + usize::from(code)) % SCRAMBLER_PERIOD]
    }
}

pub(crate) fn interleaved_position(band: TetrapolBand, j: usize) -> usize {
    match band {
        TetrapolBand::Uhf => usize::from(UHF_DATA[j]),
        TetrapolBand::Vhf => 19 * usize::from(BIT_REVERSE[j % 8]) + (3 * (j / 8)) % 19,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    Voice { asb: [u8; 2] },
    Data(DataBlock),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DataBlock {
    pub(crate) flag: u8,
    pub(crate) octets: [u8; BLOCK_BYTES],
    pub(crate) asb: [u8; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decoded {
    pub(crate) block: Block,
    pub(crate) corrected: u32,
}

fn soft_xor(a: f32, b: f32) -> f32 {
    let magnitude = a.abs().min(b.abs());
    if (a < 0.0) == (b < 0.0) {
        magnitude
    } else {
        -magnitude
    }
}

fn flip(soft: f32, bit: u8) -> f32 {
    if bit == 0 { soft } else { -soft }
}

fn uhf_symbols(frame: &[f32; FRAME_BITS], code: u8, out: &mut [f32; PAYLOAD_BITS]) {
    for (j, slot) in out.iter_mut().enumerate() {
        let s = |k: usize| scrambling_bit(code, k);
        let (soft, mask) = if PRECODED[j] {
            (
                soft_xor(frame[SYNC_BITS + j], frame[SYNC_BITS + j - 1]),
                s(j) ^ s(j - 2),
            )
        } else {
            let previous = if j == 0 { 0 } else { s(j - 1) };
            (frame[SYNC_BITS + j], s(j) ^ previous)
        };
        *slot = flip(soft, mask);
    }
}

fn vhf_symbols(
    frame: &[f32; FRAME_BITS],
    code: u8,
    slip: Option<usize>,
    out: &mut [f32; PAYLOAD_BITS],
) {
    let mut level = 0u8;
    for (j, slot) in out.iter_mut().enumerate() {
        let transition = frame[SYNC_BITS + j];
        level ^= u8::from(transition < 0.0);
        let corrected = level ^ u8::from(slip.is_some_and(|at| j >= at));
        *slot = flip(transition.abs(), corrected ^ scrambling_bit(code, j));
    }
}

fn weakest_transitions(frame: &[f32; FRAME_BITS], out: &mut [usize]) {
    for chosen in 0..out.len() {
        let mut best = 0;
        let mut weakest = f32::INFINITY;
        for j in 0..PAYLOAD_BITS {
            let strength = frame[SYNC_BITS + j].abs();
            if strength < weakest && !out[..chosen].contains(&j) {
                best = j;
                weakest = strength;
            }
        }
        out[chosen] = best;
    }
}

fn branch(state: usize, bit: usize) -> (u8, u8, usize) {
    let previous = (state >> 1) as u8 & 1;
    let before = state as u8 & 1;
    let input = bit as u8;
    (
        input ^ previous ^ before,
        input ^ before,
        (bit << 1) | (state >> 1),
    )
}

fn viterbi(coded: &[f32], steps: usize, start: usize, end: usize, bits: &mut [u8]) -> f32 {
    let mut metric = [f32::NEG_INFINITY; STATES];
    metric[start] = 0.0;
    let mut from = [[0u8; STATES]; SECOND_STEPS];
    for step in 0..steps {
        let (c0, c1) = (coded[2 * step], coded[2 * step + 1]);
        let mut next = [f32::NEG_INFINITY; STATES];
        for (state, &m) in metric.iter().enumerate() {
            if m == f32::NEG_INFINITY {
                continue;
            }
            for bit in 0..2 {
                let (o0, o1, target) = branch(state, bit);
                let candidate = m + flip(c0, o0) + flip(c1, o1);
                if candidate > next[target] {
                    next[target] = candidate;
                    from[step][target] = state as u8;
                }
            }
        }
        metric = next;
    }
    let mut state = end;
    for step in (0..steps).rev() {
        bits[step] = (state >> 1) as u8;
        state = usize::from(from[step][state]);
    }
    metric[end]
}

fn tail_biting(coded: &[f32], bits: &mut [u8; FIRST_STEPS]) {
    let mut best = f32::NEG_INFINITY;
    let mut trial = [0u8; FIRST_STEPS];
    for state in 0..STATES {
        let metric = viterbi(coded, FIRST_STEPS, state, state, &mut trial);
        if metric > best {
            best = metric;
            *bits = trial;
        }
    }
}

fn disagreements(coded: &[f32], bits: &[u8], start: usize) -> u32 {
    let mut state = start;
    let mut errors = 0;
    for (step, &bit) in bits.iter().enumerate() {
        let (o0, o1, next) = branch(state, usize::from(bit));
        errors += u32::from((coded[2 * step] < 0.0) != (o0 == 1));
        errors += u32::from((coded[2 * step + 1] < 0.0) != (o1 == 1));
        state = next;
    }
    errors
}

fn divisible(bits: &[u8], generator: u32) -> bool {
    let degree = 31 - generator.leading_zeros();
    let mask = (1u32 << degree) - 1;
    let mut register = 0u32;
    for &bit in bits {
        let top = (register >> (degree - 1)) & 1;
        register = ((register << 1) | u32::from(bit)) & mask;
        if top == 1 {
            register ^= generator & mask;
        }
    }
    register == 0
}

fn tail_start(first: &[u8; FIRST_STEPS]) -> usize {
    usize::from(first[25]) << 1 | usize::from(first[24])
}

const SLIP_CANDIDATES: usize = 3;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) voice: u32,
    pub(crate) first: u32,
    pub(crate) total: u32,
}

pub(crate) struct Decoder {
    symbols: [f32; PAYLOAD_BITS],
    deinterleaved: [f32; PAYLOAD_BITS],
    weakest: [usize; SLIP_CANDIDATES],
}

impl Decoder {
    pub(crate) fn new() -> Self {
        Self {
            symbols: [0.0; PAYLOAD_BITS],
            deinterleaved: [0.0; PAYLOAD_BITS],
            weakest: [0; SLIP_CANDIDATES],
        }
    }

    pub(crate) fn prepare(&mut self, frame: &[f32; FRAME_BITS], band: TetrapolBand) {
        if band == TetrapolBand::Vhf {
            weakest_transitions(frame, &mut self.weakest);
        }
    }

    pub(crate) fn frame(
        &mut self,
        frame: &[f32; FRAME_BITS],
        band: TetrapolBand,
        code: u8,
        limits: Limits,
    ) -> Option<Decoded> {
        if band == TetrapolBand::Uhf {
            uhf_symbols(frame, code, &mut self.symbols);
            return self.decode(band, limits);
        }
        let weakest = self.weakest;
        std::iter::once(None)
            .chain(weakest.into_iter().map(Some))
            .find_map(|slip| {
                vhf_symbols(frame, code, slip, &mut self.symbols);
                self.decode(band, limits)
            })
    }

    fn decode(&mut self, band: TetrapolBand, limits: Limits) -> Option<Decoded> {
        let mut first = [0u8; FIRST_STEPS];
        self.gather(band, 0..FIRST_CODED);
        tail_biting(&self.deinterleaved[..FIRST_CODED], &mut first);
        let first_errors = disagreements(
            &self.deinterleaved[..FIRST_CODED],
            &first,
            tail_start(&first),
        );
        if first[0] == 0 {
            return (first_errors <= limits.voice)
                .then(|| voice(&first, first_errors))
                .flatten();
        }
        if first_errors > limits.first {
            return None;
        }
        self.gather(band, FIRST_CODED..PAYLOAD_BITS);
        let mut second = [0u8; SECOND_STEPS];
        viterbi(
            &self.deinterleaved[FIRST_CODED..],
            SECOND_STEPS,
            0,
            0,
            &mut second,
        );
        let errors = first_errors + disagreements(&self.deinterleaved[FIRST_CODED..], &second, 0);
        if errors > limits.total {
            return None;
        }
        data(&first, &second, errors)
    }

    fn gather(&mut self, band: TetrapolBand, range: std::ops::Range<usize>) {
        for j in range {
            self.deinterleaved[j] = self.symbols[interleaved_position(band, j)];
        }
    }
}

fn voice(first: &[u8; FIRST_STEPS], corrected: u32) -> Option<Decoded> {
    let mut checked = *first;
    for bit in &mut checked[23..] {
        *bit ^= 1;
    }
    divisible(&checked, VOICE_CRC).then_some(Decoded {
        block: Block::Voice {
            asb: [first[21], first[22]],
        },
        corrected,
    })
}

fn data(first: &[u8; FIRST_STEPS], second: &[u8; SECOND_STEPS], corrected: u32) -> Option<Decoded> {
    let mut field = [0u8; DATA_FIELD_BITS];
    field[..FIRST_STEPS].copy_from_slice(first);
    field[FIRST_STEPS..].copy_from_slice(&second[..DATA_FIELD_BITS - FIRST_STEPS]);
    if !divisible(&field, DATA_CRC) {
        return None;
    }
    let mut octets = [0u8; BLOCK_BYTES];
    for (k, &bit) in field[3..67].iter().enumerate() {
        octets[k / 8] |= bit << (k % 8);
    }
    Some(Decoded {
        block: Block::Data(DataBlock {
            flag: field[1] | field[2] << 1,
            octets,
            asb: [field[67], field[68]],
        }),
        corrected,
    })
}
