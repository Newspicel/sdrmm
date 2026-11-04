use sdrmm_wire::{Capability, DecoderSettings, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text_of;

pub const MEASUREMENT_REPORT_MS: u32 = 1_000;

pub const MIN_ENR_DB: f32 = 1.0;

pub const MAX_ENR_DB: f32 = 40.0;

fn default_enr_db() -> f32 {
    15.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoiseFigureParams {
    #[serde(default = "default_enr_db")]
    pub enr_db: f32,
}

impl Default for NoiseFigureParams {
    fn default() -> Self {
        Self {
            enr_db: default_enr_db(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoiseFigureReport {
    pub window_ms: u32,
    pub level_dbfs: f32,
    pub settled_s: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_on: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot_dbfs: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_dbfs: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nf_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_k: Option<f32>,
}

impl NoiseFigureReport {
    #[must_use]
    pub fn summary(&self) -> String {
        match (self.nf_db, self.y_db) {
            (Some(nf), Some(y)) => format!("NF {nf:.2} dB · Y {y:.2} dB"),
            (None, Some(y)) => format!("Y {y:.2} dB, above the ENR"),
            _ => format!("{:.1} dBFS · toggle the noise source", self.level_dbfs),
        }
    }
}

impl EventPayload for NoiseFigureReport {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let source = self.source_on.map(|on| if on { "on" } else { "off" });
        Presentation {
            fields: fields(&[
                ("Noise figure", number(self.nf_db, Unit::Db, 2)),
                (
                    "Noise temperature",
                    number(self.temperature_k, Unit::Kelvin, 0),
                ),
                ("Y-factor", number(self.y_db, Unit::Db, 2)),
                ("Source on", number(self.hot_dbfs, Unit::Dbfs, 2)),
                ("Source off", number(self.cold_dbfs, Unit::Dbfs, 2)),
                ("Level", number(Some(self.level_dbfs), Unit::Dbfs, 2)),
                ("Source", source.and_then(text_of)),
                ("Settled", number(Some(self.settled_s), Unit::Seconds, 1)),
            ]),
            ..Presentation::of(self.summary())
        }
    }
}

pub(crate) fn noise_figure_kind() -> KindSpec {
    KindSpec::of::<NoiseFigureReport>("Noise figure", "Noise figure", Hue::Muted)
        .caps(&[Capability::Log])
        .build()
}

impl EventContract for NoiseFigureReport {
    const KIND: EventKind = EventKind::named("noise_figure");
}

impl DecoderSettings for NoiseFigureParams {
    const TYPE_ID: &'static str = "noise_figure";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn a_noise_figure_report_shows_the_measured_figures() {
        let view = NoiseFigureReport {
            window_ms: 1000,
            level_dbfs: -60.0,
            settled_s: 2.5,
            source_on: Some(true),
            hot_dbfs: Some(-50.0),
            cold_dbfs: Some(-60.0),
            y_db: Some(10.0),
            nf_db: Some(5.12),
            temperature_k: Some(650.0),
        }
        .present(&EventKind::named("noise_figure"));
        assert_eq!(
            view.field("Noise figure"),
            number(Some(5.12_f32), Unit::Db, 2).as_ref()
        );
        assert_eq!(
            view.field("Noise temperature"),
            number(Some(650.0_f32), Unit::Kelvin, 0).as_ref()
        );
        assert_eq!(view.field("Source"), text_of("on").as_ref());
    }
}
