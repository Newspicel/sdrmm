use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;

fn default_gnss_prn() -> u8 {
    1
}

fn default_gnss_doppler_hz() -> u32 {
    10_000
}

fn default_gnss_threshold() -> f32 {
    4.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GnssParams {
    #[serde(default = "default_gnss_prn")]
    pub prn: u8,
    #[serde(default = "default_gnss_doppler_hz")]
    pub doppler_hz: u32,
    #[serde(default = "default_gnss_threshold")]
    pub threshold: f32,
}

impl Default for GnssParams {
    fn default() -> Self {
        Self {
            prn: default_gnss_prn(),
            doppler_hz: default_gnss_doppler_hz(),
            threshold: default_gnss_threshold(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GnssFrame {
    pub prn: u8,
    pub doppler_hz: f32,
    pub code_phase_chips: f32,
    pub cn0_db_hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subframe: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tow_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub week: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<String>,
}

impl EventPayload for GnssFrame {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: Some(format!("GPS-{}", self.prn)),
            fields: gnss_fields(self),
            ..Presentation::of(gnss_summary(self))
        }
    }
}

fn gnss_fields(g: &GnssFrame) -> Vec<Field> {
    let subframe = g
        .subframe
        .map_or_else(|| text_of("acquiring"), |id| count(Some(id)));
    fields(&[
        (
            "signal",
            "Signal",
            text_of(format!("GPS L1 C/A PRN {}", g.prn)),
        ),
        (
            "doppler",
            "Doppler",
            number(Some(g.doppler_hz), Unit::Hz, 0),
        ),
        (
            "code_phase",
            "Code phase",
            number(Some(g.code_phase_chips), Unit::Chips, 2),
        ),
        ("c_n0", "C/N0", number(Some(g.cn0_db_hz), Unit::DbHz, 1)),
        ("subframe", "Subframe", subframe),
        (
            "time_of_week",
            "Time of week",
            number(g.tow_seconds, Unit::Seconds, 0),
        ),
        ("week", "Week", count(g.week)),
        ("words", "Words", text(Some(&g.words.join(" ")))),
    ])
}

fn gnss_summary(g: &GnssFrame) -> String {
    let mut parts = vec![
        format!("GPS PRN {}", g.prn),
        format!("{:+.0} Hz", g.doppler_hz),
        format!("{:.1} dB-Hz", g.cn0_db_hz),
    ];
    if let Some(id) = g.subframe {
        parts.push(format!("subframe {id}"));
    } else {
        parts.push("acquired".to_owned());
    }
    if let Some(tow) = g.tow_seconds {
        parts.push(format!("TOW {tow} s"));
    }
    joined(parts)
}

pub(crate) fn gnss_kind() -> KindSpec {
    KindSpec::of::<GnssFrame>("GNSS lab", "Satellites", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

impl EventContract for GnssFrame {
    const KIND: EventKind = EventKind::named("gnss");
}

impl sdrmm_wire::DecoderSettings for GnssParams {
    const TYPE_ID: &'static str = "gnss";
}
