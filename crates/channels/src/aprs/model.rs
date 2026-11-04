use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, KindView, Symbol};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Place;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::bearing;
use sdrmm_wire::present::feet_to_metres;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::knots_to_kilometres_per_hour;
use sdrmm_wire::present::lat_lon;
use sdrmm_wire::present::number;
use sdrmm_wire::present::position;
use sdrmm_wire::present::text;
use sdrmm_wire::present::trimmed;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AprsMode {
    #[default]
    Afsk1200,
    G3ruh9600,
}

fn default_aprs_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AprsParams {
    #[serde(default)]
    pub mode: AprsMode,
    #[serde(default = "default_aprs_bandwidth_hz")]
    pub bandwidth_hz: f64,
}

impl Default for AprsParams {
    fn default() -> Self {
        Self {
            mode: AprsMode::default(),
            bandwidth_hz: default_aprs_bandwidth_hz(),
        }
    }
}

labelled! {
    AprsMode { Afsk1200 => "AFSK 1200", G3ruh9600 => "G3RUH 9600" }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AprsPacket {
    pub source: String,
    pub destination: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    pub info: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub course_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_kt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_ft: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_e_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weather: Option<crate::aprs::AprsWeather>,
    pub tnc2: String,
}

impl EventPayload for AprsPacket {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            place: lat_lon(self.lat, self.lon).map(|at| {
                Place::at(at)
                    .heading(bearing(self.course_deg))
                    .altitude(self.altitude_ft.map(|feet| feet_to_metres(feet.into())))
            }),
            entity: trimmed(Some(&self.source)),
            callsign: trimmed(Some(&self.source)),
            fields: aprs_fields(self),
            ..Presentation::of(aprs_summary(self))
        }
    }
}

fn aprs_fields(p: &AprsPacket) -> Vec<Field> {
    let mut shown = fields(&[
        ("source", "Source", text(Some(&p.source))),
        ("destination", "Destination", text(Some(&p.destination))),
        ("path", "Path", text(Some(&p.path.join(" → ")))),
        ("symbol", "Symbol", text(p.symbol.as_deref())),
        ("position", "Position", position(p.lat, p.lon)),
        ("course", "Course", number(p.course_deg, Unit::Degrees, 0)),
        (
            "speed",
            "Speed",
            number(
                p.speed_kt.map(knots_to_kilometres_per_hour),
                Unit::KilometresPerHour,
                1,
            ),
        ),
        (
            "altitude",
            "Altitude",
            number(
                p.altitude_ft.map(|feet| feet_to_metres(feet.into())),
                Unit::Metres,
                0,
            ),
        ),
        ("message", "Message", text(p.mic_e_message.as_deref())),
        ("comment", "Comment", text(p.comment.as_deref())),
    ]);
    shown.extend(p.weather.as_ref().map(weather_fields).unwrap_or_default());
    shown.extend(fields(&[("packet", "Packet", text(Some(&p.tnc2)))]));
    shown
}

fn weather_fields(w: &AprsWeather) -> Vec<Field> {
    fields(&[
        (
            "wind",
            "Wind",
            number(w.wind_speed_ms, Unit::MetresPerSecond, 1),
        ),
        (
            "wind_from",
            "Wind from",
            number(w.wind_dir_deg, Unit::Degrees, 0),
        ),
        (
            "gust",
            "Gust",
            number(w.wind_gust_ms, Unit::MetresPerSecond, 1),
        ),
        (
            "temperature",
            "Temperature",
            number(w.temperature_c, Unit::Celsius, 1),
        ),
        (
            "humidity",
            "Humidity",
            number(w.humidity_pct, Unit::Percent, 0),
        ),
        (
            "pressure",
            "Pressure",
            number(w.pressure_hpa, Unit::Hectopascal, 1),
        ),
        (
            "rain_1_h",
            "Rain 1 h",
            number(w.rain_1h_mm, Unit::Millimetres, 1),
        ),
        (
            "rain_24_h",
            "Rain 24 h",
            number(w.rain_24h_mm, Unit::Millimetres, 1),
        ),
        (
            "rain_today",
            "Rain today",
            number(w.rain_midnight_mm, Unit::Millimetres, 1),
        ),
        (
            "snow_24_h",
            "Snow 24 h",
            number(w.snow_24h_mm, Unit::Millimetres, 1),
        ),
        (
            "light",
            "Light",
            number(w.luminosity_wm2, Unit::WattsPerSquareMetre, 0),
        ),
    ])
}

