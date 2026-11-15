use crate::filter::iir::one_pole_coeff;

const HYSTERESIS: f32 = 0.1;
const MIN_SNR: f32 = 6.0;

#[derive(Clone, Copy, Debug)]
pub struct KeyingTiming {
    pub peak_rise_s: f64,
    pub peak_fall_s: f64,
    pub floor_fall_s: f64,
    pub floor_rise_s: f64,
    pub warmup_s: f64,
}

impl KeyingTiming {
    pub const MORSE: Self = Self {
        peak_rise_s: 5e-3,
        peak_fall_s: 0.5,
        floor_fall_s: 20e-3,
        floor_rise_s: 1.0,
        warmup_s: 20e-3,
    };

    pub const BURST: Self = Self {
        peak_rise_s: 100e-6,
        peak_fall_s: 50e-3,
        floor_fall_s: 2e-3,
        floor_rise_s: 100e-3,
        warmup_s: 2e-3,
    };
}

#[derive(Clone, Debug)]
pub struct KeyingSlicer {
    peak: f32,
    floor: f32,
    warmup_samples: u32,
    peak_rise: f32,
    peak_fall: f32,
    floor_rise: f32,
    floor_fall: f32,
    warmup: u32,
    key: bool,
}

impl KeyingSlicer {
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        Self::with_timing(sample_rate, KeyingTiming::MORSE)
    }

    #[must_use]
    pub fn with_timing(sample_rate: f64, timing: KeyingTiming) -> Self {
        assert!(sample_rate > 0.0, "sample rate must be positive");
        let warmup_samples = (timing.warmup_s * sample_rate).round().max(1.0) as u32;
        Self {
            peak: 0.0,
            floor: 0.0,
            warmup_samples,
            peak_rise: one_pole_coeff(sample_rate, timing.peak_rise_s),
            peak_fall: one_pole_coeff(sample_rate, timing.peak_fall_s),
            floor_rise: one_pole_coeff(sample_rate, timing.floor_rise_s),
            floor_fall: one_pole_coeff(sample_rate, timing.floor_fall_s),
            warmup: warmup_samples,
            key: false,
        }
    }

    pub fn push(&mut self, envelope: f32) -> bool {
        if !envelope.is_finite() {
            return self.key;
        }
        if self.warmup > 0 {
            self.warmup -= 1;
            self.peak = self.peak.max(envelope);
            self.floor = self.peak;
            return false;
        }
        let peak_coeff = if envelope > self.peak {
            self.peak_rise
        } else {
            self.peak_fall
        };
        self.peak += peak_coeff * (envelope - self.peak);
        let floor_coeff = if envelope < self.floor {
            self.floor_fall
        } else if self.key {
            0.0
        } else {
            self.floor_rise
        };
        self.floor += floor_coeff * (envelope - self.floor);

        let span = (self.peak - self.floor).max(0.0);
        let mid = self.floor + span / 2.0;
        let guard = span * HYSTERESIS;
        if envelope > mid + guard {
            self.key = true;
        } else if envelope < mid - guard {
            self.key = false;
        }
        if self.snr() < MIN_SNR {
            self.key = false;
        }
        self.key
    }

    #[must_use]
    pub fn snr(&self) -> f32 {
        self.peak / self.floor.max(f32::MIN_POSITIVE)
    }

    pub fn reset(&mut self) {
        self.peak = 0.0;
        self.floor = 0.0;
        self.warmup = self.warmup_samples;
        self.key = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demod::tone::Envelope, testutil::XorShift32};

    const CW_RATE: f64 = 8_000.0;
    const DOT: usize = 480;

    fn keyed_envelope(pattern: &[(bool, usize)], noise: f32) -> Vec<f32> {
        let mut rng = XorShift32(0x5eed_1234);
        let mut env = Envelope::new(CW_RATE, 2e-3, 2e-3);
        let mut out = Vec::new();
        for &(on, dots) in pattern {
            for _ in 0..dots * DOT {
                let level = f32::from(u8::from(on)) + rng.next_f32() * noise;
                out.push(env.push(level.max(0.0)));
            }
        }
        out
    }

    #[test]
    fn keying_slicer_recovers_a_noisy_pattern() {
        let pattern = [
            (false, 4),
            (true, 1),
            (false, 1),
            (true, 3),
            (false, 1),
            (true, 1),
            (false, 1),
            (true, 1),
            (false, 3),
        ];
        let signal = keyed_envelope(&pattern, 0.1);
        let mut slicer = KeyingSlicer::new(CW_RATE);
        let keys: Vec<bool> = signal.iter().map(|&v| slicer.push(v)).collect();

        let mut start = 0;
        for (i, &(on, dots)) in pattern.iter().enumerate() {
            let len = dots * DOT;
            if i > 0 {
                let steady = &keys[start + len / 4..start + 3 * len / 4];
                let wrong = steady.iter().filter(|&&k| k != on).count();
                assert_eq!(
                    wrong,
                    0,
                    "element {i} (key {on}): {wrong} of {} samples wrong",
                    steady.len()
                );
            }
            start += len;
        }
    }

    #[test]
    fn keying_slicer_ignores_pure_noise() {
        let signal = keyed_envelope(&[(false, 40)], 0.5);
        let mut slicer = KeyingSlicer::new(CW_RATE);
        for (n, &v) in signal.iter().enumerate() {
            assert!(!slicer.push(v), "keyed on noise at sample {n}");
        }
        let snr = slicer.snr();
        assert!(snr < MIN_SNR, "noise reported snr {snr}");
    }

    #[test]
    fn keying_slicer_holds_through_a_non_finite_sample() {
        let signal = keyed_envelope(&[(false, 2), (true, 4)], 0.05);
        let mut slicer = KeyingSlicer::new(CW_RATE);
        let mut key = false;
        for &v in &signal {
            key = slicer.push(v);
        }
        assert!(key, "should be keyed at the end of a key-down run");
        assert!(slicer.push(f32::NAN), "state dropped on NaN");
        assert!(slicer.snr().is_finite());
    }
}
