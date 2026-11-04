//! Beacon: sends frames as two-tone FSK, the transmit half of the example plugin.

use std::collections::VecDeque;

use sdrmm_plugin::{Complex, Payload, Transmitter};

use crate::{CarrierSettings, INPUT_RATE_HZ};

/// The mark tone sits this far above the channel frequency, the space tone as far below.
pub const SHIFT_HZ: f64 = 1_000.0;
pub const BAUD: f64 = 100.0;
const AMPLITUDE: f32 = 0.5;
const MAX_QUEUED_BITS: usize = 8 * 4_096;

pub struct Beacon {
    bits: VecDeque<bool>,
    tone_hz: f64,
    left: usize,
    phase: f64,
    samples_per_bit: usize,
}

fn fits(settings: &CarrierSettings) -> Result<(), String> {
    if settings.bandwidth_hz / 2.0 < SHIFT_HZ {
        Err(format!(
            "beacon tones at ±{SHIFT_HZ} Hz need a width of {} Hz",
            2.0 * SHIFT_HZ
        ))
    } else {
        Ok(())
    }
}

impl Beacon {
    fn next_bit(&mut self) -> bool {
        let Some(bit) = self.bits.pop_front() else {
            return false;
        };
        self.tone_hz = if bit { SHIFT_HZ } else { -SHIFT_HZ };
        self.left = self.samples_per_bit;
        true
    }

    fn sample(&mut self) -> Complex<f32> {
        self.phase = (self.phase + std::f64::consts::TAU * self.tone_hz / INPUT_RATE_HZ)
            .rem_euclid(std::f64::consts::TAU);
        self.left -= 1;
        Complex::new(self.phase.cos() as f32, self.phase.sin() as f32) * AMPLITUDE
    }
}

impl Transmitter for Beacon {
    type Settings = CarrierSettings;

    fn open(rate_hz: f64, settings: CarrierSettings) -> Result<Self, String> {
        if rate_hz != INPUT_RATE_HZ {
            return Err(format!("beacon sends at {INPUT_RATE_HZ} Hz, got {rate_hz}"));
        }
        fits(&settings)?;
        Ok(Self {
            bits: VecDeque::new(),
            tone_hz: 0.0,
            left: 0,
            phase: 0.0,
            samples_per_bit: (rate_hz / BAUD).round() as usize,
        })
    }

    fn apply(&mut self, settings: CarrierSettings) -> Result<(), String> {
        fits(&settings)
    }

    fn submit(&mut self, payload: Payload<'_>) -> Result<(), String> {
        let Payload::Frame(bytes) = payload else {
            return Err("beacon sends frames, not audio".to_owned());
        };
        if self.bits.len() + bytes.len() * 8 > MAX_QUEUED_BITS {
            return Err("beacon backlog full".to_owned());
        }
        self.bits.extend(
            bytes
                .iter()
                .flat_map(|byte| (0..8).rev().map(move |bit| byte >> bit & 1 == 1)),
        );
        Ok(())
    }

    fn generate(&mut self, out: &mut [Complex<f32>]) -> usize {
        let mut written = 0;
        for slot in out {
            if self.left == 0 && !self.next_bit() {
                break;
            }
            *slot = self.sample();
            written += 1;
        }
        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beacon() -> Beacon {
        Beacon::open(INPUT_RATE_HZ, CarrierSettings::default()).expect("opens")
    }

    #[test]
    fn a_frame_keys_one_bit_per_baud() {
        let mut tx = beacon();
        tx.submit(Payload::Frame(&[0b1000_0000])).expect("queues");
        let mut out = vec![Complex::default(); 10_000];
        let written = tx.generate(&mut out);
        assert_eq!(written, 8 * 480);
        assert_eq!(tx.generate(&mut out), 0, "nothing left to send");
    }

    #[test]
    fn a_mark_turns_the_phase_forward_and_a_space_back() {
        let mut tx = beacon();
        tx.submit(Payload::Frame(&[0b1000_0000])).expect("queues");
        let mut out = vec![Complex::default(); 960];
        tx.generate(&mut out);
        let turn = |a: Complex<f32>, b: Complex<f32>| (b * a.conj()).arg();
        assert!(turn(out[10], out[11]) > 0.0);
        assert!(turn(out[500], out[501]) < 0.0);
    }

    #[test]
    fn audio_and_a_narrow_width_are_refused() {
        assert!(beacon().submit(Payload::Audio(&[0.0; 4])).is_err());
        let narrow = CarrierSettings {
            bandwidth_hz: 1_000.0,
            ..CarrierSettings::default()
        };
        assert!(Beacon::open(INPUT_RATE_HZ, narrow).is_err());
    }
}
