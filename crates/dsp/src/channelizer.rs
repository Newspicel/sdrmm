use std::f64::consts::TAU;

use num_complex::Complex;

use crate::fir::design_lowpass_kaiser;

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
struct Planes {
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Planes {
    fn zeroed(len: usize) -> Self {
        Self {
            re: vec![0.0; len],
            im: vec![0.0; len],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Channelizer {
    bins: usize,
    decimation: usize,
    reversed: Vec<f32>,
    twiddles: Vec<Planes>,
    outputs: usize,
    padded: usize,
    history: Planes,
    partial: Planes,
    sums: Planes,
    span: usize,
    until_output: usize,
    position: u64,
}

const LANE_GROUP: usize = 4;

fn twiddle_table(bins: usize, wanted: &[i32]) -> Vec<Planes> {
    let padded = wanted.len().next_multiple_of(LANE_GROUP);
    (0..bins)
        .map(|phase| {
            let mut table = Planes::zeroed(bins * padded);
            for slot in 0..bins {
                let step = (phase + slot + 1) % bins;
                for (lane, &bin) in wanted.iter().enumerate() {
                    let turns = f64::from(bin) * step as f64 / bins as f64;
                    let (sin, cos) = (-TAU * turns).sin_cos();
                    table.re[slot * padded + lane] = cos as f32;
                    table.im[slot * padded + lane] = sin as f32;
                }
            }
            table
        })
        .collect()
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
            twiddles: twiddle_table(bins, wanted),
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
        let table = &self.twiddles[(absolute % self.bins as u64) as usize];
        match (self.bins, self.padded) {
            (12, 12) => rotate_fixed::<12, 12>(&self.partial, table, &mut self.sums),
            (8, 8) => rotate_fixed::<8, 8>(&self.partial, table, &mut self.sums),
            (10, 12) => rotate_fixed::<10, 12>(&self.partial, table, &mut self.sums),
            (_, padded) => rotate(&self.partial, table, padded, &mut self.sums),
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

fn fir_fixed<const BINS: usize>(
    taps: &[f32],
    window_re: &[f32],
    window_im: &[f32],
    partial: &mut Planes,
) {
    let mut re = [0.0f32; BINS];
    let mut im = [0.0f32; BINS];
    let (taps, _) = taps.as_chunks::<BINS>();
    let (window_re, _) = window_re.as_chunks::<BINS>();
    let (window_im, _) = window_im.as_chunks::<BINS>();
    for ((tap, sample_re), sample_im) in taps.iter().zip(window_re).zip(window_im) {
        for slot in 0..BINS {
            re[slot] += tap[slot] * sample_re[slot];
            im[slot] += tap[slot] * sample_im[slot];
        }
    }
    partial.re.copy_from_slice(&re);
    partial.im.copy_from_slice(&im);
}

fn fir_any(bins: usize, taps: &[f32], window_re: &[f32], window_im: &[f32], partial: &mut Planes) {
    partial.re.fill(0.0);
    partial.im.fill(0.0);
    for ((tap, re), im) in taps
        .chunks_exact(bins)
        .zip(window_re.chunks_exact(bins))
        .zip(window_im.chunks_exact(bins))
    {
        for (((acc_re, acc_im), &tap), (&re, &im)) in partial
            .re
            .iter_mut()
            .zip(partial.im.iter_mut())
            .zip(tap)
            .zip(re.iter().zip(im))
        {
            *acc_re += tap * re;
            *acc_im += tap * im;
        }
    }
}

fn rotate_fixed<const BINS: usize, const PADDED: usize>(
    partial: &Planes,
    table: &Planes,
    sums: &mut Planes,
) {
    let mut sum_re = [0.0f32; PADDED];
    let mut sum_im = [0.0f32; PADDED];
    let (rows_re, _) = table.re.as_chunks::<PADDED>();
    let (rows_im, _) = table.im.as_chunks::<PADDED>();
    for slot in 0..BINS {
        let (a, b) = (partial.re[slot], partial.im[slot]);
        let (c, d) = (&rows_re[slot], &rows_im[slot]);
        for lane in 0..PADDED {
            sum_re[lane] += a * c[lane] - b * d[lane];
            sum_im[lane] += a * d[lane] + b * c[lane];
        }
    }
    sums.re.copy_from_slice(&sum_re);
    sums.im.copy_from_slice(&sum_im);
}

fn rotate(partial: &Planes, table: &Planes, padded: usize, sums: &mut Planes) {
    sums.re.fill(0.0);
    sums.im.fill(0.0);
    for (((&a, &b), c), d) in partial
        .re
        .iter()
        .zip(&partial.im)
        .zip(table.re.chunks_exact(padded))
        .zip(table.im.chunks_exact(padded))
    {
        for (((sum_re, sum_im), &c), &d) in sums.re.iter_mut().zip(sums.im.iter_mut()).zip(c).zip(d)
        {
            *sum_re += a * c - b * d;
            *sum_im += a * d + b * c;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use num_complex::Complex;

    use super::{
        Channelizer, ChannelizerError, Planes, fir_any, fir_fixed, rotate, rotate_fixed,
        twiddle_table,
    };

    fn tone(freq: f64, len: usize) -> Vec<Complex<f32>> {
        (0..len)
            .map(|n| Complex::from_polar(1.0, (TAU * freq * n as f64) as f32))
            .collect()
    }

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

    fn power(samples: &[Complex<f32>]) -> f32 {
        samples.iter().map(Complex::norm_sqr).sum::<f32>() / samples.len() as f32
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
            let turned = (pair[1] * pair[0].conj()).arg() as f64;
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
    fn the_fixed_and_general_filters_agree() {
        let taps: Vec<f32> = (0..192)
            .map(|k| ((k * 37) % 101) as f32 / 101.0 - 0.5)
            .collect();
        let re: Vec<f32> = (0..192)
            .map(|k| ((k * 13) % 29) as f32 / 29.0 - 0.5)
            .collect();
        let im: Vec<f32> = (0..192)
            .map(|k| ((k * 7) % 31) as f32 / 31.0 - 0.5)
            .collect();
        let mut fixed = Planes::zeroed(12);
        let mut general = Planes::zeroed(12);
        fir_fixed::<12>(&taps, &re, &im, &mut fixed);
        fir_any(12, &taps, &re, &im, &mut general);
        for (a, b) in fixed
            .re
            .iter()
            .chain(&fixed.im)
            .zip(general.re.iter().chain(&general.im))
        {
            assert!((a - b).abs() < 1e-5, "{a} vs {b}");
        }
    }

    #[test]
    fn the_fixed_and_general_rotations_agree() {
        let partial = Planes {
            re: (0..12).map(|k| k as f32 * 0.1 - 0.5).collect(),
            im: (0..12).map(|k| 0.3 - k as f32 * 0.05).collect(),
        };
        let table = twiddle_table(12, &[-5, -2, 0, 1, 3, 4, 5, -1, -3, 2]).swap_remove(7);
        let mut fixed = Planes::zeroed(12);
        let mut general = Planes::zeroed(12);
        rotate_fixed::<12, 12>(&partial, &table, &mut fixed);
        rotate(&partial, &table, 12, &mut general);
        for (a, b) in fixed
            .re
            .iter()
            .chain(&fixed.im)
            .zip(general.re.iter().chain(&general.im))
        {
            assert!((a - b).abs() < 1e-5, "{a} vs {b}");
        }
    }

    #[test]
    fn ten_bins_match_the_general_path() {
        let taps: Vec<f32> = (0..160)
            .map(|k| ((k * 37) % 101) as f32 / 101.0 - 0.5)
            .collect();
        let re: Vec<f32> = (0..160)
            .map(|k| ((k * 13) % 29) as f32 / 29.0 - 0.5)
            .collect();
        let im: Vec<f32> = (0..160)
            .map(|k| ((k * 7) % 31) as f32 / 31.0 - 0.5)
            .collect();
        let mut fixed = Planes::zeroed(10);
        let mut general = Planes::zeroed(10);
        fir_fixed::<10>(&taps, &re, &im, &mut fixed);
        fir_any(10, &taps, &re, &im, &mut general);
        let table = twiddle_table(10, &[-4, -3, -2, -1, 0, 1, 2, 3, 4]).swap_remove(3);
        let mut rotated = Planes::zeroed(12);
        let mut reference = Planes::zeroed(12);
        rotate_fixed::<10, 12>(&fixed, &table, &mut rotated);
        rotate(&general, &table, 12, &mut reference);
        for (a, b) in rotated
            .re
            .iter()
            .chain(&rotated.im)
            .zip(reference.re.iter().chain(&reference.im))
        {
            assert!((a - b).abs() < 1e-4, "{a} vs {b}");
        }
    }

    #[test]
    fn an_odd_bin_count_takes_the_general_path() {
        let mut channelizer = Channelizer::new(10, 7, 0.05, &[2, -3]).expect("shape");
        let lanes = run(&mut channelizer, &tone(-3.0 / 10.0 + 0.001, 70_000), 999);
        assert!((power(&lanes[1][200..]) - 1.0).abs() < 0.05);
        assert!(power(&lanes[0][200..]) < 1e-5);
    }
}
