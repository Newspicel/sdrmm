use std::sync::LazyLock;

use sdrmm_modem::framesync::ToneArray;

use super::super::ftx::{Code, LDPC_N as N, Protocol};

const CHECKS: usize = 87;
const MESSAGE_BITS: usize = 87;
const CRC_BITS: u32 = 12;
const CRC_POLY: u16 = 0xC06;
const CRC_MASK: u16 = (1 << CRC_BITS) - 1;
const CRC_XOR: u16 = 42;
const CRC_STREAM_BITS: usize = 88;
pub(crate) const PAYLOAD_BITS: usize = 75;

/// The checks each codeword bit takes part in, in symbol order: 87 parity bits, then the 87
/// message bits. From `JS8Call`'s `JS8.cpp`.
const COLUMN_CHECKS: [[u8; 3]; N] = [
    [0, 24, 68],
    [1, 4, 72],
    [2, 31, 67],
    [3, 50, 60],
    [5, 62, 69],
    [6, 32, 78],
    [7, 49, 85],
    [8, 36, 42],
    [9, 40, 64],
    [10, 13, 63],
    [11, 74, 76],
    [12, 22, 80],
    [14, 15, 81],
    [16, 55, 65],
    [17, 52, 59],
    [18, 30, 51],
    [19, 66, 83],
    [20, 28, 71],
    [21, 23, 43],
    [25, 34, 75],
    [26, 35, 37],
    [27, 39, 41],
    [29, 53, 54],
    [33, 48, 86],
    [38, 56, 57],
    [44, 73, 82],
    [45, 61, 79],
    [46, 47, 84],
    [58, 70, 77],
    [0, 49, 52],
    [1, 46, 83],
    [2, 24, 78],
    [3, 5, 13],
    [4, 6, 79],
    [7, 33, 54],
    [8, 35, 68],
    [9, 42, 82],
    [10, 22, 73],
    [11, 16, 43],
    [12, 56, 75],
    [14, 26, 55],
    [15, 27, 28],
    [17, 18, 58],
    [19, 39, 62],
    [20, 34, 51],
    [21, 53, 63],
    [23, 61, 77],
    [25, 31, 76],
    [29, 71, 84],
    [30, 64, 86],
    [32, 38, 50],
    [36, 47, 74],
    [37, 69, 70],
    [40, 41, 67],
    [44, 66, 85],
    [45, 80, 81],
    [48, 65, 72],
    [57, 59, 65],
    [60, 64, 84],
    [0, 13, 20],
    [1, 12, 58],
    [2, 66, 81],
    [3, 31, 72],
    [4, 35, 53],
    [5, 42, 45],
    [6, 27, 74],
    [7, 32, 70],
    [8, 48, 75],
    [9, 57, 63],
    [10, 47, 67],
    [11, 18, 44],
    [14, 49, 60],
    [15, 21, 25],
    [16, 71, 79],
    [17, 39, 54],
    [19, 34, 50],
    [22, 24, 33],
    [23, 62, 86],
    [26, 38, 73],
    [28, 77, 82],
    [29, 69, 76],
    [30, 68, 83],
    [21, 36, 85],
    [37, 40, 80],
    [41, 43, 56],
    [46, 52, 61],
    [51, 55, 78],
    [59, 74, 80],
    [0, 38, 76],
    [1, 15, 40],
    [2, 30, 53],
    [3, 35, 77],
    [4, 44, 64],
    [5, 56, 84],
    [6, 13, 48],
    [7, 20, 45],
    [8, 14, 71],
    [9, 19, 61],
    [10, 16, 70],
    [11, 33, 46],
    [12, 67, 85],
    [17, 22, 42],
    [18, 63, 72],
    [23, 47, 78],
    [24, 69, 82],
    [25, 79, 86],
    [26, 31, 39],
    [27, 55, 68],
    [28, 62, 65],
    [29, 41, 49],
    [32, 36, 81],
    [34, 59, 73],
    [37, 54, 83],
    [43, 51, 60],
    [50, 52, 71],
    [57, 58, 66],
    [46, 55, 75],
    [0, 18, 36],
    [1, 60, 74],
    [2, 7, 65],
    [3, 59, 83],
    [4, 33, 38],
    [5, 25, 52],
    [6, 31, 56],
    [8, 51, 66],
    [9, 11, 14],
    [10, 50, 68],
    [12, 13, 64],
    [15, 30, 42],
    [16, 19, 35],
    [17, 79, 85],
    [20, 47, 58],
    [21, 39, 45],
    [22, 32, 61],
    [23, 29, 73],
    [24, 41, 63],
    [26, 48, 84],
    [27, 37, 72],
    [28, 43, 80],
    [34, 67, 69],
    [40, 62, 75],
    [44, 48, 70],
    [49, 57, 86],
    [47, 53, 82],
    [12, 54, 78],
    [76, 77, 81],
    [0, 1, 23],
    [2, 5, 74],
    [3, 55, 86],
    [4, 43, 52],
    [6, 49, 82],
    [7, 9, 27],
    [8, 54, 61],
    [10, 28, 66],
    [11, 32, 39],
    [13, 15, 19],
    [14, 34, 72],
    [16, 30, 38],
    [17, 35, 56],
    [18, 45, 75],
    [20, 41, 83],
    [21, 33, 58],
    [22, 25, 60],
    [24, 59, 64],
    [26, 63, 79],
    [29, 36, 65],
    [31, 44, 71],
    [37, 50, 85],
    [40, 76, 78],
    [42, 55, 67],
    [46, 73, 81],
    [39, 51, 77],
    [53, 60, 70],
    [45, 57, 68],
];

