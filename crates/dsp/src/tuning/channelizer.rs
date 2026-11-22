mod kernel;
mod planes;
mod spectral;

use num_complex::Complex;

use kernel::{fir_any, fir_fixed, rotate, rotate_fixed};
use planes::{LANE_GROUP, Planes, twiddle_table};
use spectral::Spectral;

use crate::filter::fir::design_lowpass_kaiser;

const TAPS_PER_BIN: usize = 16;
const STOPBAND_DB: f64 = 70.0;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ChannelizerError {
    #[error("{bins} bins cannot be decimated by {decimation}")]
    Shape { bins: usize, decimation: usize },
    #[error("bin {bin} is outside the {bins} bins")]
    Bin { bin: i32, bins: usize },
    #[error("cutoff {cutoff} must lie in (0, 0.5)")]
    Cutoff { cutoff: f64 },
}

#[derive(Clone, Debug)]
enum Rotation {
    Table(Vec<Planes>),
    Spectral(Spectral),
}

impl Rotation {
    /// The table stays ahead where it has an unrolled kernel or only a few bins are wanted; the
    /// FFT wins everywhere else, by about 2x on a full 64 bin bank.
    fn for_shape(bins: usize, wanted: &[i32]) -> Self {
        let padded = wanted.len().next_multiple_of(LANE_GROUP);
        let unrolled = matches!((bins, padded), (12 | 10, 12) | (8, 8));
        if unrolled || wanted.len() * 4 < bins {
            Self::Table(twiddle_table(bins, wanted))
        } else {
            Self::Spectral(Spectral::new(bins, wanted))
        }
    }
}

#[derive(Clone, Debug)]
pub struct Channelizer {
    bins: usize,
    decimation: usize,
    reversed: Vec<f32>,
    rotation: Rotation,
    outputs: usize,
    padded: usize,
    history: Planes,
    partial: Planes,
    sums: Planes,
    span: usize,
    until_output: usize,
    position: u64,
}

impl Channelizer {
    pub fn new(
        bins: usize,
        decimation: usize,
        cutoff: f64,
        wanted: &[i32],
    ) -> Result<Self, ChannelizerError> {
        if bins < 2 || decimation == 0 || decimation > bins {
            return Err(ChannelizerError::Shape { bins, decimation });
        }
        if !(cutoff > 0.0 && cutoff < 0.5) {
            return Err(ChannelizerError::Cutoff { cutoff });
        }
        let half = bins as i32 / 2;
        if let Some(&bin) = wanted.iter().find(|&&bin| bin < -half || bin >= half) {
            return Err(ChannelizerError::Bin { bin, bins });
        }
        let span = bins * TAPS_PER_BIN;
        let mut reversed = design_lowpass_kaiser(span, cutoff, STOPBAND_DB);
        reversed.reverse();
        Ok(Self {
            bins,
            decimation,
            reversed,
            rotation: Rotation::for_shape(bins, wanted),
            outputs: wanted.len(),
            padded: wanted.len().next_multiple_of(LANE_GROUP),
            history: Planes::zeroed(span - 1),
            partial: Planes::zeroed(bins),
            sums: Planes::zeroed(wanted.len().next_multiple_of(LANE_GROUP)),
            span,
            until_output: decimation,
            position: 0,
        })
    }

    #[must_use]
    pub fn outputs(&self) -> usize {
        self.outputs
    }

    pub fn reset(&mut self) {
        self.history = Planes::zeroed(self.span - 1);
        self.until_output = self.decimation;
        self.position = 0;
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut [Vec<Complex<f32>>]) {
        for lane in out.iter_mut() {
            lane.clear();
        }
        let base = self.history.re.len();
        self.history.re.extend(input.iter().map(|sample| sample.re));
        self.history.im.extend(input.iter().map(|sample| sample.im));
        let mut consumed = 0;
        while self.until_output <= input.len() - consumed {
            consumed += self.until_output;
            self.until_output = self.decimation;
            let oldest = base + consumed - self.span;
            let absolute = self.position + consumed as u64 - 1;
            self.emit(oldest, absolute, out);
        }
        self.until_output -= input.len() - consumed;
        self.position += input.len() as u64;
        let drop = self.history.re.len() - (self.span - 1);
        self.history.re.drain(..drop);
        self.history.im.drain(..drop);
    }

