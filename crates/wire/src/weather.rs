use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    decode::{EventKind, Symbol},
    present::{
        EventPayload, Field, FieldValue, LineStyle, Place, Presentation, Shape, Unit, bearing,
        count, fields, lat_lon, number, picture_fields, picture_size, position, repaired, text,
        text_of, trimmed,
    },
};

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AptParams {
    #[serde(default = "default_true")]
    pub keep_partial: bool,
}

impl Default for AptParams {
    fn default() -> Self {
        Self { keep_partial: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LrptMode {
    Qpsk72,
    #[default]
    Oqpsk72,
    Oqpsk80,
}

impl LrptMode {
    pub const ALL: [Self; 3] = [Self::Qpsk72, Self::Oqpsk72, Self::Oqpsk80];

    #[must_use]
    pub const fn symbol_rate(self) -> f64 {
        match self {
            Self::Qpsk72 | Self::Oqpsk72 => 72_000.0,
            Self::Oqpsk80 => 80_000.0,
        }
    }

    #[must_use]
    pub const fn offset(self) -> bool {
        matches!(self, Self::Oqpsk72 | Self::Oqpsk80)
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Qpsk72 => "QPSK 72k",
            Self::Oqpsk72 => "OQPSK 72k",
            Self::Oqpsk80 => "OQPSK 80k",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LrptParams {
    #[serde(default)]
    pub mode: LrptMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WefaxIoc {
    #[default]
    Ioc576,
    Ioc288,
}

impl WefaxIoc {
    pub const ALL: [Self; 2] = [Self::Ioc576, Self::Ioc288];

    #[must_use]
    pub const fn value(self) -> u16 {
        match self {
            Self::Ioc576 => 576,
            Self::Ioc288 => 288,
        }
    }

    #[must_use]
    pub fn width(self) -> u16 {
        (f64::from(self.value()) * std::f64::consts::PI).round() as u16
    }

    #[must_use]
    pub const fn start_tone_hz(self) -> f64 {
        match self {
            Self::Ioc576 => 300.0,
            Self::Ioc288 => 675.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WefaxLpm {
    Lpm60,
    Lpm90,
    #[default]
    Lpm120,
    Lpm240,
}

impl WefaxLpm {
    pub const ALL: [Self; 4] = [Self::Lpm60, Self::Lpm90, Self::Lpm120, Self::Lpm240];

    #[must_use]
    pub const fn value(self) -> u16 {
        match self {
            Self::Lpm60 => 60,
            Self::Lpm90 => 90,
            Self::Lpm120 => 120,
            Self::Lpm240 => 240,
        }
    }

    #[must_use]
    pub fn line_ms(self) -> f64 {
        60_000.0 / f64::from(self.value())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WefaxParams {
    #[serde(default)]
    pub ioc: WefaxIoc,
    #[serde(default)]
    pub lpm: WefaxLpm,
    #[serde(default = "default_true")]
    pub keep_partial: bool,
}

impl Default for WefaxParams {
    fn default() -> Self {
        Self {
            ioc: WefaxIoc::default(),
            lpm: WefaxLpm::default(),
            keep_partial: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RadiosondeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sonde: Option<SondeType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst_m: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_m: Option<i32>,
}

pub const DEFAULT_SONDE_BURST_M: u32 = 26_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SondeForecast {
    pub burst: crate::LatLon,
    pub burst_altitude_m: f64,
    pub burst_seen: bool,
    pub landing: crate::LatLon,
    pub landing_in_s: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<crate::LatLon>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AvhrrChannel {
    Ch1,
    Ch2,
    Ch3a,
    Ch3b,
    Ch4,
    Ch5,
}

impl AvhrrChannel {
    pub const ALL: [Self; 6] = [
        Self::Ch1,
        Self::Ch2,
        Self::Ch3a,
        Self::Ch3b,
        Self::Ch4,
        Self::Ch5,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ch1 => "1",
            Self::Ch2 => "2",
            Self::Ch3a => "3A",
            Self::Ch3b => "3B",
            Self::Ch4 => "4",
            Self::Ch5 => "5",
        }
    }

    #[must_use]
    pub const fn wedge(self) -> u8 {
        match self {
            Self::Ch1 => 1,
            Self::Ch2 => 2,
            Self::Ch3a => 3,
            Self::Ch4 => 4,
            Self::Ch5 => 5,
            Self::Ch3b => 6,
        }
    }

    #[must_use]
    pub fn from_wedge(wedge: u8) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|channel| channel.wedge() == wedge)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AptImage {
    pub seq: u32,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_a: Option<AvhrrChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_b: Option<AvhrrChannel>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LrptImage {
    pub seq: u32,
    pub mode: LrptMode,
    pub width: u16,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
    pub apids: Vec<u16>,
    pub frames: u32,
    pub frames_corrected: u32,
    pub frames_failed: u32,
    pub packets_lost: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WefaxPicture {
    pub seq: u32,
    pub ioc: WefaxIoc,
    pub lpm: WefaxLpm,
    pub width: u16,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
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

fn apt_summary(p: &AptImage) -> String {
    let mut parts = vec!["APT".to_owned()];
    if let (Some(a), Some(b)) = (p.channel_a, p.channel_b) {
        parts.push(format!("ch {}/{}", a.label(), b.label()));
    }
    parts.push(if p.complete {
        format!("{} lines in {} s", p.lines, p.duration_ms / 1_000)
    } else {
        format!("{} lines, cut short", p.lines)
    });
    parts.join(" · ")
}

fn lrpt_summary(p: &LrptImage) -> String {
    let apids = p
        .apids
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join("/");
    let mut parts = vec![p.mode.label().to_owned()];
    if !apids.is_empty() {
        parts.push(format!("APID {apids}"));
    }
    parts.push(format!("{} lines", p.lines));
    if p.frames_failed > 0 || p.packets_lost > 0 {
        parts.push(format!(
            "{} frames lost · {} packets lost",
            p.frames_failed, p.packets_lost
        ));
    }
    parts.join(" · ")
}

fn wefax_summary(p: &WefaxPicture) -> String {
    let mut parts = vec![format!("IOC {} · {} LPM", p.ioc.value(), p.lpm.value())];
    parts.push(if p.complete {
        format!("{} lines in {} s", p.lines, p.duration_ms / 1_000)
    } else {
        format!("{} lines, cut short", p.lines)
    });
    parts.join(" · ")
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

impl EventPayload for AptImage {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            fields: picture_fields(
                &[("Channels", apt_channels(self))],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(apt_summary(self))
        }
    }
}

fn apt_channels(p: &AptImage) -> Option<FieldValue> {
    if p.channel_a.is_none() && p.channel_b.is_none() {
        return None;
    }
    let label = |channel: Option<AvhrrChannel>| channel.map_or("?", AvhrrChannel::label);
    text_of(format!(
        "ch {} + {}",
        label(p.channel_a),
        label(p.channel_b)
    ))
}

impl EventPayload for LrptImage {
    fn present(&self, _kind: EventKind) -> Presentation {
        let apids = self
            .apids
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        Presentation {
            fields: picture_fields(
                &[
                    ("Mode", text_of(self.mode.label())),
                    ("Size", picture_size(self.width, self.lines)),
                    ("APIDs", text(Some(&apids))),
                    ("Frames", count(Some(self.frames))),
                    ("Frames repaired", count(Some(self.frames_corrected))),
                    ("Frames failed", count(Some(self.frames_failed))),
                    ("Packets lost", count(Some(self.packets_lost))),
                ],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(lrpt_summary(self))
        }
    }
}

impl EventPayload for WefaxPicture {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            fields: picture_fields(
                &[
                    ("IOC", count(Some(self.ioc.value()))),
                    ("LPM", count(Some(self.lpm.value()))),
                    ("Size", picture_size(self.width, self.lines)),
                ],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(wefax_summary(self))
        }
    }
}

impl EventPayload for RadiosondeFrame {
    fn present(&self, _kind: EventKind) -> Presentation {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wefax_geometry_follows_the_index_of_cooperation() {
        assert_eq!(WefaxIoc::Ioc576.width(), 1810);
        assert_eq!(WefaxIoc::Ioc288.width(), 905);
        assert_eq!(WefaxLpm::Lpm120.line_ms(), 500.0);
        assert_eq!(WefaxLpm::Lpm60.line_ms(), 1_000.0);
    }

    #[test]
    fn avhrr_wedges_name_their_channel() {
        for channel in AvhrrChannel::ALL {
            assert_eq!(AvhrrChannel::from_wedge(channel.wedge()), Some(channel));
        }
        assert_eq!(AvhrrChannel::from_wedge(7), None);
    }

    #[test]
    fn empty_settings_take_the_defaults() {
        assert_eq!(
            serde_json::from_str::<WefaxParams>("{}").unwrap(),
            WefaxParams::default()
        );
        assert_eq!(
            serde_json::from_str::<AptParams>("{}").unwrap(),
            AptParams::default()
        );
        assert_eq!(
            serde_json::from_str::<LrptParams>("{}").unwrap().mode,
            LrptMode::Oqpsk72
        );
        assert_eq!(
            serde_json::from_str::<RadiosondeParams>("{}")
                .unwrap()
                .sonde,
            None
        );
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

    fn sonde(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<RadiosondeFrame>(json)
            .unwrap()
            .present(EventKind::Radiosonde)
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

    #[test]
    fn apt_names_its_avhrr_channels() {
        let view = AptImage {
            seq: 2,
            lines: 900,
            complete: false,
            duration_ms: 450_000,
            channel_a: Some(AvhrrChannel::Ch2),
            channel_b: Some(AvhrrChannel::Ch4),
        }
        .present(EventKind::Apt);
        assert_eq!(view.labels(), ["Channels", "Lines", "Complete", "Took"]);
        assert_eq!(view.field("Channels"), text_of("ch 2 + 4").as_ref());
        assert_eq!(
            view.field("Took"),
            number(Some(450.0), Unit::Seconds, 1).as_ref()
        );
        assert_eq!(view.entity, None);
    }

    #[test]
    fn lrpt_shows_link_quality() {
        let view = LrptImage {
            seq: 1,
            mode: LrptMode::Oqpsk80,
            width: 1568,
            lines: 400,
            complete: true,
            duration_ms: 1000,
            apids: vec![64, 65],
            frames: 500,
            frames_corrected: 12,
            frames_failed: 1,
            packets_lost: 4,
        }
        .present(EventKind::Lrpt);
        assert_eq!(view.field("Mode"), text_of("OQPSK 80k").as_ref());
        assert_eq!(view.field("APIDs"), text_of("64, 65").as_ref());
        assert_eq!(view.field("Frames repaired"), count(Some(12)).as_ref());
        assert_eq!(view.field("Packets lost"), count(Some(4)).as_ref());
    }

    #[test]
    fn wefax_shows_its_timing() {
        let view = WefaxPicture {
            seq: 1,
            ioc: WefaxIoc::Ioc288,
            lpm: WefaxLpm::Lpm60,
            width: 904,
            lines: 300,
            complete: true,
            duration_ms: 300_000,
        }
        .present(EventKind::Wefax);
        assert_eq!(view.field("IOC"), count(Some(288)).as_ref());
        assert_eq!(view.field("LPM"), count(Some(60)).as_ref());
        assert_eq!(view.field("Size"), text_of("904 × 300").as_ref());
    }
}
