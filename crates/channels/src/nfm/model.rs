use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, ToneSquelchStatus};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::percent;

fn default_nfm_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NfmToneMode {
    #[default]
    Off,
    Detect,
    Ctcss,
    Dcs,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NfmScramblerMode {
    #[default]
    Off,
    Inversion,
    Auto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NfmParams {
    #[serde(default = "default_nfm_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default)]
    pub tone_mode: NfmToneMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctcss_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcs_code: Option<u16>,
    #[serde(default)]
    pub scrambler_mode: NfmScramblerMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inversion_hz: Option<f64>,
    #[serde(default)]
    pub compander: bool,
}

impl Default for NfmParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_nfm_bandwidth_hz(),
            tone_mode: NfmToneMode::default(),
            ctcss_hz: None,
            dcs_code: None,
            scrambler_mode: NfmScramblerMode::default(),
            inversion_hz: None,
            compander: false,
        }
    }
}

labelled! {
    NfmToneMode { Off => "off", Detect => "Detect", Ctcss => "CTCSS", Dcs => "DCS" }
}

labelled! {
    NfmScramblerMode { Off => "off", Inversion => "Inversion", Auto => "Auto" }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScramblerStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inversion_hz: Option<f64>,
    pub confidence: f64,
}

impl EventPayload for ScramblerStatus {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let found = self.inversion_hz.is_some();
        Presentation {
            fields: fields(&[
                (
                    "inversion",
                    "Inversion",
                    number(self.inversion_hz, Unit::Hz, 0),
                ),
                (
                    "confidence",
                    "Confidence",
                    percent(self.confidence as f32).filter(|_| found),
                ),
            ]),
            verified: Some(false),
            ..Presentation::of(match self.inversion_hz {
                Some(hz) => format!(
                    "inversion {hz:.0} Hz · {:.0}% confidence",
                    self.confidence * 100.0
                ),
                None => "no inversion".to_owned(),
            })
        }
    }
}

pub(crate) fn tone_kind() -> KindSpec {
    KindSpec::of::<ToneSquelchStatus>("Tone", "Tones", Hue::Muted)
        .caps(&[Capability::Log])
        .build()
}

pub(crate) fn scrambler_kind() -> KindSpec {
    KindSpec::of::<ScramblerStatus>("Scrambler", "Scramblers", Hue::Muted)
        .caps(&[Capability::Log])
        .build()
}

impl EventContract for ScramblerStatus {
    const KIND: EventKind = EventKind::named("scrambler");
}

impl sdrmm_wire::DecoderSettings for NfmParams {
    const TYPE_ID: &'static str = "nfm";
}
