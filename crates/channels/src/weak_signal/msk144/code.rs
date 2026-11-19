#[cfg(any(test, feature = "synth"))]
use std::sync::LazyLock;

use sdrmm_dsp::coding::fec::crc_msb_bits;
#[cfg(any(test, feature = "synth"))]
use sdrmm_dsp::coding::fec::ldpc::Systematic;
use sdrmm_dsp::coding::fec::ldpc::{MinSum, ParityCheck, Schedule};

use super::super::ftx::message::PAYLOAD_BITS;

pub(super) const N: usize = 128;
const K: usize = 90;
const M: usize = N - K;
const PAYLOAD: usize = PAYLOAD_BITS as usize;
const CRC_POLY: u32 = 0x15D7;
const CRC_BITS: u32 = 13;
/// WSJT-X runs its augmented CRC over the payload padded to twelve bytes; the bit-serial form
/// reaches the same remainder after six zero bits.
const CRC_PADDING: usize = 6;
const MIN_SUM_SCALE: f32 = 0.8;
const ITERATIONS: usize = 30;

#[cfg(any(test, feature = "synth"))]
static GENERATOR: LazyLock<Option<Systematic>> =
    LazyLock::new(|| Systematic::from_hex(K, &GENERATOR_ROWS));

pub(super) fn crc13(payload: u128) -> u16 {
    let bits = (0..PAYLOAD)
        .map(|index| (payload >> (PAYLOAD - 1 - index)) & 1 == 1)
        .chain(std::iter::repeat_n(false, CRC_PADDING));
    crc_msb_bits(CRC_POLY, CRC_BITS, 0, bits) as u16
}

#[cfg(any(test, feature = "synth"))]
/// The 128-bit codeword for a 77-bit payload: payload, CRC-13, then 38 parity bits.
pub(super) fn encode(payload: u128) -> Option<[bool; N]> {
    let message = (payload << CRC_BITS) | u128::from(crc13(payload));
    let bits: [bool; K] = std::array::from_fn(|index| (message >> (K - 1 - index)) & 1 == 1);
    let mut codeword = [false; N];
    GENERATOR.as_ref()?.encode(&bits, &mut codeword);
    Some(codeword)
}

fn payload_of(codeword: &[bool]) -> Option<u128> {
    let message = codeword[..K]
        .iter()
        .fold(0u128, |acc, &bit| (acc << 1) | u128::from(bit));
    let payload = message >> CRC_BITS;
    (u128::from(crc13(payload)) == message & ((1 << CRC_BITS) - 1)).then_some(payload)
}

pub(super) struct Decoded {
    pub(super) payload: u128,
    pub(super) hard_errors: u32,
}

pub(super) struct Decoder {
    min_sum: MinSum,
    negated: [f32; N],
}

impl Decoder {
    pub(super) fn new() -> Self {
        Self {
            min_sum: MinSum::new(
                ParityCheck::from_one_based_columns(M, &COLUMN_CHECKS),
                Schedule::Layered,
                MIN_SUM_SCALE,
            ),
            negated: [0.0; N],
        }
    }

    /// Decodes log-likelihood ratios that are positive for a one bit, as WSJT-X writes them.
    pub(super) fn decode(&mut self, llr: &[f32; N]) -> Option<Decoded> {
        for (negated, &value) in self.negated.iter_mut().zip(llr) {
            *negated = -value;
        }
        self.min_sum.decode(&self.negated, ITERATIONS)?;
        let hard = self.min_sum.hard();
        let payload = payload_of(hard)?;
        let hard_errors = hard
            .iter()
            .zip(llr)
            .filter(|&(&bit, &value)| bit != (value > 0.0))
            .count() as u32;
        Some(Decoded {
            payload,
            hard_errors,
        })
    }
}