fn aprs_summary(p: &AprsPacket) -> String {
    let weather = p.weather.as_ref().map(AprsWeather::summary);
    match (&p.mic_e_message, weather) {
        (Some(message), _) => format!("{} · {message}", p.tnc2),
        (None, Some(weather)) if !weather.is_empty() => format!("{} · {weather}", p.source),
        _ => p.tnc2.clone(),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AprsWeather {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_dir_deg: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_speed_ms: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_gust_ms: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rain_1h_mm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rain_24h_mm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rain_midnight_mm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humidity_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_hpa: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub luminosity_wm2: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snow_24h_mm: Option<f32>,
}

impl AprsWeather {
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(t) = self.temperature_c {
            parts.push(format!("{t:.1} °C"));
        }
        if let Some(h) = self.humidity_pct {
            parts.push(format!("{h}%"));
        }
        if let Some(p) = self.pressure_hpa {
            parts.push(format!("{p:.1} hPa"));
        }
        match (self.wind_dir_deg, self.wind_speed_ms) {
            (Some(dir), Some(speed)) => parts.push(format!("{dir:03}° {speed:.1} m/s")),
            (None, Some(speed)) => parts.push(format!("{speed:.1} m/s")),
            _ => {}
        }
        if let Some(rain) = self.rain_1h_mm {
            parts.push(format!("{rain:.1} mm/h"));
        }
        parts.join(" · ")
    }
}

pub(crate) fn aprs_kind() -> KindSpec {
    KindSpec::of::<AprsPacket>("APRS", "Stations", Hue::Violet)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::STATION)
        .sighting()
        .view(
            KindView::table(&[
                "source",
                "symbol",
                "position",
                "speed",
                "altitude",
                "temperature",
                "wind",
                "gust",
                "humidity",
                "pressure",
                "rain_1_h",
            ])
            .sorted("source", false)
            .keyed("source"),
        )
        .build()
}

impl EventContract for AprsPacket {
    const KIND: EventKind = EventKind::named("aprs");
}

impl sdrmm_wire::DecoderSettings for AprsParams {
    const TYPE_ID: &'static str = "aprs";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::DecoderEvent;
    use sdrmm_wire::LatLon;
    use sdrmm_wire::present::FieldValue;

    #[test]
    fn a_mic_e_summary_names_the_message_beside_the_monitor_line() {
        crate::registry::init();
        let mut packet = AprsPacket {
            tnc2: "DL1ABC-9>S32U6T:`(_fn\"Oj/".to_owned(),
            ..AprsPacket::default()
        };
        assert_eq!(
            DecoderEvent::of(&packet.clone()).present().summary,
            "DL1ABC-9>S32U6T:`(_fn\"Oj/"
        );
        packet.mic_e_message = Some("Returning".to_owned());
        assert_eq!(
            DecoderEvent::of(&packet).present().summary,
            "DL1ABC-9>S32U6T:`(_fn\"Oj/ · Returning"
        );
    }

    fn aprs(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<AprsPacket>(json)
            .unwrap()
            .present(&EventKind::named("aprs"))
    }

    #[test]
    fn only_amateur_decoders_carry_a_callsign() {
        let aprs = AprsPacket {
            source: "DL1ABC-9".to_owned(),
            ..AprsPacket::default()
        };
        assert_eq!(
            aprs.present(&EventKind::named("aprs")).callsign.as_deref(),
            Some("DL1ABC-9")
        );
        let wspr: crate::weak_signal::WsprSpot = serde_json::from_str(
            r#"{"text":"DL1ABC JO62 37","callsign":"DL1ABC","grid":"JO62","power_dbm":37,"snr_db":-20.0,"audio_hz":1500.0,"time_offset_s":0.0,"drift_hz":0.0}"#,
        )
        .expect("wspr fixture");
        assert!(wspr.present(&EventKind::named("wspr")).callsign.is_some());
        for (kind, text) in [("ft8", "CQ DL1ABC JO62"), ("ft4", "W1AW DL1ABC JO62")] {
            let message = crate::weak_signal::WsjtMessage {
                text: text.to_owned(),
                snr_db: -12.0,
                audio_hz: 1_500.0,
                time_offset_s: 0.1,
                hard_errors: 0,
            };
            let view = message.present(&EventKind::new(kind));
            assert!(view.callsign.is_some(), "{kind}");
        }
        let aircraft = crate::adsb::AdsbMessage {
            icao: "3C6444".to_owned(),
            callsign: Some("DLH400".to_owned()),
            ..crate::adsb::AdsbMessage::default()
        };
        assert_eq!(aircraft.present(&EventKind::named("adsb")).callsign, None);
    }

