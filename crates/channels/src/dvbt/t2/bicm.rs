use num_complex::Complex;

use super::{Coding, Constellation, DecodeError, interleave};
use sdrmm_modem::dvb::{bb, bch::Bch, ldpc::Ldpc};

const MIN_GAIN: f32 = 1e-3;

pub struct Decoder {
    coding: Coding,
    cell_map: Vec<usize>,
    bit_map: Vec<usize>,
    demapper: Demapper,
    llrs: Vec<f32>,
    word: Vec<bool>,
    ldpc: Ldpc,
    bch: Bch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub bits: usize,
    pub ldpc_iterations: usize,
    pub corrected_bits: usize,
}

impl Decoder {
    pub fn new(coding: Coding) -> Result<Self, DecodeError> {
        let coding = coding.validate()?;
        let bch = Bch::new(coding.frame, coding.correct(), coding.message());
        let mut cell_map = interleave::cell_permutation(coding.cells())?;
        cell_map.reserve(coding.frame.length() / 2 - cell_map.len());
        Ok(Self {
            coding,
            cell_map,
            bit_map: interleave::bit_permutation(coding)?,
            demapper: Demapper::new(coding.constellation),
            llrs: vec![0.0; coding.frame.length()],
            word: Vec::with_capacity(coding.information()),
            ldpc: Ldpc::with_addresses(coding.frame, coding.addresses()?)
                .ok_or(DecodeError::Parameters)?,
            bch,
        })
    }

    pub fn configure(&mut self, coding: Coding) -> Result<(), DecodeError> {
        let coding = coding.validate()?;
        if coding.frame != self.coding.frame || coding.rate != self.coding.rate {
            return Err(DecodeError::Parameters);
        }
        if coding == self.coding {
            return Ok(());
        }
        self.cell_map.resize(coding.cells(), 0);
        interleave::cell_permutation_into(&mut self.cell_map)?;
        interleave::bit_permutation_into(coding, &mut self.bit_map)?;
        self.demapper = Demapper::new(coding.constellation);
        self.coding = coding;
        Ok(())
    }

    pub fn decode(
        &mut self,
        cells: &[Complex<f32>],
        block: usize,
        noise_variance: f32,
        output: &mut [bool],
    ) -> Result<Decoded, DecodeError> {
        self.decode_cells(cells, None, block, noise_variance, output)
    }

    pub fn decode_weighted(
        &mut self,
        cells: &[Complex<f32>],
        gains: &[f32],
        block: usize,
        noise_variance: f32,
        output: &mut [bool],
    ) -> Result<Decoded, DecodeError> {
        if gains.len() != cells.len() {
            return Err(DecodeError::Length);
        }
        self.decode_cells(cells, Some(gains), block, noise_variance, output)
    }

    fn decode_cells(
        &mut self,
        cells: &[Complex<f32>],
        gains: Option<&[f32]>,
        block: usize,
        noise_variance: f32,
        output: &mut [bool],
    ) -> Result<Decoded, DecodeError> {
        if cells.len() != self.coding.cells() || output.len() < self.coding.message() {
            return Err(DecodeError::Length);
        }
        if !noise_variance.is_finite() || noise_variance <= 0.0 {
            return Err(DecodeError::Parameters);
        }
        if cells.iter().any(|p| !p.norm_sqr().is_finite()) {
            return Err(DecodeError::NonFinite);
        }
        let shift = interleave::cell_shift(cells.len(), block)?;
        let rotation = self.rotation();
        let bits = self.coding.constellation.bits();
        let gain = |index: usize| gains.map_or(1.0, |gains| gains[index].max(MIN_GAIN));
        for i in 0..cells.len() {
            let (p, here, there) = self.cell(cells, shift, rotation, i);
            let reliability = f32::midpoint(gain(here), gain(there));
            let soft = self.demapper.soften(p, noise_variance / reliability);
            for (bit, &llr) in soft[..bits].iter().enumerate() {
                self.llrs[self.bit_map[i * bits + bit]] = llr;
            }
        }
        self.word.clear();
        let iterations = self
            .ldpc
            .decode(&self.llrs, &mut self.word)
            .ok_or(DecodeError::Ldpc)?;
        let corrected = self.bch.decode(&mut self.word).ok_or(DecodeError::Bch)?;
        let length = self.coding.message();
        output[..length].copy_from_slice(&self.word[..length]);
        bb::scramble(&mut output[..length]);
        Ok(Decoded {
            bits: length,
            ldpc_iterations: iterations,
            corrected_bits: corrected,
        })
    }

    /// Fills `out` up to its capacity with a block's first cells, deinterleaved and turned
    /// back onto the plain QAM grid, as the demapper sees them.
    pub fn derotated_into(
        &self,
        cells: &[Complex<f32>],
        block: usize,
        out: &mut Vec<Complex<f32>>,
    ) -> Result<(), DecodeError> {
        if cells.len() != self.coding.cells() {
            return Err(DecodeError::Length);
        }
        let shift = interleave::cell_shift(cells.len(), block)?;
        let rotation = self.rotation();
        let room = out.capacity() - out.len();
        out.extend((0..cells.len().min(room)).map(|i| self.cell(cells, shift, rotation, i).0));
        Ok(())
    }

    fn rotation(&self) -> Complex<f32> {
        Complex::from_polar(1.0, -self.coding.constellation.rotation())
    }

