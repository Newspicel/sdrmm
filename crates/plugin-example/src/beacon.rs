//! Beacon: sends frames as two-tone FSK, the transmit half of the example plugin.

use std::collections::VecDeque;

use sdrmm_plugin::{
    Complex, Payload, Transmitter, serde_json,
    wire::{Control, EncoderDescriptor, EncoderInput, SettingField},
};
use serde::{Deserialize, Serialize};

use crate::INPUT_RATE_HZ;

/// The mark tone sits this far above the channel frequency, the space tone as far below.
pub const SHIFT_HZ: f64 = 1_000.0;
pub const BAUD: f64 = 100.0;
const AMPLITUDE: f32 = 0.5;
const MAX_QUEUED_BITS: usize = 8 * 4_096;

/// What the encoder node's message form hands over: the text to key.
#[derive(Deserialize)]
struct Message {
    text: String,
}

/// The octets a frame keys: the text of a filled message form, or the frame as it is.
fn keyed(frame: &[u8]) -> Vec<u8> {
    serde_json::from_slice::<Message>(frame)
        .map_or_else(|_| frame.to_vec(), |message| message.text.into_bytes())
}

/// The encoder the example declares in its manifest.
#[must_use]
pub fn descriptor() -> EncoderDescriptor {
    EncoderDescriptor {
        type_id: "beacon".to_owned(),
        name: "Beacon".to_owned(),
        summary: "Text as two-tone FSK".to_owned(),
        rate_hz: INPUT_RATE_HZ,
        bandwidth_hz: WIDTH_HZ,
        input: EncoderInput::Message,
        message: vec![
            SettingField::new("text", "Text", Control::text("CQ")).title("Text the beacon keys"),
        ],
        ..EncoderDescriptor::default()
    }
}

pub struct Beacon {
    bits: VecDeque<bool>,
    tone_hz: f64,
    left: usize,
    phase: f64,
    samples_per_bit: usize,
}

/// The beacon keys at a fixed shift and rate, so it has nothing to set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BeaconSettings {}

/// The mark and space tones and the keying sidebands around them.
pub const WIDTH_HZ: f64 = 2.0 * SHIFT_HZ + 4.0 * BAUD;

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
    type Settings = BeaconSettings;

    fn open(rate_hz: f64, _settings: BeaconSettings) -> Result<Self, String> {
        if rate_hz != INPUT_RATE_HZ {
            return Err(format!("beacon sends at {INPUT_RATE_HZ} Hz, got {rate_hz}"));
        }
        Ok(Self {
            bits: VecDeque::new(),
            tone_hz: 0.0,
            left: 0,
            phase: 0.0,
            samples_per_bit: (rate_hz / BAUD).round() as usize,
        })
    }

    fn submit(&mut self, payload: Payload<'_>) -> Result<(), String> {
        let Payload::Frame(frame) = payload else {
            return Err("beacon sends frames, not audio".to_owned());
        };
        let bytes = keyed(frame);
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
        Beacon::open(INPUT_RATE_HZ, BeaconSettings::default()).expect("opens")
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
    fn a_filled_message_form_keys_its_text_and_nothing_else() {
        let mut tx = beacon();
        tx.submit(Payload::Frame(br#"{"text":"A"}"#))
            .expect("queues");
        let mut out = vec![Complex::default(); 10_000];
        assert_eq!(tx.generate(&mut out), 8 * 480, "one octet for one letter");
    }

    #[test]
    fn audio_and_another_rate_are_refused() {
        assert!(beacon().submit(Payload::Audio(&[0.0; 4])).is_err());
        assert!(Beacon::open(96_000.0, BeaconSettings::default()).is_err());
    }
}
