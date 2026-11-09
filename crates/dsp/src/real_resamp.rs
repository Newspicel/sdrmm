use crate::fir::{design_lowpass_kaiser, kaiser_taps};

const PHASES: usize = 512;
const LANES: usize = 4;

/// Resamples a real stream by any ratio. Each output is one short dot product with the
/// nearest of a fine table of polyphase branches, so a wide transition band costs few taps.
#[derive(Clone, Debug)]
pub struct RealResampler {
    rows: Vec<f32>,
    taps: usize,
    step: f64,
    time: f64,
    pending: Vec<f32>,
}

impl RealResampler {
    /// `ratio` is output over input rate; `pass` and `stop` bound the transition band as
    /// fractions of the input rate.
    #[must_use]
    pub fn new(ratio: f64, pass: f64, stop: f64, attenuation_db: f64) -> Self {
        assert!(ratio.is_finite() && ratio > 0.0, "ratio must be positive");
        assert!(0.0 < pass && pass < stop, "pass must sit below stop");
        let cutoff = (0.5 * (pass + stop)).min(0.49);
        let taps = kaiser_taps(stop - pass, attenuation_db)
            .max(LANES)
            .next_multiple_of(LANES);
        let proto =
            design_lowpass_kaiser(PHASES * taps + 1, cutoff / PHASES as f64, attenuation_db);
        let mut rows = vec![0.0f32; (PHASES + 1) * taps];
        for (phase, row) in rows.chunks_exact_mut(taps).enumerate() {
            for (k, slot) in row.iter_mut().enumerate() {
                *slot = proto[(taps - 1 - k) * PHASES + phase];
            }
            let sum: f32 = row.iter().sum();
            for value in row.iter_mut() {
                *value /= sum;
            }
        }
        Self {
            rows,
            taps,
            step: ratio.recip(),
            time: (taps / 2 - 1) as f64,
            pending: Vec::new(),
        }
    }

    /// Input samples between an input and the output it lands in.
    #[must_use]
    pub fn delay(&self) -> f64 {
        (self.taps / 2) as f64 - 1.0
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.time = self.delay();
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        out.clear();
        self.pending.extend_from_slice(input);
        let taps = self.taps;
        let half = taps / 2 - 1;
        while (self.time as usize) + taps - half <= self.pending.len() {
            let whole = self.time as usize;
            let phase = ((self.time - whole as f64) * PHASES as f64 + 0.5) as usize;
            let start = whole - half;
            out.push(dot(
                &self.pending[start..start + taps],
                &self.rows[phase * taps..(phase + 1) * taps],
            ));
            self.time += self.step;
        }
        let done = (self.time as usize)
            .saturating_sub(half)
            .min(self.pending.len());
        self.pending.drain(..done);
        self.time -= done as f64;
    }
}

fn dot(samples: &[f32], taps: &[f32]) -> f32 {
    let mut sum = [0.0f32; LANES];
    for (x, h) in samples
        .as_chunks::<LANES>()
        .0
        .iter()
        .zip(taps.as_chunks::<LANES>().0)
    {
        for lane in 0..LANES {
            sum[lane] = crate::madd(x[lane], h[lane], sum[lane]);
        }
    }
    (sum[0] + sum[1]) + (sum[2] + sum[3])
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;

    fn tone(cycles_per_sample: f64, len: usize) -> Vec<f32> {
        (0..len)
            .map(|n| (TAU * cycles_per_sample * n as f64).cos() as f32)
            .collect()
    }

    fn resample(r: &mut RealResampler, input: &[f32]) -> Vec<f32> {
        let mut all = Vec::new();
        let mut out = Vec::new();
        for chunk in input.chunks(777) {
            r.process(chunk, &mut out);
            all.extend_from_slice(&out);
        }
        all
    }

    fn worst_error(got: &[f32], want: impl Fn(usize) -> f64) -> f64 {
        got.iter()
            .enumerate()
            .skip(64)
            .take(got.len() - 128)
            .map(|(k, &v)| (f64::from(v) - want(k)).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_passband_tone_comes_out_at_the_new_rate() {
        for ratio in [0.886_723_75, 1.108_404_7, 0.443_361_9, 0.715_909] {
            let mut r = RealResampler::new(ratio, 0.18, 0.44, 60.0);
            let f = 0.15;
            let got = resample(&mut r, &tone(f, 40_000));
            let expected = (40_000.0 * ratio) as usize;
            assert!(
                got.len().abs_diff(expected) <= r.taps,
                "ratio {ratio}: {} outputs",
                got.len()
            );
            let delay = r.delay();
            let err = worst_error(&got, |k| (TAU * f * (k as f64 / ratio + delay)).cos());
            assert!(err < 3e-3, "ratio {ratio}: error {err}");
        }
    }

    #[test]
    fn a_stopband_tone_is_rejected() {
        let mut r = RealResampler::new(0.886_723_75, 0.18, 0.31, 55.0);
        let got = resample(&mut r, &tone(0.36, 40_000));
        let peak = got[64..got.len() - 64]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 3e-3, "leak {peak}");
    }

    #[test]
    fn dc_passes_at_unity_gain() {
        let mut r = RealResampler::new(1.3, 0.2, 0.4, 60.0);
        let got = resample(&mut r, &vec![0.75; 10_000]);
        assert!(got[16..].iter().all(|&v| (v - 0.75).abs() < 1e-5));
    }
}
