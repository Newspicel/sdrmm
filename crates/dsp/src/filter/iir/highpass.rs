use super::corner_coeff;

const SECTIONS: usize = 3;

#[derive(Clone, Debug)]
pub struct Highpass {
    lows: [f32; SECTIONS],
    coeff: f32,
}

impl Highpass {
    #[must_use]
    pub fn new(rate: f64, corner_hz: f64) -> Self {
        assert!(
            rate > 0.0 && corner_hz > 0.0,
            "rate and corner must be positive"
        );
        Self {
            lows: [0.0; SECTIONS],
            coeff: corner_coeff(rate, corner_hz),
        }
    }

    pub fn reset(&mut self) {
        self.lows = [0.0; SECTIONS];
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.lows.iter().all(|v| v.is_finite()) {
            self.reset();
        }
        for s in samples {
            for low in &mut self.lows {
                *low += self.coeff * (*s - *low);
                *s -= *low;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_1_SQRT_2;

    use super::*;
    use crate::testutil::{real_tone, rms_r};

    fn highpass_gain(corner_hz: f64, freq_hz: f64) -> f32 {
        let rate = 48_000.0;
        let mut filter = Highpass::new(rate, corner_hz);
        let n = (rate * 2.0) as usize;
        let mut x = real_tone(freq_hz / rate, n);
        filter.process(&mut x);
        rms_r(&x[n / 2..]) / FRAC_1_SQRT_2
    }

    #[test]
    fn highpass_takes_the_subaudible_band_out_and_keeps_the_voice_band() {
        let analytic = |f: f64| (f / f.hypot(300.0)).powi(3) as f32;
        for freq_hz in [67.0f64, 88.5, 254.1, 300.0, 1_000.0] {
            let gain = highpass_gain(300.0, freq_hz);
            let want = analytic(freq_hz);
            assert!(
                (gain / want - 1.0).abs() < 0.1,
                "{freq_hz} Hz: gain {gain}, expected {want}"
            );
        }
        assert!(highpass_gain(300.0, 88.5) < 0.03);
        assert!(highpass_gain(300.0, 3_000.0) > 0.9);
    }

    #[test]
    fn highpass_removes_a_dc_offset_and_recovers_from_a_non_finite_sample() {
        let mut filter = Highpass::new(48_000.0, 300.0);
        let mut dc = vec![1.0f32; 48_000];
        filter.process(&mut dc);
        assert!(dc[dc.len() - 1].abs() < 0.01, "dc residue {}", dc[47_999]);

        let mut poisoned = vec![0.5f32; 480];
        poisoned[7] = f32::NAN;
        filter.process(&mut poisoned);
        let mut tone = real_tone(1_000.0 / 48_000.0, 48_000);
        filter.process(&mut tone);
        assert!(tone.iter().all(|v| v.is_finite()), "state still poisoned");
    }
}
