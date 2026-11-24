use sdrmm_dsp::coding::fec::ldpc::{MinSum, Osd, ParityCheck, Schedule};

use super::tables::{CHECKS_74, CHECKS_101, GENERATOR_74, GENERATOR_101};

pub(super) const N: usize = 240;
const CRC_BITS: usize = 24;
const CRC_POLY: u32 = 0x100_065B;
const CRC_SPAN: u32 = (1 << (CRC_BITS + 1)) - 1;
const MIN_SUM_SCALE: f32 = 0.8;
const ITERATIONS: usize = 30;
const OSD_PASSES: usize = 2;
const STALL_AFTER: usize = 10;
const STALL_COUNT: usize = 5;
const STALL_CHECKS: usize = 15;

/// What a codeword carries: FST4's 77-bit message or FST4W's 50-bit one, each behind CRC-24.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Payload {
    Standard,
    Wspr,
}

impl Payload {
    pub(super) const fn payload_bits(self) -> usize {
        match self {
            Self::Standard => 77,
            Self::Wspr => 50,
        }
    }

    pub(super) const fn message_bits(self) -> usize {
        self.payload_bits() + CRC_BITS
    }

    /// How many information bits ordered statistics flips at once: the 50-bit beacon has fewer
    /// to search, so it can afford a deeper search for its weaker signals.
    const fn osd_order(self) -> usize {
        match self {
            Self::Standard => 2,
            Self::Wspr => 3,
        }
    }

    /// The information bits ordered-statistics decoding re-encodes: the payload and the CRC
    /// bits left free for error detection, as WSJT-X's `Keff`.
    const fn osd_dimension(self) -> usize {
        match self {
            Self::Standard => 91,
            Self::Wspr => 66,
        }
    }

    const fn checks(self) -> &'static [[u16; 3]; N] {
        match self {
            Self::Standard => &CHECKS_101,
            Self::Wspr => &CHECKS_74,
        }
    }

    const fn generator(self) -> &'static [u128] {
        match self {
            Self::Standard => &GENERATOR_101,
            Self::Wspr => &GENERATOR_74,
        }
    }
}

/// The remainder of `bits` divided by the CRC-24 polynomial 0x100065B, as WSJT-X's `get_crc24`:
/// zero when `bits` ends in a CRC that matches what precedes it.
pub(super) fn crc24(bits: &[bool]) -> u32 {
    if bits.len() <= CRC_BITS {
        return 0;
    }
    let mut register = bits[..=CRC_BITS]
        .iter()
        .fold(0u32, |acc, &bit| (acc << 1) | u32::from(bit));
    for index in 0..bits.len() - CRC_BITS {
        register = (register & !1) | u32::from(bits[index + CRC_BITS]);
        if register >> CRC_BITS & 1 == 1 {
            register ^= CRC_POLY;
        }
        register = (register << 1) & CRC_SPAN;
    }
    register >> 1
}

/// The payload followed by its CRC-24, the message the LDPC code protects.
pub(super) fn with_crc(payload: &[bool]) -> Vec<bool> {
    let mut message = payload.to_vec();
    message.resize(payload.len() + CRC_BITS, false);
    let crc = crc24(&message);
    for (index, bit) in message[payload.len()..].iter_mut().enumerate() {
        *bit = crc >> (CRC_BITS - 1 - index) & 1 == 1;
    }
    message
}