    /// Cell `i` in coded order, with where its I and its Q came from: a rotated
    /// constellation sends Q one cell later.
    fn cell(
        &self,
        cells: &[Complex<f32>],
        shift: usize,
        rotation: Complex<f32>,
        i: usize,
    ) -> (Complex<f32>, usize, usize) {
        let here = (self.cell_map[i] + shift) % cells.len();
        if !self.coding.rotated {
            return (cells[here], here, here);
        }
        let there = (self.cell_map[(i + 1) % cells.len()] + shift) % cells.len();
        (
            Complex::new(cells[here].re, cells[there].im) * rotation,
            here,
            there,
        )
    }
}

#[derive(Clone, Copy, Default)]
struct Nearest {
    lower: f32,
    upper: f32,
    midpoint: f32,
}

impl Nearest {
    fn squared_distance(self, value: f32) -> f32 {
        let closest = if value < self.midpoint {
            self.lower
        } else {
            self.upper
        };
        (value - closest).powi(2)
    }
}

struct Demapper {
    intervals: [[[Nearest; 2]; 4]; 17],
    dimension: usize,
    levels: usize,
    min: f32,
    inverse_step: f32,
}

impl Demapper {
    fn new(constellation: Constellation) -> Self {
        let dimension = constellation.bits() / 2;
        let levels = 1 << dimension;
        let mut amplitudes = [(0.0, 0); 16];
        for (label, entry) in amplitudes[..levels].iter_mut().enumerate() {
            let word =
                (0..dimension).fold(0, |word, bit| word | ((label >> bit & 1) << (2 * bit + 1)));
            *entry = (point(word, constellation).re, label);
        }
        let amplitudes = &mut amplitudes[..levels];
        amplitudes.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut intervals = [[[Nearest::default(); 2]; 4]; 17];
        for (interval, options) in intervals[..=levels].iter_mut().enumerate() {
            for (bit, classes) in options[..dimension].iter_mut().enumerate() {
                for (value, closest) in classes.iter_mut().enumerate() {
                    let has_value =
                        |(_, label): &&(f32, usize)| label >> (dimension - 1 - bit) & 1 == value;
                    let lower = amplitudes[..interval].iter().rev().find(has_value);
                    let upper = amplitudes[interval..].iter().find(has_value);
                    let lower = lower.or(upper).expect("each bit has both values").0;
                    let upper = upper.map_or(lower, |entry| entry.0);
                    *closest = Nearest {
                        lower,
                        upper,
                        midpoint: f32::midpoint(lower, upper),
                    };
                }
            }
        }
        Self {
            intervals,
            dimension,
            levels,
            min: amplitudes[0].0,
            inverse_step: (levels - 1) as f32 / (amplitudes[levels - 1].0 - amplitudes[0].0),
        }
    }

    fn soften(&self, sample: Complex<f32>, variance: f32) -> [f32; 8] {
        let mut soft = [0.0; 8];
        for (axis, value) in [sample.re, sample.im].into_iter().enumerate() {
            let interval =
                (((value - self.min) * self.inverse_step + 1.0) as usize).min(self.levels);
            for (bit, nearest) in self.intervals[interval][..self.dimension]
                .iter()
                .enumerate()
            {
                let zero = nearest[0].squared_distance(value);
                let one = nearest[1].squared_distance(value);
                soft[2 * bit + axis] = ((one - zero) / variance).clamp(-32.0, 32.0);
            }
        }
        soft
    }
}

pub fn point(word: usize, constellation: Constellation) -> Complex<f32> {
    let bits = constellation.bits();
    let dimension = bits / 2;
    let component = |axis| {
        let bit = |position| word >> (bits - 1 - 2 * position - axis) & 1;
        let mut amplitude = 1;
        for position in (1..dimension).rev() {
            amplitude = (1 << (dimension - position)) + (1 - 2 * bit(position) as i32) * amplitude;
        }
        (1 - 2 * bit(0) as i32) as f32 * amplitude as f32
    };
    let scale = ((2 * ((1 << bits) - 1)) as f32 / 3.0).sqrt();
    Complex::new(component(0), component(1)) / scale
}

pub(super) fn soften(
    sample: Complex<f32>,
    points: &[Complex<f32>],
    bits: usize,
    variance: f32,
) -> [f32; 8] {
    let mut distances = [[f32::INFINITY; 2]; 8];
    let dimension = bits / 2;
    for level in 0..1 << dimension {
        let word = (0..dimension).fold(0, |word, bit| word | ((level >> bit & 1) << (2 * bit + 1)));
        let amplitude = points[word].re;
        let real = (sample.re - amplitude).powi(2);
        let imaginary = (sample.im - amplitude).powi(2);
        for bit in 0..dimension {
            let value = level >> (dimension - 1 - bit) & 1;
            distances[2 * bit][value] = distances[2 * bit][value].min(real);
            distances[2 * bit + 1][value] = distances[2 * bit + 1][value].min(imaginary);
        }
    }
    let mut soft = [0.0; 8];
    for (out, distance) in soft[..bits].iter_mut().zip(&distances) {
        *out = ((distance[1] - distance[0]) / variance).clamp(-32.0, 32.0);
    }
    soft
}

#[cfg(test)]
mod demapper_tests {
    use super::*;

    #[test]
    fn interval_demapper_matches_exhaustive_distances() {
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
            let demapper = Demapper::new(constellation);
            for step in -2048..=2048 {
                let sample = Complex::new(step as f32 / 727.0, step as f32 / 1123.0);
                assert_eq!(
                    demapper.soften(sample, 0.15),
                    soften(sample, &points, bits, 0.15)
                );
            }
            for &sample in &points {
                assert_eq!(
                    demapper.soften(sample, 0.15),
                    soften(sample, &points, bits, 0.15)
                );
            }
        }
    }
}
