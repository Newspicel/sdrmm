//! Beacon: sends text as two-tone FSK, the transmit half of the example plugin, declared like a
//! built-in encoder.

use std::collections::VecDeque;

use sdrmm_plugin::{
    Complex, EncoderBase, EncoderError, Modulator, NativeEncoder, NativeModulator, TxPayload,
    decoder::encoder::Carries,
    serde_json::Value,
    wire::{Control, NumberSpec, SettingField},
};
use serde::{Deserialize, Serialize};

use crate::carrier::INPUT_RATE_HZ;

const TYPE_ID: &str = "beacon";
/// The mark tone sits this far above the channel frequency, the space tone as far below.
pub const SHIFT_HZ: f64 = 1_000.0;
const AMPLITUDE: f32 = 0.5;
const MAX_QUEUED_BITS: usize = 8 * 4_096;

const BASE: EncoderBase = EncoderBase {
    type_id: TYPE_ID,
    name: "Beacon",
    summary: "Text as two-tone FSK",
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeaconSettings {
    pub baud: f64,
}

impl Default for BeaconSettings {
    fn default() -> Self {
        Self { baud: 100.0 }
    }
}

/// What the encoder node's message form hands over: the text to key.
#[derive(Deserialize)]
struct Message {
    text: String,
}

/// The encoder as the plugin hands it over.
pub(crate) fn encoder() -> NativeEncoder<BeaconSettings> {
    NativeEncoder::new::<Beacon>(BASE, width_hz)
        .form(form)
        .messages(message_form, compose)
}

/// The mark and space tones and the keying sidebands around them.
pub(crate) fn width_hz(settings: &BeaconSettings) -> f64 {
    2.0 * SHIFT_HZ + 4.0 * settings.baud
}

fn form() -> Vec<SettingField> {
    vec![
        SettingField::new("baud", "Baud", NumberSpec::new(10.0, 300.0, 10.0))
            .title("Bits keyed per second"),
    ]
}

fn message_form() -> Vec<SettingField> {
    vec![SettingField::new("text", "Text", Control::text("CQ")).title("Text the beacon keys")]
}

fn compose(message: &Value) -> Result<TxPayload, EncoderError> {
    let message = Message::deserialize(message)
        .map_err(|error| EncoderError::message(TYPE_ID, error.to_string()))?;
    Ok(TxPayload::Frame(message.text.into_bytes()))
}

pub struct Beacon {
    bits: VecDeque<bool>,
    tone_hz: f64,
    left: usize,
    phase: f64,
    samples_per_bit: usize,
}

impl NativeModulator for Beacon {
    type Settings = BeaconSettings;

    fn rate_hz() -> f64 {
        INPUT_RATE_HZ
    }

    fn new(settings: &BeaconSettings) -> Result<Self, EncoderError> {
        if !(settings.baud.is_finite() && settings.baud > 0.0) {
            return Err(EncoderError::settings(
                TYPE_ID,
                format!("{} baud", settings.baud),
            ));
        }
        Ok(Self {
            bits: VecDeque::new(),
            tone_hz: 0.0,
            left: 0,
            phase: 0.0,
            samples_per_bit: (INPUT_RATE_HZ / settings.baud).round() as usize,
        })
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

impl Modulator for Beacon {
    fn submit(&mut self, payload: TxPayload) -> Result<(), EncoderError> {
        let TxPayload::Frame(bytes) = payload else {
            return Err(EncoderError::WrongPayload {
                encoder: TYPE_ID.to_owned(),
                takes: Carries::Frames,
            });
        };
        if self.bits.len() + bytes.len() * 8 > MAX_QUEUED_BITS {
            return Err(EncoderError::Refused {
                encoder: TYPE_ID.to_owned(),
                reason: "backlog full".to_owned(),
            });
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
    use sdrmm_plugin::{Encoder, decoder::encoder::EncoderParams, serde_json};

    use super::*;

    fn beacon() -> Beacon {
        Beacon::new(&BeaconSettings::default()).expect("opens")
    }

    #[test]
    fn a_frame_keys_one_bit_per_baud() {
        let mut tx = beacon();
        tx.submit(TxPayload::Frame(vec![0b1000_0000]))
            .expect("queues");
        let mut out = vec![Complex::default(); 10_000];
        assert_eq!(tx.generate(&mut out), 8 * 480);
        assert_eq!(tx.generate(&mut out), 0, "nothing left to send");
    }

    #[test]
    fn a_mark_turns_the_phase_forward_and_a_space_back() {
        let mut tx = beacon();
        tx.submit(TxPayload::Frame(vec![0b1000_0000]))
            .expect("queues");
        let mut out = vec![Complex::default(); 960];
        tx.generate(&mut out);
        let turn = |a: Complex<f32>, b: Complex<f32>| (b * a.conj()).arg();
        assert!(turn(out[10], out[11]) > 0.0);
        assert!(turn(out[500], out[501]) < 0.0);
    }

    #[test]
    fn a_filled_message_form_composes_to_its_text() {
        let composed = encoder().compose(&serde_json::json!({"text": "A"}));
        assert!(matches!(composed, Ok(TxPayload::Frame(bytes)) if bytes == b"A"));
    }

    #[test]
    fn the_width_follows_the_baud() {
        let params = EncoderParams::new(TYPE_ID, &BeaconSettings { baud: 50.0 }).expect("params");
        assert_eq!(encoder().width_hz(&params).expect("a width"), 2_200.0);
    }

    #[test]
    fn audio_and_a_dead_baud_are_refused() {
        assert!(beacon().submit(TxPayload::Audio(vec![0.0; 4])).is_err());
        assert!(Beacon::new(&BeaconSettings { baud: 0.0 }).is_err());
    }
}
