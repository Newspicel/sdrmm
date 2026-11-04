use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::number;
use sdrmm_wire::present::percent;
use sdrmm_wire::present::text_of;

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

pub const CTCSS_TONES_HZ: [f64; 50] = [
    67.0, 69.3, 71.9, 74.4, 77.0, 79.7, 82.5, 85.4, 88.5, 91.5, 94.8, 97.4, 100.0, 103.5, 107.2,
    110.9, 114.8, 118.8, 123.0, 127.3, 131.8, 136.5, 141.3, 146.2, 151.4, 156.7, 159.8, 162.2,
    165.5, 167.9, 171.3, 173.8, 177.3, 179.9, 183.5, 186.2, 189.9, 192.8, 196.6, 199.5, 203.5,
    206.5, 210.7, 218.1, 225.7, 229.1, 233.6, 241.8, 250.3, 254.1,
];

pub const DCS_CODES: [u16; 83] = [
    23, 25, 26, 31, 32, 43, 47, 51, 54, 65, 71, 72, 73, 74, 114, 115, 116, 125, 131, 132, 134, 143,
    152, 155, 156, 162, 165, 172, 174, 205, 223, 226, 243, 244, 245, 251, 261, 263, 265, 271, 306,
    311, 315, 331, 343, 346, 351, 364, 365, 371, 411, 412, 413, 423, 431, 432, 445, 464, 465, 466,
    503, 506, 516, 532, 546, 565, 606, 612, 624, 627, 631, 632, 654, 662, 664, 703, 712, 723, 731,
    732, 734, 743, 754,
];

labelled! {
    NfmToneMode { Off => "off", Detect => "Detect", Ctcss => "CTCSS", Dcs => "DCS" }
}

labelled! {
    NfmScramblerMode { Off => "off", Inversion => "Inversion", Auto => "Auto" }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToneSquelchStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctcss_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcs_code: Option<u16>,
    pub open: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScramblerStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inversion_hz: Option<f64>,
    pub confidence: f64,
}

impl EventPayload for ToneSquelchStatus {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let mut parts = Vec::new();
        if let Some(hz) = self.ctcss_hz {
            parts.push(format!("CTCSS {hz:.1} Hz"));
        }
        if let Some(code) = self.dcs_code {
            parts.push(format!("DCS {code:03}"));
        }
        if parts.is_empty() {
            parts.push("no tone".to_owned());
        }
        let squelch = if self.open { "open" } else { "muted" };
        parts.push(squelch.to_owned());
        Presentation {
            fields: fields(&[
                ("CTCSS", number(self.ctcss_hz, Unit::Hz, 1)),
                (
                    "DCS",
                    self.dcs_code.and_then(|code| text_of(format!("{code:03}"))),
                ),
                ("Audio", text_of(squelch)),
            ]),
            verified: Some(false),
            ..Presentation::of(joined(parts))
        }
    }
}

impl EventPayload for ScramblerStatus {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let found = self.inversion_hz.is_some();
        Presentation {
            fields: fields(&[
                ("Inversion", number(self.inversion_hz, Unit::Hz, 0)),
                (
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

impl EventContract for ToneSquelchStatus {
    const KIND: EventKind = EventKind::named("tone");
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
