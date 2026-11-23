use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    ModelArtifact, ModelState,
    channel::{Control, ParamOption, SettingField},
    present::{Field, Unit, fields, number},
};

pub const SPEECH_MODELS_PREFIX: &str = "speech/v1";
pub const SPEECH_MODELS_URL: &str = "https://downloads.sdrmm.com/speech/v1";

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    ToSchema,
)]
pub enum SpeechModel {
    /// NVIDIA Parakeet TDT 0.6B v3: 25 European languages, CC-BY-4.0.
    #[default]
    #[serde(rename = "parakeet-tdt-0.6b-v3")]
    ParakeetTdtV3,
    /// Parakeet v3 fine-tuned on air traffic control English, MIT on top of CC-BY-4.0.
    #[serde(rename = "parakeet-tdt-0.6b-v3-atc")]
    ParakeetTdtV3Atc,
}

impl SpeechModel {
    pub const ALL: [Self; 2] = [Self::ParakeetTdtV3, Self::ParakeetTdtV3Atc];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ParakeetTdtV3 => "parakeet-tdt-0.6b-v3",
            Self::ParakeetTdtV3Atc => "parakeet-tdt-0.6b-v3-atc",
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ParakeetTdtV3 => "Parakeet v3",
            Self::ParakeetTdtV3Atc => "Parakeet v3 ATC",
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::ParakeetTdtV3 => "25 European languages",
            Self::ParakeetTdtV3Atc => "Air traffic control English",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.name() == name)
    }

    #[must_use]
    pub fn file_name(self) -> String {
        format!("{}.sdrmmnn", self.name())
    }

    #[must_use]
    pub fn url(self) -> String {
        format!("{SPEECH_MODELS_URL}/{}", self.file_name())
    }

    #[must_use]
    pub fn artifact(self) -> ModelArtifact {
        let (bytes, sha256) = match self {
            Self::ParakeetTdtV3 => (
                1_279_593_366,
                "5c419ee72b9ec4e24210c400a3ee7c4c94c68592fa006e7336a64443ce2da8d4",
            ),
            Self::ParakeetTdtV3Atc => (
                1_279_593_429,
                "00a1a21a242fa78b4c2ba252d87699e607925aa829397d446662d3143ea6c47a",
            ),
        };
        ModelArtifact { bytes, sha256 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SpeechModelStatus {
    pub model: SpeechModel,
    pub bytes: u64,
    #[serde(flatten)]
    pub state: ModelState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SpeechModelsResponse {
    pub models: Vec<SpeechModelStatus>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct TranscriberParams {
    pub model: SpeechModel,
}

impl TranscriberParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        vec![
            SettingField::new(
                "model",
                "Model",
                Control::Choice {
                    options: SpeechModel::ALL
                        .into_iter()
                        .map(|model| ParamOption::new(model, model.label()).title(model.title()))
                        .collect(),
                },
            )
            .title("The speech model that writes the text"),
        ]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TranscriberNode {
    #[serde(default)]
    pub settings: TranscriberParams,
}

/// A transcriber's work so far, with the readouts its face shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TranscriberStatus {
    pub node: String,
    pub model: SpeechModel,
    pub queued: u64,
    pub done: u64,
    pub dropped: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readouts: Vec<Field>,
}

impl TranscriberStatus {
    #[must_use]
    pub fn with_readouts(mut self) -> Self {
        self.readouts = fields(&[
            (
                "queued",
                "Queued",
                number(Some(self.queued as f64), Unit::None, 0),
            ),
            (
                "done",
                "Done",
                number(Some(self.done as f64), Unit::None, 0),
            ),
            (
                "dropped",
                "Dropped",
                number(Some(self.dropped as f64), Unit::None, 0),
            ),
            (
                "took",
                "Took",
                number(
                    Some(self.took_ms.unwrap_or(0) as f64 / 1_000.0),
                    Unit::Seconds,
                    1,
                ),
            ),
        ]);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_downloads_from_the_speech_prefix() {
        for model in SpeechModel::ALL {
            assert!(model.url().starts_with(SPEECH_MODELS_URL));
            assert!(SPEECH_MODELS_URL.ends_with(SPEECH_MODELS_PREFIX));
            assert_eq!(SpeechModel::from_name(model.name()), Some(model));
        }
    }

    #[test]
    fn the_face_keeps_every_readout_before_the_first_call() {
        let status = TranscriberStatus::default().with_readouts();
        let keys: Vec<&str> = status.readouts.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["queued", "done", "dropped", "took"]);
    }

    #[test]
    fn the_model_dropdown_offers_every_model() {
        let form = TranscriberParams::form();
        let Control::Choice { options } = &form[0].control else {
            panic!("model is a dropdown");
        };
        assert_eq!(options.len(), SpeechModel::ALL.len());
    }
}
