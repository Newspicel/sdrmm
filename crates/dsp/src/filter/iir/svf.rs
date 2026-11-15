use std::f64::consts::PI;

/// What a state-variable filter passes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SvfShape {
    Lowpass,
    Highpass,
    /// Unity gain at the centre.
    Bandpass,
    Notch,
    /// A bell: `gain_db` at the centre, unity far from it.
    Peak {
        gain_db: f64,
    },
    Allpass,
}

/// A trapezoidal state-variable filter. Its state survives a retune, so a centre swept
/// while audio runs neither clicks nor rings, and it stays accurate at corners far below
/// the rate where a direct-form biquad in `f32` loses its poles.
#[derive(Clone, Copy, Debug)]
pub struct Svf {
    a1: f32,
    a2: f32,
    a3: f32,
    m0: f32,
    m1: f32,
    m2: f32,
    ic1: f32,
    ic2: f32,
}

impl Svf {
    const MIN_Q: f64 = 0.05;
    /// The lowest centre frequency, as a share of the rate.
    const MIN_FREQ: f64 = 1e-5;
    /// The highest centre frequency, as a share of the Nyquist rate.
    const MAX_FREQ: f64 = 0.995;

    #[must_use]
    pub fn new(shape: SvfShape, rate: f64, freq_hz: f64, q: f64) -> Self {
        let mut filter = Self {
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            m0: 1.0,
            m1: 0.0,
            m2: 0.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        filter.retune(shape, rate, freq_hz, q);
        filter
    }

    /// New coefficients over the running state.
    pub fn retune(&mut self, shape: SvfShape, rate: f64, freq_hz: f64, q: f64) {
        assert!(rate > 0.0, "rate must be positive");
        let lowest = rate * Self::MIN_FREQ;
        let freq = if freq_hz.is_finite() {
            freq_hz.clamp(lowest, rate / 2.0 * Self::MAX_FREQ)
        } else {
            lowest
        };
        let g = (PI * freq / rate).tan();
        let q = q.max(Self::MIN_Q);
        let bell = match shape {
            SvfShape::Peak { gain_db } if gain_db.is_finite() => 10f64.powf(gain_db / 40.0),
            _ => 1.0,
        };
        let k = 1.0 / (q * bell);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let (m0, m1, m2) = match shape {
            SvfShape::Lowpass => (0.0, 0.0, 1.0),
            SvfShape::Highpass => (1.0, -k, -1.0),
            SvfShape::Bandpass => (0.0, k, 0.0),
            SvfShape::Notch => (1.0, -k, 0.0),
            SvfShape::Peak { .. } => (1.0, k * (bell * bell - 1.0), 0.0),
            SvfShape::Allpass => (1.0, -2.0 * k, 0.0),
        };
        self.a1 = a1 as f32;
        self.a2 = (g * a1) as f32;
        self.a3 = (g * g * a1) as f32;
        self.m0 = m0 as f32;
        self.m1 = m1 as f32;
        self.m2 = m2 as f32;
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if !(self.ic1.is_finite() && self.ic2.is_finite()) {
            self.reset();
        }
        for s in samples {
            let v0 = *s;
            let v3 = v0 - self.ic2;
            let v1 = self.a1 * self.ic1 + self.a2 * v3;
            let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
            self.ic1 = 2.0 * v1 - self.ic1;
            self.ic2 = 2.0 * v2 - self.ic2;
            *s = self.m0 * v0 + self.m1 * v1 + self.m2 * v2;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_1_SQRT_2;
    use std::f64::consts::FRAC_1_SQRT_2 as Q_BUTTERWORTH;

    use super::*;
    use crate::testutil::{real_tone, rms_r};

    const RATE: f64 = 48_000.0;

    fn gain(mut filter: Svf, freq_hz: f64) -> f32 {
        let n = (RATE * 0.5) as usize;
        let mut x = real_tone(freq_hz / RATE, n);
        filter.process(&mut x);
        rms_r(&x[n / 2..]) / FRAC_1_SQRT_2
    }

    fn db(gain: f32) -> f32 {
        20.0 * gain.log10()
    }

    #[test]
    fn lowpass_and_highpass_are_3_db_down_at_the_corner_and_fall_12_db_an_octave() {
        for corner in [300.0f64, 3_000.0] {
            let low = Svf::new(SvfShape::Lowpass, RATE, corner, Q_BUTTERWORTH);
            assert!((gain(low, corner) - FRAC_1_SQRT_2).abs() < 0.03);
            assert!((0.2..0.28).contains(&gain(low, corner * 2.0)));

            let high = Svf::new(SvfShape::Highpass, RATE, corner, Q_BUTTERWORTH);
            assert!((gain(high, corner) - FRAC_1_SQRT_2).abs() < 0.03);
            assert!((0.2..0.28).contains(&gain(high, corner / 2.0)));
        }
    }

    #[test]
    fn bandpass_passes_its_centre_at_unity_and_cuts_an_octave_away() {
        let filter = Svf::new(SvfShape::Bandpass, RATE, 700.0, 7.0);
        assert!((gain(filter, 700.0) - 1.0).abs() < 0.02);
        assert!(gain(filter, 1_400.0) < 0.1);
        assert!(gain(filter, 350.0) < 0.1);
    }

    #[test]
    fn notch_removes_its_tone_and_leaves_its_neighbours() {
        let filter = Svf::new(SvfShape::Notch, RATE, 1_000.0, 10.0);
        assert!(gain(filter, 1_000.0) < 0.02);
        assert!(gain(filter, 500.0) > 0.95);
        assert!(gain(filter, 2_000.0) > 0.95);
    }

    #[test]
    fn peak_lifts_its_centre_by_its_gain_and_leaves_far_tones() {
        for gain_db in [12.0, -12.0] {
            let filter = Svf::new(SvfShape::Peak { gain_db }, RATE, 700.0, 5.0);
            assert!((db(gain(filter, 700.0)) - gain_db as f32).abs() < 0.3);
            assert!(db(gain(filter, 5_000.0)).abs() < 0.3);
        }
    }

    #[test]
    fn allpass_keeps_every_level() {
        let filter = Svf::new(SvfShape::Allpass, RATE, 1_000.0, 0.7);
        for freq in [100.0, 1_000.0, 8_000.0] {
            assert!((gain(filter, freq) - 1.0).abs() < 0.02, "{freq} Hz");
        }
    }

    #[test]
    fn stays_accurate_at_a_corner_far_below_the_rate() {
        let mut filter = Svf::new(SvfShape::Highpass, 192_000.0, 10.0, Q_BUTTERWORTH);
        let mut x = real_tone(10.0 / 192_000.0, 192_000 * 4);
        filter.process(&mut x);
        let at = rms_r(&x[x.len() / 2..]) / FRAC_1_SQRT_2;
        assert!((at - FRAC_1_SQRT_2).abs() < 0.03, "{at}");
    }

    #[test]
    fn a_retune_mid_stream_does_not_jump() {
        let mut filter = Svf::new(SvfShape::Peak { gain_db: 12.0 }, RATE, 600.0, 5.0);
        let mut x = real_tone(650.0 / RATE, 4_800);
        let (head, tail) = x.split_at_mut(2_400);
        filter.process(head);
        filter.retune(SvfShape::Peak { gain_db: 12.0 }, RATE, 700.0, 5.0);
        filter.process(tail);
        let step = (x[2_400] - x[2_399]).abs();
        let typical = x[2_300..2_399]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(step < typical * 1.5, "step {step} against {typical}");
    }

    #[test]
    fn recovers_after_a_non_finite_sample_and_survives_degenerate_settings() {
        let mut filter = Svf::new(SvfShape::Bandpass, RATE, 700.0, 5.0);
        let mut poisoned = vec![0.5f32; 64];
        poisoned[3] = f32::NAN;
        filter.process(&mut poisoned);
        let mut x = real_tone(700.0 / RATE, 4_800);
        filter.process(&mut x);
        assert!(x.iter().all(|v| v.is_finite()));

        for (freq, q) in [(0.0, 0.0), (RATE, -1.0), (f64::NAN, f64::NAN)] {
            let mut filter = Svf::new(SvfShape::Peak { gain_db: f64::NAN }, RATE, freq, q);
            let mut x = real_tone(1_000.0 / RATE, 4_800);
            filter.process(&mut x);
            assert!(
                x.iter().all(|v| v.is_finite() && v.abs() < 10.0),
                "{freq} {q}"
            );
        }
    }
}
