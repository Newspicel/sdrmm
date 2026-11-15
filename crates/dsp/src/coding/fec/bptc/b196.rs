use super::turbo::{
    ALPHA, BETA, CHASE_FLIPS, MAX_PASSES, conclude, decisions, half_pass, normalized, siso,
};
use crate::coding::fec::{block::ParityCode, conv::Soft};

/// The most a decision may contradict the channel, in mean bit reliabilities. A turbo decoder
/// finds some codeword near almost anything, so a decision overruling more is likelier one it
/// found in noise than the one sent.
const MAX_COST: f32 = 6.0;
const INTERLEAVE_STEP: usize = 181;
const RESERVED_BITS: usize = 1;
const ROW_BITS: usize = 15;
const ROW_DATA_BITS: usize = 11;
const COLUMN_BITS: usize = 13;
const DATA_ROWS: usize = 9;
const FIRST_ROW_SPARE_BITS: usize = 3;

pub struct Bptc196;

impl Bptc196 {
    pub const CODED_BITS: usize = 196;
    pub const DATA_BITS: usize = 96;

    #[must_use]
    pub fn decode(coded: &[bool; Self::CODED_BITS]) -> Option<([bool; Self::DATA_BITS], u32)> {
        let mut matrix = [false; Self::CODED_BITS];
        for (a, slot) in matrix.iter_mut().enumerate() {
            *slot = coded[Self::interleaved(a)];
        }
        let corrected = Self::settle(&mut matrix)?;
        Some((Self::extract(&matrix), corrected))
    }

    /// Turbo product decoding (Pyndiah): columns and rows pass extrinsic reliabilities to each
    /// other, so a component's wrong guess does not reach the other dimension as a certainty.
    #[must_use]
    pub fn decode_soft(coded: &[Soft; Self::CODED_BITS]) -> Option<([bool; Self::DATA_BITS], u32)> {
        let mut deinterleaved = [0 as Soft; Self::CODED_BITS];
        for (a, slot) in deinterleaved.iter_mut().enumerate() {
            *slot = coded[Self::interleaved(a)];
        }
        let channel = normalized(&deinterleaved)?;
        let mut extrinsic = [0.0f32; Self::CODED_BITS];
        let columns =
            || (0..ROW_BITS).map(|c| std::array::from_fn::<_, COLUMN_BITS, _>(|r| cell(r, c)));
        let rows =
            || (0..COLUMN_BITS).map(|r| std::array::from_fn::<_, ROW_BITS, _>(|k| cell(r, k)));
        for pass in 0..MAX_PASSES {
            let column_half = (ALPHA[2 * pass], BETA[2 * pass]);
            half_pass(
                &channel,
                &mut extrinsic,
                columns(),
                column_half,
                |y, beta, w| {
                    siso(
                        y,
                        CHASE_FLIPS,
                        beta,
                        |b| ParityCode::HAMMING_13_9.decode(b),
                        w,
                    );
                },
            );
            let row_half = (ALPHA[2 * pass + 1], BETA[2 * pass + 1]);
            half_pass(&channel, &mut extrinsic, rows(), row_half, |y, beta, w| {
                siso(
                    y,
                    CHASE_FLIPS,
                    beta,
                    |b| ParityCode::HAMMING_15_11.decode(b),
                    w,
                );
            });
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
        for (a, &bit) in Self::data_positions().zip(data.iter()) {
            matrix[a] = bit;
        }
        for r in 0..DATA_ROWS {
            ParityCode::HAMMING_15_11.encode(&mut matrix[cell(r, 0)..cell(r, ROW_BITS)]);
        }
        for c in 0..ROW_BITS {
            let mut column: [bool; COLUMN_BITS] = std::array::from_fn(|r| matrix[cell(r, c)]);
            ParityCode::HAMMING_13_9.encode(&mut column);
            for (r, bit) in column.into_iter().enumerate() {
                matrix[cell(r, c)] = bit;
            }
        }
        let mut coded = [false; Self::CODED_BITS];
        for (a, bit) in matrix.into_iter().enumerate() {
            coded[Self::interleaved(a)] = bit;
        }
        coded
    }

    fn interleaved(index: usize) -> usize {
        index * INTERLEAVE_STEP % Self::CODED_BITS
    }

    fn settle(matrix: &mut [bool; Self::CODED_BITS]) -> Option<u32> {
        let mut corrected = 0;
        for _ in 0..MAX_PASSES {
            let repaired = Self::pass(matrix)?;
            corrected += repaired;
            if repaired == 0 {
                return Some(corrected);
            }
        }
        None
    }

    fn pass(matrix: &mut [bool; Self::CODED_BITS]) -> Option<u32> {
        let mut corrected = 0;
        for c in 0..ROW_BITS {
            let mut column: [bool; COLUMN_BITS] = std::array::from_fn(|r| matrix[cell(r, c)]);
            corrected += ParityCode::HAMMING_13_9.decode(&mut column)?;
            for (r, bit) in column.into_iter().enumerate() {
                matrix[cell(r, c)] = bit;
            }
        }
        for r in 0..DATA_ROWS {
            corrected +=
                ParityCode::HAMMING_15_11.decode(&mut matrix[cell(r, 0)..cell(r, ROW_BITS)])?;
        }
        Some(corrected)
    }

    fn data_positions() -> impl Iterator<Item = usize> {
        (FIRST_ROW_SPARE_BITS..ROW_DATA_BITS)
            .map(|c| cell(0, c))
            .chain((1..DATA_ROWS).flat_map(|r| (0..ROW_DATA_BITS).map(move |c| cell(r, c))))
    }

    fn extract(matrix: &[bool; Self::CODED_BITS]) -> [bool; Self::DATA_BITS] {
        let mut out = [false; Self::DATA_BITS];
        for (slot, a) in out.iter_mut().zip(Self::data_positions()) {
            *slot = matrix[a];
        }
        out
    }
}

/// Position of row `r`, column `c` of the 13 by 15 product matrix, after its reserved bit.
const fn cell(r: usize, c: usize) -> usize {
    r * ROW_BITS + c + RESERVED_BITS
}
