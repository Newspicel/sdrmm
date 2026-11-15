use num_complex::Complex;

use super::assert_slot_timing;
use crate::pulse::{self, Norm};

pub const OVERSAMPLE: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct SlotWaveform {
    samples_per_slot: f64,
    phase: f64,
    level: f32,
}

impl SlotWaveform {
    #[must_use]
    pub fn new(samples_per_slot: f64, phase: f64, level: f32) -> Self {
        assert_slot_timing(samples_per_slot, phase);
        assert!(level.is_finite(), "level must be finite");
        Self {
            samples_per_slot,
            phase,
            level,
        }
    }

    #[must_use]
    pub fn samples(&self, slots: usize) -> usize {
        (slots as f64 * self.samples_per_slot + self.phase).round() as usize
    }

    pub fn render(&self, on: &[bool], out: &mut Vec<Complex<f32>>) {
        let samples = self.samples(on.len());
        let fine_phase = self.phase * OVERSAMPLE as f64;
        let fine_per_slot = self.samples_per_slot * OVERSAMPLE as f64;
        out.reserve(samples);
        for sample in 0..samples {
            let mut acc = Complex::new(0.0f32, 0.0);
            for step in 0..OVERSAMPLE {
                let fine = (sample * OVERSAMPLE + step) as f64;
                let slot = (fine - fine_phase) / fine_per_slot;
                if slot >= 0.0 && on.get(slot as usize).copied().unwrap_or(false) {
                    acc += Complex::new(self.level, 0.0);
                }
            }
            out.push(acc / OVERSAMPLE as f32);
        }
    }
}

#[must_use]
pub fn slot_taps(samples_per_slot: f64) -> Vec<f32> {
    pulse::rect(samples_per_slot, Norm::Energy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(on: &[bool], sps: f64, phase: f64) -> Vec<f32> {
        let mut out = Vec::new();
        SlotWaveform::new(sps, phase, 1.0).render(on, &mut out);
        out.iter().map(|s| s.re).collect()
    }

    #[test]
    fn an_integer_rate_renders_the_rect_pulse_exactly() {
        let taps = slot_taps(8.0);
        let scale = 1.0 / taps[0];
        let rendered = render(&[false, true, false], 8.0, 0.0);
        assert_eq!(rendered.len(), 24);
        for (i, &s) in rendered.iter().enumerate() {
            let expect = if (8..16).contains(&i) {
                taps[i - 8] * scale
            } else {
                0.0
            };
            assert!((s - expect).abs() < 1e-6, "sample {i}: {s} vs {expect}");
        }
    }

    #[test]
    fn a_boundary_inside_a_sample_reads_partial_amplitude() {
        let rendered = render(&[true, false], 2.0, 0.5);
        assert!((rendered[0] - 0.5).abs() < 1e-6, "{rendered:?}");
        assert!((rendered[1] - 1.0).abs() < 1e-6, "{rendered:?}");
        assert!((rendered[2] - 0.5).abs() < 1e-6, "{rendered:?}");
    }

    #[test]
    fn a_keyed_slot_radiates_its_width_at_every_phase() {
        for phase in [0.0, 0.19, 0.5, 0.93] {
            for &sps in &[1.024, 2.4, 7.0] {
                let mut on = vec![false; 12];
                on[5] = true;
                let mut out = Vec::new();
                SlotWaveform::new(sps, phase, 1.0).render(&on, &mut out);
                let sum: f64 = out.iter().map(|s| f64::from(s.re)).sum();
                assert!(
                    (sum - sps).abs() < 0.05,
                    "sps {sps} phase {phase}: radiated {sum}"
                );
            }
        }
    }
}