const ORIGINAL: &[u8] = &[4, 2, 5, 6, 1, 3, 0];
const MODIFIED: [&[u8]; 3] = [
    &[0, 6, 2, 3, 5, 4, 1],
    &[1, 5, 0, 2, 3, 6, 4],
    &[2, 5, 0, 6, 4, 1, 3],
];
const IDENTITY: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
/// Plain continuous-phase FSK: a Gaussian filter this wide leaves the tones square.
const SQUARE_BT: f32 = 64.0;

/// JS8 Normal: 6.25 baud in a 15-second period.
pub(crate) const NORMAL: Protocol = Protocol {
    symbol_samples: 1_920,
    tones: 8,
    bits_per_symbol: 3,
    symbols: 79,
    sync: ToneArray::new(&[(0, ORIGINAL), (36, ORIGINAL), (72, ORIGINAL)]),
    gray: IDENTITY,
    scramble: 0,
    bt: SQUARE_BT,
    ramp_symbols: 0,
    slot_samples: 180_000,
    fft_samples: 192_000,
    earliest_start_s: -2.0,
    latest_start_s: 2.9,
    coherent_spans: &[1, 2, 3],
    snr_offset_db: -27.0,
    min_sync: 1.3,
    coherent_sync: false,
    full_time_search: false,
    spectral_candidates: true,
    osd_min_presence: 0.0,
    osd_max_snr_db: f32::INFINITY,
    assume_cq: false,
    code: &Js8Code,
};

/// JS8 Slow: 3.125 baud in a 30-second period. Its window is searched every other hop, so the
/// start search spans more than two hops.
pub(crate) const SLOW: Protocol = Protocol {
    symbol_samples: 3_840,
    sync: ToneArray::new(&[(0, MODIFIED[0]), (36, MODIFIED[1]), (72, MODIFIED[2])]),
    slot_samples: 360_000,
    fft_samples: 384_000,
    latest_start_s: 4.9,
    snr_offset_db: -30.0,
    ..NORMAL
};

/// The (174, 87) LDPC code JS8 sends, guarding 75 payload bits with a 12-bit CRC.
pub(crate) struct Js8Code;

static PARITY: LazyLock<[u128; CHECKS]> = LazyLock::new(parity_rows);

impl Code for Js8Code {
    fn column_checks(&self) -> &[[u8; 3]; N] {
        &COLUMN_CHECKS
    }

    fn checks(&self) -> usize {
        CHECKS
    }

    fn encode(&self, payload: u128) -> [u8; N] {
        let message = with_crc(payload);
        let mut bits = [0u8; N];
        for (bit, row) in bits[..CHECKS].iter_mut().zip(PARITY.iter()) {
            *bit = ((row & message).count_ones() & 1) as u8;
        }
        for (index, bit) in bits[CHECKS..].iter_mut().enumerate() {
            *bit = (message >> (MESSAGE_BITS - 1 - index)) as u8 & 1;
        }
        bits
    }

    fn payload_of(&self, codeword: &[u8; N]) -> Option<u128> {
        let message = codeword[CHECKS..]
            .iter()
            .fold(0u128, |acc, &bit| (acc << 1) | u128::from(bit));
        let payload = message >> CRC_BITS;
        (crc12(payload) == (message as u16 & CRC_MASK)).then_some(payload)
    }

    fn ordered_statistics(&self) -> bool {
        false
    }
}

fn with_crc(payload: u128) -> u128 {
    (payload << CRC_BITS) | u128::from(crc12(payload))
}

/// Boost's augmented CRC-12 (polynomial 0xC06) over the payload and thirteen zero bits, as `JS8Call`
/// packs them into eleven bytes, `XORed` with 42.
pub(crate) fn crc12(payload: u128) -> u16 {
    let mut register = 0u16;
    for index in 0..CRC_STREAM_BITS {
        let bit = if index < PAYLOAD_BITS {
            (payload >> (PAYLOAD_BITS - 1 - index)) as u16 & 1
        } else {
            0
        };
        let carry = register >> (CRC_BITS - 1) & 1;
        register = ((register << 1) | bit) & CRC_MASK;
        if carry == 1 {
            register ^= CRC_POLY;
        }
    }
    register ^ CRC_XOR
}