    #[test]
    fn weather_summary_lists_what_was_reported() {
        let weather = AprsWeather {
            wind_dir_deg: Some(220),
            wind_speed_ms: Some(2.2),
            temperature_c: Some(12.5),
            humidity_pct: Some(80),
            pressure_hpa: Some(1013.2),
            ..AprsWeather::default()
        };
        assert_eq!(
            weather.summary(),
            "12.5 °C · 80% · 1013.2 hPa · 220° 2.2 m/s"
        );
    }

    #[test]
    fn aprs_carries_what_the_monitor_line_packs_away() {
        let view = aprs(serde_json::json!({
            "source": "DL1ABC-9", "destination": "S32U6T", "path": ["WIDE1-1", "WIDE2-1"],
            "info": "`(_fn\"Oj/", "tnc2": "DL1ABC-9>S32U6T:`(_fn\"Oj/",
            "lat": 52.52, "lon": 13.405, "course_deg": 251.0, "speed_kt": 20.0,
            "altitude_ft": 1500, "mic_e_message": "En Route",
        }));
        assert_eq!(view.field("Source"), text(Some("DL1ABC-9")).as_ref());
        assert_eq!(view.field("Path"), text(Some("WIDE1-1 → WIDE2-1")).as_ref());
        assert_eq!(
            view.field("Position"),
            Some(&FieldValue::Position(LatLon {
                lat: 52.52,
                lon: 13.405
            }))
        );
        assert_eq!(
            view.field("Course"),
            number(Some(251.0), Unit::Degrees, 0).as_ref()
        );
        assert_eq!(
            view.field("Speed"),
            number(
                Some(knots_to_kilometres_per_hour(20.0)),
                Unit::KilometresPerHour,
                1
            )
            .as_ref()
        );
        assert_eq!(
            view.field("Altitude"),
            number(Some(feet_to_metres(1500.0)), Unit::Metres, 0).as_ref()
        );
        assert_eq!(view.field("Message"), text(Some("En Route")).as_ref());
        assert_eq!(
            view.field("Packet"),
            text(Some("DL1ABC-9>S32U6T:`(_fn\"Oj/")).as_ref()
        );
    }

    #[test]
    fn aprs_weather_comes_with_units() {
        let view = aprs(serde_json::json!({
            "source": "DL1WX", "destination": "APRS", "info": "_", "tnc2": "DL1WX>APRS:_",
            "weather": {
                "wind_dir_deg": 220, "wind_speed_ms": 4.5, "wind_gust_ms": 9.0,
                "temperature_c": 12.25, "humidity_pct": 81, "pressure_hpa": 1013.2,
                "rain_1h_mm": 0.5, "luminosity_wm2": 300,
            },
        }));
        assert_eq!(
            view.labels(),
            [
                "Source",
                "Destination",
                "Wind",
                "Wind from",
                "Gust",
                "Temperature",
                "Humidity",
                "Pressure",
                "Rain 1 h",
                "Light",
                "Packet"
            ]
        );
        assert_eq!(
            view.field("Wind"),
            number(Some(4.5), Unit::MetresPerSecond, 1).as_ref()
        );
        assert_eq!(
            view.field("Wind from"),
            number(Some(220), Unit::Degrees, 0).as_ref()
        );
        assert_eq!(
            view.field("Rain 1 h"),
            number(Some(0.5), Unit::Millimetres, 1).as_ref()
        );
        assert_eq!(
            view.field("Light"),
            number(Some(300), Unit::WattsPerSquareMetre, 0).as_ref()
        );
    }

    #[test]
    fn the_station_table_names_fields_packets_show() {
        let tracker = AprsPacket {
            source: "DL1ABC-9".to_owned(),
            symbol: Some("/>".to_owned()),
            lat: Some(52.5),
            lon: Some(13.4),
            speed_kt: Some(30.0),
            altitude_ft: Some(120),
            tnc2: "DL1ABC-9>APRS:!".to_owned(),
            ..AprsPacket::default()
        };
        let weather = AprsPacket {
            source: "DL1WX".to_owned(),
            weather: Some(crate::aprs::AprsWeather {
                temperature_c: Some(12.0),
                wind_speed_ms: Some(3.0),
                wind_gust_ms: Some(6.0),
                humidity_pct: Some(80),
                pressure_hpa: Some(1013.2),
                rain_1h_mm: Some(0.4),
                ..crate::aprs::AprsWeather::default()
            }),
            tnc2: "DL1WX>APRS:_".to_owned(),
            ..AprsPacket::default()
        };
        crate::testutil::assert_views_hold(
            &aprs_kind(),
            &[DecoderEvent::of(&tracker), DecoderEvent::of(&weather)],
        );
    }
}
