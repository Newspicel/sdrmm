use crate::filter::iir::one_pole_coeff;
use crate::math::stats::median;

#[derive(Clone, Debug)]
pub struct ClickRemover {
    window: Vec<f32>,
    write: usize,
    sorted: Vec<f32>,
    half: usize,
    average: f32,
    coeff: f32,
    threshold: f32,
    removed: u64,
}

impl ClickRemover {
    const AVERAGE_TAU_S: f64 = 0.05;

    #[must_use]
    pub fn new(rate: f64, width_s: f64, threshold: f32) -> Self {
        assert!(rate > 0.0, "rate must be positive");
        let half = ((rate * width_s / 2.0).round() as usize).max(1);
        let span = 2 * half + 1;
        Self {
            window: vec![0.0; span],
            write: 0,
            sorted: vec![0.0; span],
            half,
            average: 0.0,
            coeff: one_pole_coeff(rate, Self::AVERAGE_TAU_S),
            threshold: threshold.max(1.0),
            removed: 0,
        }
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold.max(1.0);
    }

    #[must_use]
    pub fn latency(&self) -> usize {
        self.half
    }

    #[must_use]
    pub fn removed_samples(&self) -> u64 {
        self.removed
    }

    pub fn reset(&mut self) {
        self.window.fill(0.0);
        self.write = 0;
        self.average = 0.0;
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.average.is_finite() {
            self.reset();
        }
        let span = self.window.len();
        for slot in samples {
            let x = if slot.is_finite() { *slot } else { 0.0 };
            self.write = (self.write + 1) % span;
            self.window[self.write] = x;

            let centre = self.window[(self.write + self.half + 1) % span];
            let magnitude = centre.abs();
            if self.average <= 0.0 {
                self.average = magnitude;
            } else {
                self.average += self.coeff * (magnitude - self.average);
            }

            let limit = self.threshold * self.average.max(f32::MIN_POSITIVE);
            *slot = if magnitude > limit {
                let median = self.median();
                if (centre - median).abs() > limit {
                    self.removed += 1;
                    median
                } else {
                    centre
                }
            } else {
                centre
            };
        }
    }

    fn median(&mut self) -> f32 {
        self.sorted.copy_from_slice(&self.window);
        median(&mut self.sorted).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audio::noise::testing::{RATE, tone_amplitude},
        testutil::{XorShift32, real_tone, rms_r},
    };

    const CLICK_WIDTH_S: f64 = 100e-6;
    const CLICK_THRESHOLD: f32 = 6.0;

    fn click_remover() -> ClickRemover {
        ClickRemover::new(RATE, CLICK_WIDTH_S, CLICK_THRESHOLD)
    }

    fn speech_with_clicks(len: usize, period: usize, amplitude: f32) -> Vec<f32> {
        let mut audio = real_tone(700.0 / RATE, len)
            .iter()
            .map(|s| s * 0.2)
            .collect::<Vec<_>>();
        for n in (period..len).step_by(period) {
            audio[n] = amplitude;
        }
        audio
    }

    fn run_clicks(remover: &mut ClickRemover, input: &[f32], block: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len());
        for chunk in input.chunks(block) {
            let mut buf = chunk.to_vec();
            remover.process(&mut buf);
            out.extend_from_slice(&buf);
        }
        out
    }

    #[test]
    fn clicks_are_cut_out_and_the_audio_under_them_is_kept() {
        let input = speech_with_clicks(48_000, 500, 3.0);
        let mut remover = click_remover();
        let output = run_clicks(&mut remover, &input, 480);

        let settled = 8_000;
        let peak = output[settled..].iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(peak < 0.3, "a click survived at {peak}");
        assert!(remover.removed_samples() > 0);
        let latency = remover.latency();
        let kept = tone_amplitude(&output[settled + latency..], 700.0);
        assert!((kept - 0.2).abs() < 0.02, "audio was chewed up: {kept}");
    }

    #[test]
    fn clean_audio_passes_through_untouched() {
        let mut rng = XorShift32(0x4B7C_1E39);
        let input: Vec<f32> = real_tone(900.0 / RATE, 48_000)
            .iter()
            .map(|t| t * 0.3 + rng.next_f32() * 0.05)
            .collect();
        let mut remover = click_remover();
        let output = run_clicks(&mut remover, &input, 997);
        let latency = remover.latency();
        for n in 4_000..input.len() - latency {
            assert!(
                (output[n + latency] - input[n]).abs() < 1e-6,
                "sample {n} was altered: {} vs {}",
                output[n + latency],
                input[n]
            );
        }
        assert_eq!(remover.removed_samples(), 0);
    }

    #[test]
    fn a_loud_transient_that_is_not_an_impulse_is_left_alone() {
        let mut input = vec![0.0f32; 24_000];
        input.extend(real_tone(500.0 / RATE, 24_000).iter().map(|s| s * 0.4));
        let mut remover = click_remover();
        let output = run_clicks(&mut remover, &input, 480);
        let latency = remover.latency();
        let burst = tone_amplitude(&output[28_000 + latency..], 500.0);
        assert!(burst > 0.35, "the burst was cut down to {burst}");
    }

    #[test]
    fn click_removal_returns_one_sample_for_every_sample_it_is_given() {
        let mut remover = click_remover();
        let input = speech_with_clicks(24_000, 300, 4.0);
        let (mut produced, mut pos) = (0, 0);
        for len in [13usize, 1_024, 97, 4_096].iter().cycle() {
            if pos >= input.len() {
                break;
            }
            let end = (pos + len).min(input.len());
            let mut buf = input[pos..end].to_vec();
            remover.process(&mut buf);
            assert_eq!(buf.len(), end - pos);
            produced += buf.len();
            pos = end;
        }
        assert_eq!(produced, input.len());
    }

    #[test]
    fn a_wider_window_removes_a_wider_click() {
        let mut input = real_tone(700.0 / RATE, 48_000)
            .iter()
            .map(|s| s * 0.2)
            .collect::<Vec<_>>();
        for n in (2_000..input.len()).step_by(1_000) {
            input[n..n + 6].fill(3.0);
        }
        let peak_after = |width_s: f64| {
            let mut remover = ClickRemover::new(RATE, width_s, CLICK_THRESHOLD);
            let output = run_clicks(&mut remover, &input, 480);
            output[8_000..].iter().fold(0.0f32, |a, s| a.max(s.abs()))
        };
        assert!(
            peak_after(100e-6) > 1.0,
            "the narrow window kept up somehow"
        );
        assert!(peak_after(400e-6) < 0.3, "the wide window missed the click");
    }

    #[test]
    fn click_removal_recovers_after_a_non_finite_sample() {
        let mut remover = click_remover();
        let mut poisoned = vec![f32::NAN; 256];
        poisoned[3] = f32::INFINITY;
        remover.process(&mut poisoned);
        assert!(poisoned.iter().all(|s| s.is_finite()));
        let mut back = real_tone(700.0 / RATE, 24_000)
            .iter()
            .map(|s| s * 0.2)
            .collect::<Vec<_>>();
        remover.process(&mut back);
        assert!(rms_r(&back[8_000..]) > 0.1, "audio stayed muted");
    }
}
