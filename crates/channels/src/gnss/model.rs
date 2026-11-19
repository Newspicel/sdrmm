use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, LatLon, Symbol};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Place;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::number;
use sdrmm_wire::present::position;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;

fn default_gnss_doppler_hz() -> u32 {
    7_000
}

fn default_gnss_threshold() -> f32 {
    4.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GnssParams {
    #[serde(default = "default_gnss_doppler_hz")]
    pub doppler_hz: u32,
    #[serde(default = "default_gnss_threshold")]
    pub threshold: f32,
}

impl Default for GnssParams {
    fn default() -> Self {
        Self {
            doppler_hz: default_gnss_doppler_hz(),
            threshold: default_gnss_threshold(),
        }
    }
}

/// One tracked satellite: reported when it is acquired and with each subframe it sends.
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

/// The receiver's own position solved from the tracked satellites.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GnssFix {
    pub lat: f64,
    pub lon: f64,
    pub altitude_m: f64,
    pub satellites: Vec<u8>,
    pub pdop: f32,
    pub hdop: f32,
    pub vdop: f32,
    pub residual_m: f32,
    pub clock_bias_us: f64,
    pub week: u16,
    pub tow_seconds: f64,
}

impl EventPayload for GnssFix {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: Some("GPS fix".to_owned()),
            place: Some(Place {
                altitude_m: Some(self.altitude_m as f32),
                ..Place::at(LatLon {
                    lat: self.lat,
                    lon: self.lon,
                })
            }),
            fields: fix_fields(self),
            ..Presentation::of(fix_summary(self))
        }
    }
}

fn fix_fields(f: &GnssFix) -> Vec<Field> {
    let satellites: Vec<String> = f.satellites.iter().map(u8::to_string).collect();
    fields(&[
        ("position", "Position", position(Some(f.lat), Some(f.lon))),
        (
            "altitude",
            "Altitude",
            number(Some(f.altitude_m), Unit::Metres, 1),
        ),
        (
            "satellites",
            "Satellites",
            text(Some(&satellites.join(" "))),
        ),
        ("pdop", "PDOP", number(Some(f.pdop), Unit::None, 1)),
        ("hdop", "HDOP", number(Some(f.hdop), Unit::None, 1)),
        ("vdop", "VDOP", number(Some(f.vdop), Unit::None, 1)),
        (
            "residual",
            "Residual",
            number(Some(f.residual_m), Unit::Metres, 1),
        ),
        (
            "time_of_week",
            "Time of week",
            number(Some(f.tow_seconds), Unit::Seconds, 1),
        ),
        ("week", "Week", count(Some(f.week))),
    ])
}

fn fix_summary(f: &GnssFix) -> String {
    joined([
        format!("{:.5}, {:.5}", f.lat, f.lon),
        format!("{:.0} m", f.altitude_m),
        format!("{} sats", f.satellites.len()),
        format!("PDOP {:.1}", f.pdop),
    ])
}

pub(crate) fn gnss_kind() -> KindSpec {
    KindSpec::of::<GnssFrame>("GNSS lab", "Satellites", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

pub(crate) fn gnss_fix_kind() -> KindSpec {
    KindSpec::of::<GnssFix>("GNSS fix", "Fixes", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::FIX)
        .sighting()
        .build()
}

impl EventContract for GnssFrame {
    const KIND: EventKind = EventKind::named("gnss");
}

impl EventContract for GnssFix {
    const KIND: EventKind = EventKind::named("gnss_fix");
}

impl sdrmm_wire::DecoderSettings for GnssParams {
    const TYPE_ID: &'static str = "gnss";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::DecoderEvent;
    use sdrmm_wire::present::{Unit, number, text};

    #[test]
    fn gnss_says_when_it_is_still_acquiring() {
        let frame = GnssFrame {
            prn: 12,
            doppler_hz: -1234.0,
            code_phase_chips: 511.5,
            cn0_db_hz: 44.25,
            subframe: None,
            tow_seconds: None,
            week: None,
            words: Vec::new(),
        };
        let view = frame.present(&EventKind::named("gnss"));
        assert_eq!(
            view.labels(),
            ["Signal", "Doppler", "Code phase", "C/N0", "Subframe"]
        );
        assert_eq!(view.field("Subframe"), text(Some("acquiring")).as_ref());
        assert_eq!(
            view.field("C/N0"),
            number(Some(44.25), Unit::DbHz, 1).as_ref()
        );
        assert_eq!(
            view.field("Code phase"),
            number(Some(511.5), Unit::Chips, 2).as_ref()
        );
    }

    #[test]
    fn a_gnss_summary_keeps_the_measurements_used_by_the_live_log() {
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
    fn a_fix_lands_on_the_map_with_its_satellites_and_dilution() {
        let fix = DecoderEvent::of(&GnssFix {
            lat: 60.182,
            lon: 24.828,
            altitude_m: 47.2,
            satellites: vec![5, 7, 13, 20, 30],
            pdop: 1.84,
            hdop: 0.9,
            vdop: 1.6,
            residual_m: 2.1,
            clock_bias_us: 1.5,
            week: 2_305,
            tow_seconds: 302_400.0,
        });
        let view = fix.present();
        assert_eq!(
            view.summary,
            "60.18200, 24.82800 · 47 m · 5 sats · PDOP 1.8"
        );
        let place = view.place.expect("a fix has a place");
        assert_eq!((place.at.lat, place.at.lon), (60.182, 24.828));
        assert_eq!(
            view.field("Satellites"),
            text(Some("5 7 13 20 30")).as_ref()
        );
    }
}