const COLUMN_CHECKS: [[u16; 3]; N] = [
    [21, 34, 36],
    [1, 8, 28],
    [2, 9, 37],
    [3, 7, 19],
    [4, 16, 32],
    [2, 5, 22],
    [6, 13, 25],
    [10, 31, 33],
    [11, 24, 27],
    [12, 15, 23],
    [14, 18, 26],
    [17, 20, 29],
    [17, 30, 34],
    [6, 34, 35],
    [1, 10, 30],
    [3, 18, 23],
    [4, 12, 25],
    [5, 28, 36],
    [7, 14, 21],
    [8, 15, 31],
    [9, 27, 32],
    [11, 19, 35],
    [13, 16, 37],
    [20, 24, 38],
    [21, 22, 26],
    [12, 29, 33],
    [1, 17, 35],
    [2, 28, 30],
    [3, 10, 32],
    [4, 8, 36],
    [5, 19, 29],
    [6, 20, 27],
    [7, 22, 37],
    [9, 11, 33],
    [13, 24, 26],
    [14, 31, 34],
    [15, 16, 25],
    [13, 18, 38],
    [8, 20, 23],
    [1, 32, 33],
    [2, 17, 19],
    [3, 24, 34],
    [4, 7, 38],
    [5, 11, 31],
    [6, 18, 21],
    [9, 15, 36],
    [10, 16, 28],
    [12, 26, 30],
    [14, 27, 29],
    [22, 25, 35],
    [23, 30, 32],
    [4, 11, 37],
    [1, 14, 23],
    [2, 8, 25],
    [3, 13, 27],
    [5, 10, 37],
    [6, 16, 31],
    [7, 15, 18],
    [9, 22, 24],
    [12, 19, 36],
    [17, 26, 38],
    [20, 21, 33],
    [20, 28, 35],
    [4, 29, 34],
    [1, 26, 36],
    [2, 23, 34],
    [3, 9, 38],
    [5, 6, 17],
    [7, 27, 35],
    [8, 14, 32],
    [10, 15, 22],
    [11, 18, 29],
    [12, 13, 28],
    [16, 19, 33],
    [21, 25, 31],
    [24, 30, 37],
    [1, 3, 21],
    [2, 18, 31],
    [4, 6, 9],
    [5, 8, 33],
    [7, 29, 32],
    [10, 13, 19],
    [11, 22, 23],
    [12, 27, 34],
    [14, 15, 30],
    [16, 27, 38],
    [17, 28, 37],
    [20, 25, 26],
    [5, 24, 35],
    [3, 6, 36],
    [1, 12, 31],
    [2, 4, 33],
    [3, 16, 30],
    [1, 2, 24],
    [5, 23, 27],
    [6, 28, 32],
    [7, 17, 36],
    [8, 22, 38],
    [9, 18, 20],
    [10, 21, 29],
    [11, 13, 34],
    [4, 14, 20],
    [11, 30, 38],
    [14, 35, 37],
    [15, 19, 26],
    [3, 28, 29],
    [7, 8, 9],
    [5, 18, 34],
    [13, 15, 17],
    [12, 16, 35],
    [10, 23, 25],
    [19, 21, 37],
    [17, 27, 31],
    [24, 25, 36],
    [1, 18, 19],
    [6, 26, 33],
    [22, 31, 32],
    [3, 20, 22],
    [4, 21, 27],
    [2, 13, 29],
    [6, 7, 12],
    [15, 24, 32],
    [9, 25, 30],
    [23, 37, 38],
    [5, 16, 26],
    [11, 14, 28],
    [33, 36, 38],
    [8, 10, 35],
];

#[cfg(any(test, feature = "synth"))]
const GENERATOR_ROWS: [&str; M] = [
    "a08ea80879050a5e94da994",
    "59f3b48040ca089c81ee880",
    "e4070262802e31b7b17d3dc",
    "95cbcbaf032dc3d960bacc8",
    "c4d79b5dcc21161a254ffbc",
    "93fde9cdbf2622a70868424",
    "e73b888bb1b01167379ba28",
    "45a0d0a0f39a7ad2439949c",
    "759acef19444bcad79c4964",
    "71eb4dddf4f5ed9e2ea17e0",
    "80f0ad76fb247d6b4ca8d38",
    "184fff3aa1b82dc66640104",
    "ca4e320bb382ed14cbb1094",
    "52514447b90e25b9e459e28",
    "dd10c1666e071956bd0df38",
    "99c332a0b792a2da8ef1ba8",
    "7bd9f688e7ed402e231aaac",
    "00fcad76eb647d6a0ca8c38",
    "6ac8d0499c43b02eed78d70",
    "2c2c764baf795b4788db010",
    "0e907bf9e280d2624823dd0",
    "b857a6e315afd8c1c925e64",
    "8deb58e22d73a141cae3778",
    "22d3cb80d92d6ac132dfe08",
    "754763877b28c187746855c",
    "1d1bb7cf6953732e04ebca4",
    "2c65e0ea4466ab9f5e1deec",
    "6dc530ca37fc916d1f84870",
    "49bccbbee152355be7ac984",
    "e8387f3f4367cf45a150448",
    "8ce25e03d67d51091c81884",
    "b798012ffa40a93852752c8",
    "2e43307933adfca37adc3c8",
    "ca06e0a42ca1ec782d6c06c",
    "c02b762927556a7039e638c",
    "4a3e9b7d08b6807f8619fac",
    "45e8030f68997bb68544424",
    "7e79362c16773efc6482e30",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn llr_for(codeword: &[bool; N], flips: &[usize]) -> [f32; N] {
        std::array::from_fn(|bit| {
            let one = codeword[bit] != flips.contains(&bit);
            if one { 4.0 } else { -4.0 }
        })
    }

    #[test]
    fn every_codeword_satisfies_every_check() {
        let code = ParityCheck::from_one_based_columns(M, &COLUMN_CHECKS);
        for payload in [0u128, 1, 0x1234_5678_9abc_def0_1234, (1u128 << 77) - 1] {
            let codeword = encode(payload).expect("generator rows parse");
            assert!(code.satisfies(&codeword), "{payload:x}");
            assert_eq!(payload_of(&codeword), Some(payload));
        }
    }

    #[test]
    fn belief_propagation_repairs_scattered_errors() {
        let payload = 0x0abc_def0_1234_5678_9abc;
        let codeword = encode(payload).expect("generator rows parse");
        let decoded = Decoder::new()
            .decode(&llr_for(&codeword, &[3, 40, 77, 100, 127]))
            .expect("decodes");
        assert_eq!(decoded.payload, payload);
        assert_eq!(decoded.hard_errors, 5);
    }

    #[test]
    fn a_broken_crc_is_refused() {
        let mut codeword = encode(0x55).expect("generator rows parse");
        codeword[80] = !codeword[80];
        assert!(payload_of(&codeword).is_none());
    }
}
