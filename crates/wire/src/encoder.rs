use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use crate::SettingField;

/// What an encoder turns into a signal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EncoderInput {
    /// An audio wire, keyed by a microphone.
    #[default]
    Audio,
    /// Messages the operator fills into the `message` form and sends.
    Message,
}

/// One mode the server can transmit, built in or loaded as a plugin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EncoderDescriptor {
    pub type_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    /// The baseband rate the encoder produces.
    pub rate_hz: f64,
    /// The width the signal stays inside at the default settings.
    pub bandwidth_hz: f64,
    #[serde(default)]
    pub input: EncoderInput,
    /// The form of one message, in order. Only a message encoder has one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message: Vec<SettingField>,
    /// The form an encoder node renders for its settings, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingField>,
    /// What a fresh encoder starts on.
    #[serde(default = "empty_object")]
    #[schema(value_type = std::collections::BTreeMap<String, Value>)]
    pub defaults: Value,
}

impl Default for EncoderDescriptor {
    fn default() -> Self {
        Self {
            type_id: String::new(),
            name: String::new(),
            summary: String::new(),
            rate_hz: 0.0,
            bandwidth_hz: 0.0,
            input: EncoderInput::default(),
            message: Vec::new(),
            settings: Vec::new(),
            defaults: empty_object(),
        }
    }
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EncoderTypesResponse {
    pub types: Vec<EncoderDescriptor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_descriptor_takes_audio_and_starts_on_no_settings() {
        let back: EncoderDescriptor = serde_json::from_value(serde_json::json!({
            "type_id": "probe",
            "name": "Probe",
            "rate_hz": 48_000.0,
            "bandwidth_hz": 2_000.0,
        }))
        .expect("descriptor");
        assert_eq!(back.input, EncoderInput::Audio);
        assert!(back.message.is_empty());
        assert_eq!(back.defaults, serde_json::json!({}));
    }
}
