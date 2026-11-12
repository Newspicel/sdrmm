use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::channel::ChannelParams;

pub const DEFAULT_TRANSMIT_TIMEOUT_S: u32 = 180;
pub const MAX_TRANSMIT_TIMEOUT_S: u32 = 3_600;
pub const DEFAULT_TRANSMIT_LEVEL_DB: f64 = -3.0;
pub const MIN_TRANSMIT_LEVEL_DB: f64 = -60.0;
pub const MICROPHONE_RATE_HZ: u32 = 48_000;
pub const MAX_MICROPHONE_FRAME: usize = 4_800;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct TransmitSettings {
    /// Where the signal lands on the air. Unset, a modulator sits on the radio's centre and a
    /// lane where it was received.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_hz: Option<f64>,
    /// Digital drive below full scale.
    pub level_db: f64,
    /// The longest one transmission may last before the transmitter disarms itself.
    pub timeout_s: u32,
}

impl Default for TransmitSettings {
    fn default() -> Self {
        Self {
            frequency_hz: None,
            level_db: DEFAULT_TRANSMIT_LEVEL_DB,
            timeout_s: DEFAULT_TRANSMIT_TIMEOUT_S,
        }
    }
}

impl TransmitSettings {
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        if let Some(frequency_hz) = self.frequency_hz
            && !(frequency_hz.is_finite() && frequency_hz > 0.0)
        {
            return Some(format!("frequency {frequency_hz} Hz is not a frequency"));
        }
        if !(MIN_TRANSMIT_LEVEL_DB..=0.0).contains(&self.level_db) {
            return Some(format!(
                "level {} dB outside {MIN_TRANSMIT_LEVEL_DB} to 0 dB",
                self.level_db
            ));
        }
        if !(1..=MAX_TRANSMIT_TIMEOUT_S).contains(&self.timeout_s) {
            return Some(format!(
                "timeout {} s outside 1 to {MAX_TRANSMIT_TIMEOUT_S} s",
                self.timeout_s
            ));
        }
        None
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TransmitNode {
    #[serde(flatten)]
    pub settings: TransmitSettings,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModulatorNode {
    pub channel_type: String,
    /// The decoder's own settings object, absent fields taking its defaults.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[schema(value_type = std::collections::BTreeMap<String, Value>)]
    pub settings: Value,
}

impl ModulatorNode {
    #[must_use]
    pub fn params(&self) -> ChannelParams {
        match &self.settings {
            Value::Object(_) => ChannelParams {
                type_id: self.channel_type.clone(),
                settings: self.settings.clone(),
            },
            _ => ChannelParams::bare(&self.channel_type),
        }
    }
}

pub const DEFAULT_VOX_THRESHOLD_DB: f64 = -35.0;
pub const DEFAULT_VOX_HANG_MS: u32 = 600;
pub const MAX_VOX_HANG_MS: u32 = 5_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct MicrophoneNode {
    /// Keys by itself while the microphone is louder than the threshold.
    pub vox: bool,
    /// Peak level, in dB below full scale, that opens the voice key.
    pub threshold_db: f64,
    /// How long the voice key stays open after the level falls.
    pub hang_ms: u32,
}

impl Default for MicrophoneNode {
    fn default() -> Self {
        Self {
            vox: false,
            threshold_db: DEFAULT_VOX_THRESHOLD_DB,
            hang_ms: DEFAULT_VOX_HANG_MS,
        }
    }
}

impl MicrophoneNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        (-90.0..=0.0).contains(&self.threshold_db) && self.hang_ms <= MAX_VOX_HANG_MS
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn sextet(symbol: u8) -> Option<u32> {
    BASE64
        .iter()
        .position(|known| *known == symbol)
        .map(|index| index as u32)
}

fn base64_octets(encoded: &str) -> Option<Vec<u8>> {
    let symbols = encoded.trim_end_matches('=').as_bytes();
    if !encoded.len().is_multiple_of(4) || encoded.len() - symbols.len() > 2 {
        return None;
    }
    let mut octets = Vec::with_capacity(symbols.len() * 3 / 4);
    for group in symbols.chunks(4) {
        let mut word = 0u32;
        for symbol in group {
            word = word << 6 | sextet(*symbol)?;
        }
        word <<= 6 * (4 - group.len());
        let whole = [(word >> 16) as u8, (word >> 8) as u8, word as u8];
        octets.extend_from_slice(&whole[..group.len().saturating_sub(1)]);
    }
    Some(octets)
}

/// The samples of one `Speak` command: base64 of mono 16-bit little-endian PCM at
/// `MICROPHONE_RATE_HZ`. `None` when it is malformed or longer than `MAX_MICROPHONE_FRAME`.
#[must_use]
pub fn microphone_pcm(encoded: &str) -> Option<Vec<f32>> {
    if encoded.len() > MAX_MICROPHONE_FRAME * 2 * 4 / 3 + 4 {
        return None;
    }
    let octets = base64_octets(encoded)?;
    let (pairs, odd) = octets.as_chunks::<2>();
    if !odd.is_empty() || pairs.len() > MAX_MICROPHONE_FRAME {
        return None;
    }
    Some(
        pairs
            .iter()
            .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32_768.0)
            .collect(),
    )
}

/// What `microphone_pcm` reads back, as a client writes it.
#[must_use]
pub fn encode_microphone_pcm(pcm: &[f32]) -> String {
    let octets: Vec<u8> = pcm
        .iter()
        .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16).to_le_bytes())
        .collect();
    let mut encoded = String::with_capacity(octets.len().div_ceil(3) * 4);
    for group in octets.chunks(3) {
        let word = group
            .iter()
            .fold(0u32, |word, octet| word << 8 | u32::from(*octet))
            << (8 * (3 - group.len()));
        for index in 0..4 {
            if index <= group.len() {
                encoded.push(BASE64[(word >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum TransmitAction {
    Arm,
    Disarm,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModulatorMessage {
    /// One message, shaped by the form its decoder declares.
    #[schema(value_type = std::collections::BTreeMap<String, Value>)]
    pub message: Value,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransmitPhase {
    /// Armed and waiting for its source.
    #[default]
    Idle,
    OnAir,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TransmitStatus {
    pub node: String,
    pub phase: TransmitPhase,
    pub frequency_hz: f64,
    pub sample_rate: f64,
    pub settings: TransmitSettings,
    /// Samples handed to the radio since arming.
    pub samples: u64,
    pub bursts: u64,
    /// Times the source ran dry in the middle of a transmission.
    pub underruns: u64,
    /// Peak drive of the latest transmission, in dB below full scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_db: Option<f32>,
    /// The width that holds 99 % of the latest transmission's power.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupied_hz: Option<f64>,
    /// The width the modulator's mode is meant to stay inside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_a_backed_off_level_and_refuse_what_they_cannot_hold() {
        let settings: TransmitSettings = serde_json::from_str("{}").expect("defaults");
        assert_eq!(settings, TransmitSettings::default());
        assert_eq!(settings.problem(), None);
        for bad in [
            TransmitSettings {
                level_db: 1.0,
                ..TransmitSettings::default()
            },
            TransmitSettings {
                timeout_s: 0,
                ..TransmitSettings::default()
            },
            TransmitSettings {
                frequency_hz: Some(-5.0),
                ..TransmitSettings::default()
            },
        ] {
            assert!(bad.problem().is_some(), "{bad:?}");
        }
    }

    #[test]
    fn a_modulator_without_settings_runs_its_decoders_defaults() {
        let node: ModulatorNode =
            serde_json::from_value(serde_json::json!({"channel_type": "nfm"})).expect("parsed");
        assert_eq!(node.params(), ChannelParams::bare("nfm"));
        let set: ModulatorNode = serde_json::from_value(serde_json::json!({
            "channel_type": "aprs", "settings": {"mode": "afsk1200"}
        }))
        .expect("parsed");
        assert_eq!(set.params().settings["mode"], "afsk1200");
    }

    #[test]
    fn spoken_samples_survive_the_command_and_a_malformed_one_is_dropped() {
        for len in [0usize, 1, 2, 3, 4, 960] {
            let pcm: Vec<f32> = (0..len).map(|n| (n as f32 / 7.0).sin() * 0.9).collect();
            let encoded = encode_microphone_pcm(&pcm);
            assert_eq!(encoded.len() % 4, 0, "{len} samples");
            let back = microphone_pcm(&encoded).expect("decoded");
            assert_eq!(back.len(), len);
            for (sent, got) in pcm.iter().zip(&back) {
                assert!((sent - got).abs() < 1e-4, "{sent} came back {got}");
            }
        }
        assert_eq!(encode_microphone_pcm(&[0.0, 1.0]), "AAD/fw==");
        assert_eq!(
            microphone_pcm("AAD/fw=="),
            Some(vec![0.0, 32_767.0 / 32_768.0])
        );
        for malformed in ["AAD", "AA*/fw==", "AAAA====", "AA=="] {
            assert_eq!(microphone_pcm(malformed), None, "{malformed}");
        }
        let long = encode_microphone_pcm(&vec![0.0; MAX_MICROPHONE_FRAME + 1]);
        assert_eq!(microphone_pcm(&long), None, "too long a frame");
        let most = encode_microphone_pcm(&vec![0.0; MAX_MICROPHONE_FRAME]);
        assert_eq!(
            microphone_pcm(&most).map(|pcm| pcm.len()),
            Some(MAX_MICROPHONE_FRAME)
        );
    }

    #[test]
    fn a_microphone_defaults_to_the_key_and_refuses_a_voice_key_out_of_range() {
        let node: MicrophoneNode = serde_json::from_str("{}").expect("defaults");
        assert_eq!(node, MicrophoneNode::default());
        assert!(!node.vox);
        assert!(node.valid());
        for bad in [
            MicrophoneNode {
                threshold_db: 3.0,
                ..MicrophoneNode::default()
            },
            MicrophoneNode {
                hang_ms: MAX_VOX_HANG_MS + 1,
                ..MicrophoneNode::default()
            },
        ] {
            assert!(!bad.valid(), "{bad:?}");
        }
    }

    #[test]
    fn actions_name_what_they_do() {
        assert_eq!(
            serde_json::to_value(TransmitAction::Arm).expect("json"),
            serde_json::json!({"do": "arm"})
        );
    }
}