/// Each parity bit as the message bits it sums, found by solving the parity checks for the
/// parity half of the codeword.
fn parity_rows() -> [u128; CHECKS] {
    let mut left = [0u128; CHECKS];
    let mut right = [0u128; CHECKS];
    for (bit, rows) in COLUMN_CHECKS.iter().enumerate() {
        for &row in rows {
            let row = usize::from(row);
            if bit < CHECKS {
                left[row] |= 1u128 << bit;
            } else {
                right[row] |= 1u128 << (MESSAGE_BITS - 1 - (bit - CHECKS));
            }
        }
    }
    for pivot in 0..CHECKS {
        let Some(found) = (pivot..CHECKS).find(|&row| left[row] >> pivot & 1 == 1) else {
            continue;
        };
        left.swap(pivot, found);
        right.swap(pivot, found);
        for row in 0..CHECKS {
            if row != pivot && left[row] >> pivot & 1 == 1 {
                left[row] ^= left[pivot];
                right[row] ^= right[pivot];
            }
        }
    }
    right
}

#[cfg(test)]
mod tests {
    use super::super::super::ftx::{LdpcDecoder, parity_check};
    use super::*;

    #[test]
    fn the_parity_half_of_the_checks_is_solvable() {
        let mut left = [0u128; CHECKS];
        for (bit, rows) in COLUMN_CHECKS.iter().enumerate().take(CHECKS) {
            for &row in rows {
                left[usize::from(row)] |= 1u128 << bit;
            }
        }
        let mut rank = 0;
        for pivot in 0..CHECKS {
            let Some(found) = (rank..CHECKS).find(|&row| left[row] >> pivot & 1 == 1) else {
                continue;
            };
            left.swap(rank, found);
            for row in 0..CHECKS {
                if row != rank && left[row] >> pivot & 1 == 1 {
                    left[row] ^= left[rank];
                }
            }
            rank += 1;
        }
        assert_eq!(rank, CHECKS);
    }

    #[test]
    fn every_codeword_satisfies_every_check_and_reads_back() {
        for payload in [1u128, 0x0123_4567_89ab_cdef_0123, (1 << PAYLOAD_BITS) - 1] {
            let codeword = Js8Code.encode(payload);
            assert!(parity_check(&Js8Code).satisfies(&codeword.map(|bit| bit == 1)));
            assert_eq!(Js8Code.payload_of(&codeword), Some(payload));
        }
    }

    #[test]
    fn a_flipped_bit_breaks_the_crc() {
        let codeword = Js8Code.encode(0x05a5_a5a5_a5a5_a5a5_a5a5);
        for bit in [CHECKS, CHECKS + 40, N - 1] {
            let mut damaged = codeword;
            damaged[bit] ^= 1;
            assert_eq!(Js8Code.payload_of(&damaged), None, "bit {bit}");
        }
    }

    #[test]
    fn the_crc_matches_bit_by_bit_long_division() {
        let payload = 0x4_2a5c_3e17_9bd0_f1a2u128 & ((1 << PAYLOAD_BITS) - 1);
        let mut stream = payload << (CRC_STREAM_BITS - PAYLOAD_BITS);
        let divisor =
            (u128::from(CRC_POLY) | 1 << CRC_BITS) << (CRC_STREAM_BITS - 1 - CRC_BITS as usize);
        for shift in (0..CRC_STREAM_BITS - CRC_BITS as usize).rev() {
            if stream >> (shift + CRC_BITS as usize) & 1 == 1 {
                stream ^= divisor >> (CRC_STREAM_BITS - 1 - CRC_BITS as usize - shift);
            }
        }
        assert_eq!(crc12(payload), stream as u16 ^ CRC_XOR);
    }

    #[test]
    fn belief_propagation_repairs_a_noisy_frame() {
        let payload = 0x7_0f0f_1234_cafe_beefu128 & ((1 << PAYLOAD_BITS) - 1);
        let codeword = Js8Code.encode(payload);
        let mut llr: [f32; N] = codeword.map(|bit| if bit == 1 { 3.0 } else { -3.0 });
        for bit in [2, 30, 77, 100, 140, 171] {
            llr[bit] *= -0.4;
        }
        let decoded = LdpcDecoder::new(&Js8Code).decode(&llr, 30, 0);
        assert_eq!(decoded.map(|found| found.payload), Some(payload));
    }

    #[test]
    fn data_blocks_carry_parity_then_message() {
        let blocks: Vec<(usize, usize)> = NORMAL.data_blocks().collect();
        assert_eq!(blocks, [(7, 29), (43, 29)]);
        let tones = NORMAL.tones_for(0x0400_0000_0000_0000_0001);
        assert_eq!(&tones[..7], ORIGINAL);
        let slow = SLOW.tones_for(0x1);
        assert_eq!(&slow[36..43], MODIFIED[1]);
    }
}
