use serde::{Deserialize, Serialize};

use sdrmm_wire::present::{
    EventPayload, Field, FieldValue, Place, Presentation, Unit, count, fields, lat_lon, number,
    position, repaired, text, text_of,
};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec, Symbol};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HorusParams {}

impl sdrmm_wire::DecoderSettings for HorusParams {
    const TYPE_ID: &'static str = "horus";
}

/// One sensor reading a Horus v3 payload names itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HorusSensor {
    pub name: String,
    pub value: String,
}

/// One Horus Binary telemetry frame. v1 and v2 carry a numeric payload ID, v3 its callsign.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HorusFrame {
    pub version: u8,
    pub payload: String,
    pub sequence: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_kmh: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub climb_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub satellites: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_temperature_c: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humidity_pct: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_hpa: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_v: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sensors: Vec<HorusSensor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
    pub snr_db: f32,
    pub errors_corrected: u32,
    /// The payload bytes in hex, CRC included, as horusdemodlib prints them.
    pub raw: String,
}

impl HorusFrame {
    pub(crate) fn new(version: u8, payload: String, sequence: u16) -> Self {
        Self {
            version,
            payload,
            sequence,
            time: None,
            lat: None,
            lon: None,
            altitude_m: None,
            speed_kmh: None,
            climb_ms: None,
            satellites: None,
            temperature_c: None,
            external_temperature_c: None,
            humidity_pct: None,
            pressure_hpa: None,
            battery_v: None,
            sensors: Vec::new(),
            custom: None,
            snr_db: 0.0,
            errors_corrected: 0,
            raw: String::new(),
        }
    }

    /// Sets the position unless it is the 0, 0 a payload sends before its first fix.
    pub(crate) fn place(&mut self, lat: f64, lon: f64) {
        let plausible = lat.is_finite()
            && lon.is_finite()
            && lat.abs() <= 90.0
            && lon.abs() <= 180.0
            && (lat, lon) != (0.0, 0.0);
        if plausible {
            self.lat = Some(lat);
            self.lon = Some(lon);
        }
    }

    fn summary(&self) -> String {
        let mut parts = vec![format!("{} #{}", self.payload, self.sequence)];
        if let Some(alt) = self.altitude_m {
            parts.push(format!("{alt:.0} m"));
        }
        if let (Some(lat), Some(lon)) = (self.lat, self.lon) {
            parts.push(format!("{lat:.5}, {lon:.5}"));
        }
        parts.join(" · ")
    }

    fn fields(&self) -> Vec<Field> {
        let mut shown = fields(&[
            ("payload", "Payload", text(Some(&self.payload))),
            (
                "format",
                "Format",
                text_of(format_args!("v{}", self.version)),
            ),
            ("sequence", "Sequence", count(Some(self.sequence))),
            ("time", "Time", text(self.time.as_deref())),
            ("position", "Position", position(self.lat, self.lon)),
            (
                "altitude",
                "Altitude",
                number(self.altitude_m, Unit::Metres, 0),
            ),
            (
                "speed",
                "Speed",
                number(self.speed_kmh, Unit::KilometresPerHour, 0),
            ),
            (
                "climb",
                "Climb",
                number(self.climb_ms, Unit::MetresPerSecond, 1),
            ),
            ("satellites", "Satellites", count(self.satellites)),
            (
                "temperature",
                "Temperature",
                number(self.temperature_c, Unit::Celsius, 1),
            ),
            (
                "external_temperature",
                "Outside",
                number(self.external_temperature_c, Unit::Celsius, 1),
            ),
            (
                "humidity",
                "Humidity",
                number(self.humidity_pct, Unit::Percent, 0),
            ),
            (
                "pressure",
                "Pressure",
                number(self.pressure_hpa, Unit::Hectopascal, 1),
            ),
            ("battery", "Battery", number(self.battery_v, Unit::Volts, 2)),
            ("custom", "Custom", text(self.custom.as_deref())),
            ("snr", "SNR", number(Some(self.snr_db), Unit::Db, 1)),
            ("repaired", "Repaired", repaired(self.errors_corrected)),
        ]);
        shown.extend(self.sensors.iter().enumerate().map(|(index, sensor)| {
            Field::new(
                format!("sensor_{index}"),
                sensor.name.clone(),
                FieldValue::Text(sensor.value.clone()),
            )
        }));
        shown
    }
}

impl EventPayload for HorusFrame {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: Some(self.payload.clone()),
            place: lat_lon(self.lat, self.lon)
                .map(|at| Place::at(at).altitude(self.altitude_m.map(|metres| metres as f32))),
            fields: self.fields(),
            ..Presentation::of(self.summary())
        }
    }
}

impl EventContract for HorusFrame {
    const KIND: EventKind = EventKind::named("horus");
}

pub(crate) fn horus_kind() -> KindSpec {
    KindSpec::of::<HorusFrame>("Horus", "Balloons", Hue::Orange)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::BALLOON)
        .sighting()
        .build()
}
