use num_complex::Complex;

use super::{Constellation, DecodeError, bicm::point};

const MAX_LEVELS: usize = 16;
const MAX_DIMENSION: usize = 4;
const LLR_LIMIT: f32 = 32.0;

/// The levels nearest a sample that carry one value of one bit: the closest below and above
/// it, and the point between them where the closer one changes.
#[derive(Clone, Copy, Default)]
struct Nearest {
    below: f32,
    above: f32,
    split: f32,
}

impl Nearest {
    fn distance(self, value: f32) -> f32 {
        let level = if value < self.split {
            self.below
        } else {
            self.above
        };
        (value - level).powi(2)
    }
}

/// Max-log soft bits for a Gray-mapped QAM grid. Each axis is a PAM of evenly spaced levels,
/// so the gap a sample falls in fixes, for every bit, the two levels that can be nearest:
/// two lookups per bit instead of a search over every level.
pub struct Demapper {
    gaps: [[[Nearest; 2]; MAX_DIMENSION]; MAX_LEVELS + 1],
    dimension: usize,
    levels: usize,
    lowest: f32,
    inverse_step: f32,
}

impl Demapper {
    pub fn new(constellation: Constellation) -> Result<Self, DecodeError> {
        let dimension = constellation.bits() / 2;
        let levels = 1 << dimension;
        let mut sorted = [(0.0_f32, 0_usize); MAX_LEVELS];
        for (label, entry) in sorted[..levels].iter_mut().enumerate() {
            let word =
                (0..dimension).fold(0, |word, bit| word | ((label >> bit & 1) << (2 * bit + 1)));
            *entry = (point(word, constellation).re, label);
        }
        let sorted = &mut sorted[..levels];
        sorted.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut gaps = [[[Nearest::default(); 2]; MAX_DIMENSION]; MAX_LEVELS + 1];
        for (gap, bits) in gaps[..=levels].iter_mut().enumerate() {
            for (bit, values) in bits[..dimension].iter_mut().enumerate() {
                for (value, nearest) in values.iter_mut().enumerate() {
                    let holds =
                        |entry: &&(f32, usize)| entry.1 >> (dimension - 1 - bit) & 1 == value;
                    let below = sorted[..gap].iter().rev().find(holds).map(|entry| entry.0);
                    let above = sorted[gap..].iter().find(holds).map(|entry| entry.0);
                    let (below, above) = match (below, above) {
                        (Some(below), Some(above)) => (below, above),
                        (Some(only), None) | (None, Some(only)) => (only, only),
                        (None, None) => return Err(DecodeError::Parameters),
                    };
                    *nearest = Nearest {
                        below,
                        above,
                        split: f32::midpoint(below, above),
                    };
                }
            }
        }
        let (lowest, highest) = (sorted[0].0, sorted[levels - 1].0);
        Ok(Self {
            gaps,
            dimension,
            levels,
            lowest,
            inverse_step: (levels - 1) as f32 / (highest - lowest),
        })
    }

    /// Soft bits of `sample` at `precision`, one over the noise variance, I and Q interleaved
    /// as the bit interleaver expects them.
    pub fn soften(&self, sample: Complex<f32>, precision: f32) -> [f32; 8] {
        let mut soft = [0.0; 8];
        for (axis, value) in [sample.re, sample.im].into_iter().enumerate() {
            let gap = (((value - self.lowest) * self.inverse_step + 1.0) as usize).min(self.levels);
            for (bit, nearest) in self.gaps[gap][..self.dimension].iter().enumerate() {
                let llr = (nearest[1].distance(value) - nearest[0].distance(value)) * precision;
                soft[2 * bit + axis] = llr.clamp(-LLR_LIMIT, LLR_LIMIT);
            }
        }
        soft
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dvbt::t2::bicm::soften;

    #[test]
    fn gaps_give_the_soft_bits_of_a_search_over_every_point() {
        for constellation in [
            Constellation::Qpsk,
            Constellation::Qam16,
            Constellation::Qam64,
            Constellation::Qam256,
        ] {
            let bits = constellation.bits();
            let points: Vec<_> = (0..1 << bits)
                .map(|word| point(word, constellation))
                .collect();
            let demapper = Demapper::new(constellation).unwrap();
            let samples = (-3000..=3000)
                .map(|step| Complex::new(step as f32 / 997.0, step as f32 / -1409.0))
                .chain(points.iter().copied());
            for sample in samples {
                let fast = demapper.soften(sample, 0.07_f32.recip());
                let exhaustive = soften(sample, &points, bits, 0.07);
                for (a, b) in fast[..bits].iter().zip(&exhaustive[..bits]) {
                    assert!(
                        (a - b).abs() <= 1e-4 * b.abs().max(1.0),
                        "{constellation:?} {sample}"
                    );
                }
            }
        }
    }
}
