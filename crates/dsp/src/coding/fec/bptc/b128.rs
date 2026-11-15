use super::turbo::{
    ALPHA, BETA, CHASE_FLIPS, MAX_PASSES, conclude, decisions, half_pass, normalized, parity_siso,
    siso,
};
use crate::coding::fec::{block::ParityCode, conv::Soft};

/// The most a decision may contradict the channel, in mean bit reliabilities; tighter than
/// BPTC(196,96) because single parity columns leave nearer neighbours.
const MAX_COST: f32 = 3.5;
/// How far a min-sum parity check's reliability is trusted. Min-sum overstates it, and with
/// only one parity per column an overstated check walks rows into wrong codewords; measured
/// against AWGN.
const PARITY_SCALE: f32 = 0.375;
const ROW_BITS: usize = 16;
const ROW_DATA_BITS: usize = 11;
const ROWS: usize = 8;
const DATA_ROWS: usize = ROWS - 1;

pub struct Bptc128;

impl Bptc128 {
    pub const CODED_BITS: usize = 128;
    pub const DATA_BITS: usize = 77;

    #[must_use]
    pub fn decode(coded: &[bool; Self::CODED_BITS]) -> Option<([bool; Self::DATA_BITS], u32)> {
        let mut matrix = [false; Self::CODED_BITS];
        for (a, &bit) in coded.iter().enumerate() {
            matrix[Self::transpose(a)] = bit;
        }
        let corrected = Self::settle(&mut matrix)?;
        Some((Self::extract(&matrix), corrected))
    }

    /// Turbo product decoding as in [`super::Bptc196::decode_soft`], with exact min-sum parity
    /// columns.
    #[must_use]
    pub fn decode_soft(coded: &[Soft; Self::CODED_BITS]) -> Option<([bool; Self::DATA_BITS], u32)> {
        let mut transposed = [0 as Soft; Self::CODED_BITS];
        for (a, &s) in coded.iter().enumerate() {
            transposed[Self::transpose(a)] = s;
        }
        let channel = normalized(&transposed)?;
        let mut extrinsic = [0.0f32; Self::CODED_BITS];
        let rows =
            || (0..ROWS).map(|r| std::array::from_fn::<_, ROW_BITS, _>(|k| r * ROW_BITS + k));
        let columns =
            || (0..ROW_BITS).map(|c| std::array::from_fn::<_, ROWS, _>(|r| r * ROW_BITS + c));
        for pass in 0..MAX_PASSES {
            let row_half = (ALPHA[2 * pass], BETA[2 * pass]);
            half_pass(&channel, &mut extrinsic, rows(), row_half, |y, beta, w| {
                siso(
                    y,
                    CHASE_FLIPS,
                    beta,
                    |b| ParityCode::HAMMING_16_11.decode(b),
                    w,
                );
            });
            let column_half = (ALPHA[2 * pass + 1], BETA[2 * pass + 1]);
            half_pass(
                &channel,
                &mut extrinsic,
                columns(),
                column_half,
                |y, _, w| {
                    parity_siso(y, PARITY_SCALE, w);
                },
            );
            if Self::settle(&mut decisions(&channel, &extrinsic)) == Some(0) {
                break;
            }
        }
        let (hard, corrected) = conclude(&channel, &extrinsic, MAX_COST, Self::settle)?;
        Some((Self::extract(&hard), corrected))
    }

    #[must_use]
    pub fn encode(data: &[bool; Self::DATA_BITS]) -> [bool; Self::CODED_BITS] {
        let mut matrix = [false; Self::CODED_BITS];
        for (i, &bit) in data.iter().enumerate() {
            matrix[Self::data_position(i)] = bit;
        }
        for r in 0..DATA_ROWS {
            ParityCode::HAMMING_16_11.encode(&mut matrix[r * ROW_BITS..(r + 1) * ROW_BITS]);
        }
        for c in 0..ROW_BITS {
            matrix[DATA_ROWS * ROW_BITS + c] =
                (0..DATA_ROWS).fold(false, |acc, r| acc ^ matrix[r * ROW_BITS + c]);
        }
        let mut coded = [false; Self::CODED_BITS];
        for (a, slot) in coded.iter_mut().enumerate() {
            *slot = matrix[Self::transpose(a)];
        }
        coded
    }

    /// Transposes the 8 by 16 matrix: index times 16 modulo 127, the last bit fixed.
    pub(super) fn transpose(index: usize) -> usize {
        let last = Self::CODED_BITS - 1;
        if index == last {
            last
        } else {
            index * ROW_BITS % last
        }
    }

    fn settle(matrix: &mut [bool; Self::CODED_BITS]) -> Option<u32> {
        let mut corrected = 0;
        for r in 0..DATA_ROWS {
            corrected +=
                ParityCode::HAMMING_16_11.decode(&mut matrix[r * ROW_BITS..(r + 1) * ROW_BITS])?;
        }
        for c in 0..ROW_BITS {
            if (0..ROWS).fold(false, |acc, r| acc ^ matrix[r * ROW_BITS + c]) {
                return None;
            }
        }
        Some(corrected)
    }

    fn data_position(i: usize) -> usize {
        i / ROW_DATA_BITS * ROW_BITS + i % ROW_DATA_BITS
    }

    fn extract(matrix: &[bool; Self::CODED_BITS]) -> [bool; Self::DATA_BITS] {
        std::array::from_fn(|i| matrix[Self::data_position(i)])
    }
}
