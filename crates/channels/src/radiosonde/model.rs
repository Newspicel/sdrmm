use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;
use sdrmm_wire::Symbol;

use sdrmm_wire::named;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::FieldValue;
use sdrmm_wire::present::LineStyle;
use sdrmm_wire::present::Place;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Shape;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::bearing;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::lat_lon;
use sdrmm_wire::present::number;
use sdrmm_wire::present::position;
use sdrmm_wire::present::repaired;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;
use sdrmm_wire::present::trimmed;

named!(SondeType);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SondeType {
    Rs41,
    Dfm,
    M10,
    M20,
    Imet4,
}

impl SondeType {
    pub const ALL: [Self; 5] = [Self::Rs41, Self::Dfm, Self::M10, Self::M20, Self::Imet4];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rs41 => "RS41",
            Self::Dfm => "DFM",
            Self::M10 => "M10",
            Self::M20 => "M20",
            Self::Imet4 => "iMet-4",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadiosondeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sonde: Option<SondeType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst_m: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_m: Option<i32>,
}

pub const DEFAULT_SONDE_BURST_M: u32 = 26_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SondeForecast {
    pub burst: sdrmm_wire::LatLon,
    pub burst_altitude_m: f64,
    pub burst_seen: bool,
    pub landing: sdrmm_wire::LatLon,
    pub landing_in_s: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<sdrmm_wire::LatLon>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadiosondeFrame {
    pub sonde: SondeType,
    pub serial: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub climb_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humidity_pct: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_hpa: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub satellites: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_v: Option<f32>,
    pub errors_corrected: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forecast: Option<SondeForecast>,
}

fn radiosonde_summary(f: &RadiosondeFrame) -> String {
    let mut parts = vec![format!("{} {}", f.sonde.label(), f.serial)];
    if let Some(alt) = f.altitude_m {
        parts.push(format!("{alt:.0} m"));
    }
    if let Some(climb) = f.climb_ms {
        parts.push(format!("{climb:+.1} m/s"));
    }
    if let Some(t) = f.temperature_c {
        parts.push(format!("{t:.1} °C"));
    }
    if let (Some(lat), Some(lon)) = (f.lat, f.lon) {
        parts.push(format!("{lat:.5}, {lon:.5}"));
    }
    parts.join(" · ")
}

impl EventPayload for RadiosondeFrame {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: Some(self.serial.clone()),
            place: lat_lon(self.lat, self.lon).map(|at| {
                Place::at(at)
                    .heading(bearing(self.heading_deg))
                    .altitude(self.altitude_m.map(|metres| metres as f32))
            }),
            shapes: self
                .forecast
                .as_ref()
                .map(SondeForecast::shapes)
                .unwrap_or_default(),
            fields: self.fields(),
            ..Presentation::of(radiosonde_summary(self))
        }
    }
}

impl RadiosondeFrame {
    fn fields(&self) -> Vec<Field> {
        let mut shown = fields(&[
            ("Serial", text(Some(&self.serial))),
            ("Type", text_of(self.sonde.label())),
            ("Frame", count(self.frame)),
            ("Time", trimmed(self.time.as_deref()).map(FieldValue::Time)),
            ("Position", position(self.lat, self.lon)),
            ("Altitude", number(self.altitude_m, Unit::Metres, 0)),
            ("Climb", number(self.climb_ms, Unit::MetresPerSecond, 1)),
            ("Speed", number(self.speed_ms, Unit::MetresPerSecond, 1)),
            ("Heading", number(self.heading_deg, Unit::Degrees, 0)),
            ("Temperature", number(self.temperature_c, Unit::Celsius, 1)),
            ("Humidity", number(self.humidity_pct, Unit::Percent, 0)),
            ("Pressure", number(self.pressure_hpa, Unit::Hectopascal, 1)),
            ("Satellites", count(self.satellites)),
            ("Battery", number(self.battery_v, Unit::Volts, 2)),
            ("Repaired", repaired(self.errors_corrected)),
        ]);
        shown.extend(
            self.forecast
                .as_ref()
                .map(SondeForecast::fields)
                .unwrap_or_default(),
        );
        shown
    }
}

impl SondeForecast {
    fn fields(&self) -> Vec<Field> {
        let burst = if self.burst_seen {
            "Burst"
        } else {
            "Burst est."
        };
        fields(&[
            (burst, number(Some(self.burst_altitude_m), Unit::Metres, 0)),
            (
                "Landing",
                position(Some(self.landing.lat), Some(self.landing.lon)),
            ),
            (
                "Lands in",
                number(Some(self.landing_in_s.max(0.0)), Unit::Seconds, 0),
            ),
        ])
    }

