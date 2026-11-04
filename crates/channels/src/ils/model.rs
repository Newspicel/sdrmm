use sdrmm_wire::{Capability, DecoderSettings, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use crate::vor::default_navaid_report_ms;
use sdrmm_wire::EventKind;

use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text_of;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IlsComponent {
    #[default]
    Localizer,
    Glideslope,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IlsParams {
    #[serde(default)]
    pub component: IlsComponent,
    #[serde(default = "default_navaid_report_ms")]
    pub report_ms: u32,
}

impl Default for IlsParams {
    fn default() -> Self {
        Self {
            component: IlsComponent::default(),
            report_ms: default_navaid_report_ms(),
        }
    }
}

labelled! {
    IlsComponent { Localizer => "Localizer", Glideslope => "Glideslope" }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IlsReading {
    pub component: IlsComponent,
    pub modulation_90: f32,
    pub modulation_150: f32,
    pub ddm: f32,
    pub deviation_dots: f32,
    pub signal_db: f32,
}

/// The depth either guidance tone needs before a reading is a localizer or glide slope at all.
const MIN_TONE_DEPTH: f32 = 0.05;

impl EventPayload for IlsReading {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            fields: ils_fields(self),
            verified: Some(
                self.modulation_90 >= MIN_TONE_DEPTH && self.modulation_150 >= MIN_TONE_DEPTH,
            ),
            ..Presentation::of(format!(
                "{} · {:+.3} DDM · {:+.2} dots",
                ils_component(self.component),
                self.ddm,
                self.deviation_dots
            ))
        }
    }
}

fn ils_component(component: IlsComponent) -> &'static str {
    match component {
        IlsComponent::Localizer => "localizer",
        IlsComponent::Glideslope => "glideslope",
    }
}

fn ils_fields(i: &IlsReading) -> Vec<Field> {
    fields(&[
        ("Component", text_of(ils_component(i.component))),
        (
            "90 Hz",
            number(Some(i.modulation_90 * 100.0), Unit::Percent, 2),
        ),
        (
            "150 Hz",
            number(Some(i.modulation_150 * 100.0), Unit::Percent, 2),
        ),
        ("DDM", number(Some(i.ddm), Unit::None, 4)),
        ("Dots", number(Some(i.deviation_dots), Unit::None, 2)),
        ("Signal", number(Some(i.signal_db), Unit::Db, 1)),
    ])
}

pub(crate) fn ils_kind() -> KindSpec {
    KindSpec::of::<IlsReading>("ILS", "ILS", Hue::Muted)
        .caps(&[Capability::Log])
        .allocated()
        .build()
}

impl DecoderSettings for IlsParams {
    const TYPE_ID: &'static str = "ils";
}

impl EventContract for IlsReading {
    const KIND: EventKind = EventKind::named("ils");
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::present::text;

    #[test]
    fn ils_names_component_and_modulation_depths() {
        let reading = IlsReading {
            component: IlsComponent::Glideslope,
            modulation_90: 0.4,
            modulation_150: 0.38,
            ddm: 0.02,
            deviation_dots: -0.25,
            signal_db: 12.0,
        };
        let view = reading.present(&EventKind::named("ils"));
        assert_eq!(view.field("Component"), text(Some("glideslope")).as_ref());
        assert_eq!(
            view.field("90 Hz"),
            number(Some(0.4_f32 * 100.0), Unit::Percent, 2).as_ref()
        );
        assert_eq!(
            view.field("Dots"),
            number(Some(-0.25), Unit::None, 2).as_ref()
        );
    }
}