    fn emit(&mut self, oldest: usize, absolute: u64, out: &mut [Vec<Complex<f32>>]) {
        let window_re = &self.history.re[oldest..oldest + self.span];
        let window_im = &self.history.im[oldest..oldest + self.span];
        match self.bins {
            8 => fir_fixed::<8>(&self.reversed, window_re, window_im, &mut self.partial),
            16 => fir_fixed::<16>(&self.reversed, window_re, window_im, &mut self.partial),
            10 => fir_fixed::<10>(&self.reversed, window_re, window_im, &mut self.partial),
            12 => fir_fixed::<12>(&self.reversed, window_re, window_im, &mut self.partial),
            bins => fir_any(
                bins,
                &self.reversed,
                window_re,
                window_im,
                &mut self.partial,
            ),
        }
        let phase = (absolute % self.bins as u64) as usize;
        match &mut self.rotation {
            Rotation::Spectral(spectral) => spectral.rotate(&self.partial, phase, &mut self.sums),
            Rotation::Table(tables) => {
                let table = &tables[phase];
                match (self.bins, self.padded) {
                    (12, 12) => rotate_fixed::<12, 12>(&self.partial, table, &mut self.sums),
                    (8, 8) => rotate_fixed::<8, 8>(&self.partial, table, &mut self.sums),
                    (10, 12) => rotate_fixed::<10, 12>(&self.partial, table, &mut self.sums),
                    (_, padded) => rotate(&self.partial, table, padded, &mut self.sums),
                }
            }
        }
        for ((sink, &re), &im) in out
            .iter_mut()
            .zip(&self.sums.re)
            .zip(&self.sums.im)
            .take(self.outputs)
        {
            sink.push(Complex::new(re, im));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;
    use crate::{math::complex::mean_power as power, testutil::complex_tone as tone};

    fn run(
        channelizer: &mut Channelizer,
        input: &[Complex<f32>],
        chunk: usize,
    ) -> Vec<Vec<Complex<f32>>> {
        let mut lanes = vec![Vec::new(); channelizer.outputs()];
        let mut scratch = vec![Vec::new(); channelizer.outputs()];
        for piece in input.chunks(chunk) {
            channelizer.process(piece, &mut scratch);
            for (lane, part) in lanes.iter_mut().zip(&scratch) {
                lane.extend_from_slice(part);
            }
        }
        lanes
    }

    #[test]
    fn a_tone_lands_in_its_own_bin_and_nowhere_else() {
        let mut channelizer = Channelizer::new(12, 9, 0.044, &[-5, -1, 0, 3, 4]).expect("shape");
        let input = tone(3.0 / 12.0 + 0.002, 90_000);
        let lanes = run(&mut channelizer, &input, 1_000);
        let settled: Vec<f32> = lanes.iter().map(|lane| power(&lane[200..])).collect();
        assert!(
            (settled[3] - 1.0).abs() < 0.05,
            "wanted bin power {}",
            settled[3]
        );
        for (index, &level) in settled.iter().enumerate().filter(|&(index, _)| index != 3) {
            assert!(level < 1e-5, "bin {index} leaked {level}");
        }
    }

    #[test]
    fn the_output_rate_is_the_input_rate_over_the_decimation() {
        let mut channelizer = Channelizer::new(8, 6, 0.06, &[0]).expect("shape");
        let lanes = run(&mut channelizer, &tone(0.0, 6_000), 7);
        assert_eq!(lanes[0].len(), 1_000);
    }

    #[test]
    fn chunking_does_not_change_the_output() {
        let input = tone(-2.0 / 12.0 + 0.01, 9_000);
        let mut whole = Channelizer::new(12, 9, 0.044, &[-2]).expect("shape");
        let mut pieces = Channelizer::new(12, 9, 0.044, &[-2]).expect("shape");
        let a = run(&mut whole, &input, 9_000);
        let b = run(&mut pieces, &input, 13);
        assert_eq!(a, b);
    }

    #[test]
    fn a_bin_keeps_the_phase_of_the_mixed_down_signal() {
        let offset = 0.003;
        let mut channelizer = Channelizer::new(12, 9, 0.044, &[1]).expect("shape");
        let lanes = run(&mut channelizer, &tone(1.0 / 12.0 + offset, 36_000), 4_096);
        let lane = &lanes[0][100..];
        let step = TAU * offset * 9.0;
        for pair in lane.windows(2) {
            let turned = f64::from((pair[1] * pair[0].conj()).arg());
            assert!(
                (turned - step).abs() < 1e-3,
                "rotated {turned}, expected {step}"
            );
        }
    }

    #[test]
    fn rejects_shapes_it_cannot_build() {
        assert_eq!(
            Channelizer::new(12, 13, 0.04, &[0]).err(),
            Some(ChannelizerError::Shape {
                bins: 12,
                decimation: 13
            })
        );
        assert_eq!(
            Channelizer::new(12, 9, 0.04, &[6]).err(),
            Some(ChannelizerError::Bin { bin: 6, bins: 12 })
        );
        assert_eq!(
            Channelizer::new(12, 9, 0.6, &[0]).err(),
            Some(ChannelizerError::Cutoff { cutoff: 0.6 })
        );
    }

    #[test]
    fn an_odd_bin_count_takes_the_general_path() {
        let mut channelizer = Channelizer::new(10, 7, 0.05, &[2, -3]).expect("shape");
        let lanes = run(&mut channelizer, &tone(-3.0 / 10.0 + 0.001, 70_000), 999);
        assert!((power(&lanes[1][200..]) - 1.0).abs() < 0.05);
        assert!(power(&lanes[0][200..]) < 1e-5);
    }
}