    fn shapes(&self) -> Vec<Shape> {
        let path = (self.path.len() >= 2).then(|| Shape::Line {
            points: self.path.clone(),
            style: LineStyle::Forecast,
        });
        let burst = Shape::Pin {
            at: self.burst,
            symbol: Symbol::Dot,
            label: "Burst".to_owned(),
        };
        let landing = Shape::Pin {
            at: self.landing,
            symbol: Symbol::Target,
            label: "Landing".to_owned(),
        };
        path.into_iter().chain([burst, landing]).collect()
    }
}

pub(crate) fn radiosonde_kind() -> KindSpec {
    KindSpec::of::<RadiosondeFrame>("Radiosonde", "Sondes", Hue::Orange)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::Balloon)
        .sighting()
        .build()
}

impl EventContract for RadiosondeFrame {
    const KIND: EventKind = EventKind::named("radiosonde");
}

impl sdrmm_wire::DecoderSettings for RadiosondeParams {
    const TYPE_ID: &'static str = "radiosonde";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::DecoderEvent;

    const SONDE: &str = r#"{"sonde":"rs41","serial":"S1234567","lat":52.2,"lon":14.1,"altitude_m":12000.0,"heading_deg":45.0,"errors_corrected":0,"forecast":{"burst":{"lat":52.4,"lon":14.3},"burst_altitude_m":31000.0,"burst_seen":false,"landing":{"lat":52.3,"lon":14.6},"landing_in_s":5400.0,"path":[{"lat":52.2,"lon":14.1},{"lat":52.4,"lon":14.3},{"lat":52.3,"lon":14.6}]}}"#;

    #[test]
    fn a_newer_sonde_frame_wins_where_it_has_a_value() {
        let first: RadiosondeFrame = serde_json::from_str(SONDE).expect("sonde fixture");
        let mut sonde = DecoderEvent::of(&first.clone());
        let climbed = DecoderEvent::of(&RadiosondeFrame {
            altitude_m: Some(12_050.0),
            lat: None,
            lon: None,
            ..first
        });
        assert!(sdrmm_wire::EventCatalog::new([radiosonde_kind()]).merge(&mut sonde, &climbed));
        let Some(merged) = sonde.read::<RadiosondeFrame>() else {
            panic!("a sonde");
        };
        assert_eq!(merged.altitude_m, Some(12_050.0));
        assert_eq!((merged.lat, merged.lon), (Some(52.2), Some(14.1)));
    }

    fn sonde(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<RadiosondeFrame>(json)
            .unwrap()
            .present(&EventKind::named("radiosonde"))
    }

    #[test]
    fn radiosonde_telemetry_comes_with_units() {
        let view = sonde(serde_json::json!({
            "sonde": "dfm", "serial": "D2150123", "frame": 4321, "lat": 48.1, "lon": 11.5,
            "altitude_m": 12345.6, "climb_ms": -4.25, "speed_ms": 12.0, "heading_deg": 90.0,
            "temperature_c": -51.2, "humidity_pct": 12.4, "pressure_hpa": 190.5,
            "satellites": 9, "battery_v": 2.9, "errors_corrected": 3,
            "time": "2026-10-12T12:34:56Z",
        }));
        assert_eq!(
            view.labels(),
            [
                "Serial",
                "Type",
                "Frame",
                "Time",
                "Position",
                "Altitude",
                "Climb",
                "Speed",
                "Heading",
                "Temperature",
                "Humidity",
                "Pressure",
                "Satellites",
                "Battery",
                "Repaired"
            ]
        );
        assert_eq!(view.field("Type"), text(Some("DFM")).as_ref());
        assert_eq!(
            view.field("Time"),
            Some(&FieldValue::Time("2026-10-12T12:34:56Z".to_owned()))
        );
        assert_eq!(
            view.field("Climb"),
            number(Some(-4.25), Unit::MetresPerSecond, 1).as_ref()
        );
        assert_eq!(
            view.field("Battery"),
            number(Some(2.9_f32), Unit::Volts, 2).as_ref()
        );
        assert_eq!(view.field("Repaired"), repaired(3).as_ref());
    }

    #[test]
    fn a_forecast_adds_burst_landing_and_time_to_land() {
        let view = sonde(serde_json::json!({
            "sonde": "rs41", "serial": "S1234567", "errors_corrected": 0,
            "forecast": {
                "burst": { "lat": 48.2, "lon": 11.4 }, "burst_altitude_m": 26000.0,
                "burst_seen": false, "landing": { "lat": 48.1, "lon": 11.9 },
                "landing_in_s": 5430.0,
            },
        }));
        assert_eq!(
            view.labels(),
            ["Serial", "Type", "Burst est.", "Landing", "Lands in"]
        );
        assert_eq!(
            view.field("Landing"),
            position(Some(48.1), Some(11.9)).as_ref()
        );
        assert_eq!(
            view.field("Lands in"),
            number(Some(5430.0), Unit::Seconds, 0).as_ref()
        );
    }
}