fn encode_message(payload: Payload, message: &[bool]) -> [bool; N] {
    let bits = payload.message_bits();
    let packed = message
        .iter()
        .take(bits)
        .fold(0u128, |acc, &bit| (acc << 1) | u128::from(bit));
    let mut codeword = [false; N];
    codeword[..bits].copy_from_slice(&message[..bits]);
    for (slot, row) in codeword[bits..].iter_mut().zip(payload.generator()) {
        *slot = (row & packed).count_ones() % 2 == 1;
    }
    codeword
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Decoded {
    pub(super) message: Vec<bool>,
    pub(super) codeword: [bool; N],
    pub(super) hard_errors: u32,
    pub(super) osd: bool,
}

pub(super) struct Code {
    payload: Payload,
    min_sum: MinSum,
    osd: Osd,
}

impl Code {
    pub(super) fn new(payload: Payload) -> Self {
        Self {
            payload,
            min_sum: MinSum::new(
                ParityCheck::from_one_based_columns(N - payload.message_bits(), payload.checks()),
                Schedule::Layered,
                MIN_SUM_SCALE,
            ),
            osd: Osd::new(N, &osd_basis(payload)),
        }
    }

    pub(super) fn encode(&self, payload: &[bool]) -> [bool; N] {
        encode_message(self.payload, &with_crc(payload))
    }

    /// Decodes `llr`, positive for a 1 bit as WSJT-X writes them: belief propagation first,
    /// then ordered statistics on its early soft outputs.
    pub(super) fn decode(&mut self, llr: &[f32; N]) -> Option<Decoded> {
        let channel = llr.map(|value| -value);
        let mut saved = Vec::with_capacity(OSD_PASSES);
        if let Some(codeword) = self.propagate(&channel, &mut saved) {
            return self.accept(llr, codeword, false);
        }
        saved.iter().find_map(|soft| {
            let word = self.osd.decode(soft, self.payload.osd_order())?;
            let codeword: [bool; N] = word.bits.try_into().ok()?;
            self.accept(llr, codeword, true)
        })
    }

    fn propagate(&mut self, channel: &[f32; N], saved: &mut Vec<[f32; N]>) -> Option<[bool; N]> {
        let mut unsatisfied = self.min_sum.load(channel);
        let mut sum = *channel;
        let mut last = unsatisfied;
        let mut stalled = 0;
        for iteration in 0..=ITERATIONS {
            if unsatisfied == 0 {
                let hard = self.min_sum.hard();
                let codeword: [bool; N] = std::array::from_fn(|bit| hard[bit]);
                if crc24(&codeword[..self.payload.message_bits()]) == 0 {
                    return Some(codeword);
                }
            }
            if iteration > 0 {
                stalled = if unsatisfied < last { 0 } else { stalled + 1 };
                if stalled >= STALL_COUNT && iteration >= STALL_AFTER && unsatisfied > STALL_CHECKS
                {
                    return None;
                }
            }
            last = unsatisfied;
            if iteration == ITERATIONS {
                break;
            }
            unsatisfied = self.min_sum.iterate(channel);
            for (total, &value) in sum.iter_mut().zip(self.min_sum.totals()) {
                *total += value;
            }
            if saved.len() < OSD_PASSES {
                saved.push(sum);
            }
        }
        None
    }

    fn accept(&self, llr: &[f32; N], codeword: [bool; N], osd: bool) -> Option<Decoded> {
        let bits = self.payload.message_bits();
        if crc24(&codeword[..bits]) != 0 || !codeword.contains(&true) {
            return None;
        }
        let hard_errors = codeword
            .iter()
            .zip(llr)
            .filter(|&(&bit, &value)| bit != (value >= 0.0))
            .count() as u32;
        Some(Decoded {
            message: codeword[..self.payload.payload_bits()].to_vec(),
            codeword,
            hard_errors,
            osd,
        })
    }
}

/// The codewords of single information bits under `Keff`: each payload bit carries the CRC
/// bits past the detection span with it, each free CRC bit stands alone.
fn osd_basis(payload: Payload) -> Vec<Vec<bool>> {
    let bits = payload.message_bits();
    let free = payload.osd_dimension();
    (0..free)
        .map(|index| {
            let mut message = vec![false; bits];
            if index < payload.payload_bits() {
                message[index] = true;
                message = with_crc(&message[..payload.payload_bits()]);
                message[payload.payload_bits()..free].fill(false);
            } else {
                message[index] = true;
            }
            encode_message(payload, &message).to_vec()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pseudo_random(bits: usize, mut state: u64) -> Vec<bool> {
        (0..bits)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                state >> 63 == 1
            })
            .collect()
    }

    fn llr_for(codeword: &[bool; N], flips: &[usize]) -> [f32; N] {
        std::array::from_fn(|bit| {
            let value = if codeword[bit] { 2.0 } else { -2.0 };
            if flips.contains(&bit) {
                -0.3 * value
            } else {
                value
            }
        })
    }

    #[test]
    fn the_crc_matches_wsjtx_and_checks_to_zero() {
        let mut bits = vec![false; 50];
        bits[0] = true;
        let message = with_crc(&bits);
        assert_eq!(crc24(&message), 0);
        let crc = message[50..]
            .iter()
            .fold(0u32, |acc, &bit| (acc << 1) | u32::from(bit));
        assert_eq!(crc, crc_of_leading_one(74));
        let mut damaged = message;
        damaged[7] ^= true;
        assert_ne!(crc24(&damaged), 0);
    }

    /// x^(len-1) mod P, the remainder a lone leading one leaves.
    fn crc_of_leading_one(len: usize) -> u32 {
        let mut register = 1u64;
        for _ in 0..len - 1 {
            register <<= 1;
            if register >> CRC_BITS & 1 == 1 {
                register ^= u64::from(CRC_POLY);
            }
        }
        register as u32
    }

    #[test]
    fn every_codeword_satisfies_every_check() {
        for payload in [Payload::Standard, Payload::Wspr] {
            let code = Code::new(payload);
            let parity =
                ParityCheck::from_one_based_columns(N - payload.message_bits(), payload.checks());
            for seed in 1..6 {
                let codeword = code.encode(&pseudo_random(payload.payload_bits(), seed));
                assert!(parity.satisfies(&codeword), "{payload:?} seed {seed}");
            }
            for row in osd_basis(payload) {
                assert!(parity.satisfies(&row), "{payload:?} basis");
            }
        }
    }

    #[test]
    fn belief_propagation_repairs_scattered_errors() {
        for payload in [Payload::Standard, Payload::Wspr] {
            let mut code = Code::new(payload);
            let bits = pseudo_random(payload.payload_bits(), 11);
            let codeword = code.encode(&bits);
            let decoded = code
                .decode(&llr_for(&codeword, &[3, 40, 77, 150, 201, 239]))
                .expect("decodes");
            assert_eq!(decoded.message, bits);
            assert_eq!(decoded.hard_errors, 6);
            assert!(!decoded.osd);
        }
    }

    #[test]
    fn a_word_of_noise_is_refused() {
        let mut code = Code::new(Payload::Standard);
        let noise = pseudo_random(N, 99);
        let llr: [f32; N] = std::array::from_fn(|bit| if noise[bit] { 0.4 } else { -0.4 });
        assert!(code.decode(&llr).is_none());
    }
}
