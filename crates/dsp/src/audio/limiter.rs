use num_complex::Complex;

/// A sample whose peak a [`Limiter`] holds: real audio by its magnitude, baseband by its
/// envelope.
pub trait Peak: Copy + Default {
    fn peak(self) -> f32;
    #[must_use]
    fn scaled(self, gain: f32) -> Self;
    #[must_use]
    fn clamped(self, ceiling: f32) -> Self;
}

impl Peak for f32 {
    fn peak(self) -> f32 {
        self.abs()
    }

    fn scaled(self, gain: f32) -> Self {
        self * gain
    }

    fn clamped(self, ceiling: f32) -> Self {
        self.clamp(-ceiling, ceiling)
    }
}

impl Peak for Complex<f32> {
    fn peak(self) -> f32 {
        self.norm()
    }

    fn scaled(self, gain: f32) -> Self {
        self * gain
    }

    fn clamped(self, ceiling: f32) -> Self {
        let norm = self.norm();
        if norm > ceiling {
            self * (ceiling / norm)
        } else {
            self
        }
    }
}

/// A lookahead peak limiter: no output passes its ceiling, and its gain moves smoothly enough
/// to keep a transmitter's spectrum clean where a hard clip would splatter.
///
/// It holds the lowest gain any sample in the lookahead needs and averages that hold over
/// the same span, so the gain is already down when a peak arrives. Output trails input by
/// [`Limiter::delay`] samples.
#[derive(Clone, Debug)]
pub struct Limiter<T: Peak> {
    ceiling: f32,
    release: f32,
    held: f32,
    line: Vec<T>,
    needed: Vec<f32>,
    smoothing: Vec<f32>,
    sum: f64,
    pos: usize,
}

impl<T: Peak> Limiter<T> {
    #[must_use]
    pub fn new(rate: f64, ceiling: f32, lookahead_s: f64, release_s: f64) -> Self {
        assert!(rate > 0.0, "rate must be positive");
        let span = ((rate * lookahead_s).round() as usize).max(1);
        Self {
            ceiling: if ceiling.is_finite() && ceiling > 0.0 {
                ceiling
            } else {
                1.0
            },
            release: crate::filter::iir::one_pole_coeff(rate, release_s.max(1.0 / rate)),
            held: 1.0,
            line: vec![T::default(); span],
            needed: vec![1.0; span],
            smoothing: vec![1.0; span],
            sum: span as f64,
            pos: 0,
        }
    }

    /// Samples the output trails the input by.
    #[must_use]
    pub fn delay(&self) -> usize {
        self.line.len() - 1
    }

    /// The gain applied to the latest output, 1 when idle.
    #[must_use]
    pub fn gain(&self) -> f32 {
        (self.sum / self.smoothing.len() as f64) as f32
    }

    pub fn reset(&mut self) {
        self.line.fill(T::default());
        self.needed.fill(1.0);
        self.smoothing.fill(1.0);
        self.sum = self.smoothing.len() as f64;
        self.held = 1.0;
        self.pos = 0;
    }

    pub fn process(&mut self, samples: &mut [T]) {
        for s in samples {
            *s = self.next(*s);
        }
    }

    fn next(&mut self, input: T) -> T {
        let span = self.line.len();
        let peak = input.peak();
        let input = if peak.is_finite() {
            input
        } else {
            T::default()
        };
        let need = if peak.is_finite() && peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };
        self.line[self.pos] = input;
        self.needed[self.pos] = need;
        let hold = self.needed.iter().copied().fold(1.0f32, f32::min);
        self.held = if hold < self.held {
            hold
        } else {
            self.held + (hold - self.held) * self.release
        };
        self.sum += f64::from(self.held) - f64::from(self.smoothing[self.pos]);
        self.smoothing[self.pos] = self.held;
        self.pos = (self.pos + 1) % span;
        if self.pos == 0 {
            self.sum = self.smoothing.iter().map(|&g| f64::from(g)).sum();
        }
        let gain = (self.sum / span as f64) as f32;
        self.line[self.pos].scaled(gain).clamped(self.ceiling)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;
    use crate::testutil::real_tone;

    const RATE: f64 = 48_000.0;

    fn limiter() -> Limiter<f32> {
        Limiter::new(RATE, 1.0, 1e-3, 0.05)
    }

    #[test]
    fn quiet_audio_passes_unchanged_after_the_delay() {
        let mut lim = limiter();
        let x: Vec<f32> = real_tone(1_000.0 / RATE, 4_800)
            .into_iter()
            .map(|v| v * 0.5)
            .collect();
        let mut y = x.clone();
        lim.process(&mut y);
        let d = lim.delay();
        for (k, (&out, &input)) in y[d..].iter().zip(&x).enumerate() {
            assert!((out - input).abs() < 1e-6, "sample {k}: {out} vs {input}");
        }
        assert!(y[..d].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn no_output_passes_the_ceiling() {
        let mut lim = Limiter::new(RATE, 0.8, 2e-3, 0.05);
        let mut x: Vec<f32> = (0..48_000)
            .map(|n| {
                let t = n as f64 / RATE;
                (3.0 * (TAU * 700.0 * t).sin() * (TAU * 3.0 * t).sin()) as f32
            })
            .collect();
        x[10_000] = 40.0;
        lim.process(&mut x);
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak <= 0.8 + 1e-6, "peak {peak}");
    }

    #[test]
    fn the_gain_is_already_down_when_a_step_arrives() {
        let mut lim = limiter();
        let mut x = vec![0.1f32; 960];
        x[480..].fill(4.0);
        lim.process(&mut x);
        let d = lim.delay();
        let edge = 480 + d;
        assert!((x[edge] - 1.0).abs() < 1e-3, "at the step {}", x[edge]);
        assert!(x[edge - 1] < 0.05, "before the step {}", x[edge - 1]);
    }

    #[test]
    fn the_gain_moves_without_a_jump() {
        let mut lim = limiter();
        let mut x = vec![0.5f32; 4_800];
        x[2_000] = 5.0;
        lim.process(&mut x);
        for pair in x.windows(2).skip(lim.delay()) {
            let step = (pair[1] - pair[0]).abs();
            let spike = pair[0] > 0.9 || pair[1] > 0.9;
            assert!(step < 0.02 || spike, "step {step}");
        }
    }

    #[test]
    fn the_gain_releases_back_to_unity() {
        let mut lim = limiter();
        let mut x = vec![0.5f32; 48_000];
        x[100] = 10.0;
        lim.process(&mut x);
        assert!((lim.gain() - 1.0).abs() < 1e-3, "gain {}", lim.gain());
    }

    #[test]
    fn baseband_is_held_by_its_envelope() {
        let mut lim = Limiter::<Complex<f32>>::new(RATE, 1.0, 1e-3, 0.05);
        let mut x: Vec<Complex<f32>> = (0..9_600)
            .map(|n| Complex::from_polar(if n % 3_000 < 200 { 2.5 } else { 0.4 }, n as f32 * 0.1))
            .collect();
        lim.process(&mut x);
        assert!(x.iter().all(|z| z.norm() <= 1.0 + 1e-6));
    }

    #[test]
    fn a_non_finite_sample_comes_out_silent_and_leaves_no_trace() {
        let mut lim = limiter();
        let mut x = vec![0.5f32; 480];
        x[10] = f32::NAN;
        x[20] = f32::INFINITY;
        lim.process(&mut x);
        assert!(x.iter().all(|v| v.is_finite()));
        assert!((lim.gain() - 1.0).abs() < 1e-6);
    }
}
