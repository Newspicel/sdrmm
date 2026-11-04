use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::named;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::FieldValue;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::flag;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::number;
use sdrmm_wire::present::only_if;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioClockStandard {
    #[default]
    Dcf77,
    Wwvb,
    Msf,
    Jjy,
}

impl RadioClockStandard {
    pub const ALL: [Self; 4] = [Self::Dcf77, Self::Wwvb, Self::Msf, Self::Jjy];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dcf77 => "DCF77",
            Self::Wwvb => "WWVB",
            Self::Msf => "MSF",
            Self::Jjy => "JJY",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RadioClockParams {
    #[serde(default)]
    pub standard: RadioClockStandard,
    #[serde(default)]
    pub invert: bool,
}

named!(RadioClockStandard);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadioClockFrame {
    pub standard: RadioClockStandard,
    pub datetime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_offset_minutes: Option<i16>,
    #[serde(default)]
    pub dst: bool,
    #[serde(default)]
    pub leap_warning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dut1_seconds: Option<f32>,
    pub symbols: String,
}

impl EventPayload for RadioClockFrame {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let standard = self.standard.label();
        let mut parts = vec![standard.to_owned(), self.datetime.clone()];
        if self.leap_warning {
            parts.push("leap warning".to_owned());
        }
        Presentation {
            entity: Some(standard.to_owned()),
            fields: radio_clock_fields(self, standard),
            ..Presentation::of(joined(parts))
        }
    }
}

fn radio_clock_fields(r: &RadioClockFrame, standard: &str) -> Vec<Field> {
    fields(&[
        ("service", "Service", text_of(standard)),
        ("civil_time", "Civil time", text(Some(&r.datetime))),
        (
            "utc_offset",
            "UTC offset",
            r.utc_offset_minutes.and_then(utc_offset),
        ),
        ("dst", "DST", flag(Some(r.dst))),
        ("leap_warning", "Leap warning", only_if(r.leap_warning)),
        ("dut1", "DUT1", number(r.dut1_seconds, Unit::Seconds, 1)),
        ("symbols", "Symbols", text(Some(&r.symbols))),
    ])
}

fn utc_offset(minutes: i16) -> Option<FieldValue> {
    let sign = if minutes < 0 { '-' } else { '+' };
    let absolute = minutes.unsigned_abs();
    text_of(format!(
        "UTC{sign}{:02}:{:02}",
        absolute / 60,
        absolute % 60
    ))
}

pub(crate) fn radio_clock_kind() -> KindSpec {
    KindSpec::of::<RadioClockFrame>("Radio clock", "Clocks", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

impl EventContract for RadioClockFrame {
    const KIND: EventKind = EventKind::named("radio_clock");
}

impl sdrmm_wire::DecoderSettings for RadioClockParams {
    const TYPE_ID: &'static str = "radio_clock";
}

#[cfg(test)]
mod model_tests {
    use sdrmm_wire::DecoderEvent;

    use super::*;
    use crate::gnss::GnssFrame;

    fn field<'a>(view: &'a Presentation, label: &str) -> Option<&'a FieldValue> {
        view.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| &field.value)
    }

    #[test]
    fn clock_and_gnss_summaries_keep_the_measurements_used_by_the_live_log() {
        crate::registry::init();
        let clock = DecoderEvent::of(&RadioClockFrame {
            standard: RadioClockStandard::Dcf77,
            datetime: "2026-08-15T12:34:00+02:00".to_owned(),
            utc_offset_minutes: Some(120),
            dst: true,
            leap_warning: false,
            dut1_seconds: None,
            symbols: String::new(),
        });
        assert_eq!(clock.present().summary, "DCF77 · 2026-08-15T12:34:00+02:00");
        assert_eq!(clock.present().entity.as_deref(), Some("DCF77"));

        let gnss = DecoderEvent::of(&GnssFrame {
            prn: 7,
            doppler_hz: 1_000.0,
            code_phase_chips: 158.34,
            cn0_db_hz: 44.5,
            subframe: None,
            tow_seconds: None,
            week: None,
            words: Vec::new(),
        });
        assert_eq!(
            gnss.present().summary,
            "GPS PRN 7 · +1000 Hz · 44.5 dB-Hz · acquired"
        );
        assert_eq!(gnss.present().entity.as_deref(), Some("GPS-7"));
    }

    #[test]
    fn a_radio_clock_frame_shows_offset_and_dut1() {
        let view = RadioClockFrame {
            standard: RadioClockStandard::Dcf77,
            datetime: "2026-03-29 03:00".to_owned(),
            utc_offset_minutes: Some(120),
            dst: true,
            leap_warning: false,
            dut1_seconds: Some(-0.2),
            symbols: "0101".to_owned(),
        }
        .present(&RadioClockFrame::KIND);
        assert_eq!(field(&view, "Service"), text_of("DCF77").as_ref());
        assert_eq!(field(&view, "UTC offset"), text_of("UTC+02:00").as_ref());
        assert_eq!(field(&view, "DST"), flag(Some(true)).as_ref());
        assert_eq!(field(&view, "Leap warning"), None);
        assert_eq!(utc_offset(-210), text_of("UTC-03:30"));
    }
}
