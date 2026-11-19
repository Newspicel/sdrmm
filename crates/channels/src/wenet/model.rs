use sdrmm_wire::present::{
    EventPayload, FieldValue, Place, Presentation, Unit, bearing, count, fields, lat_lon, number,
    picture_fields, picture_size, position, text, text_of, trimmed,
};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec, KindView, Symbol};
use serde::{Deserialize, Serialize};

/// The two Wenet framings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WenetMode {
    /// 115177 baud, every byte an RS232 word.
    #[default]
    Uart,
    /// Wenet v2: 96000 baud, scrambled.
    I2s,
}

impl WenetMode {
    pub const ALL: [Self; 2] = [Self::Uart, Self::I2s];

    #[must_use]
    pub const fn baud(self) -> f64 {
        match self {
            Self::Uart => 115_177.0,
            Self::I2s => 96_000.0,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Uart => "115k2",
            Self::I2s => "v2 96k",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WenetParams {}

impl sdrmm_wire::DecoderSettings for WenetParams {
    const TYPE_ID: &'static str = "wenet";
}

/// Which telemetry packet an event came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WenetPacket {
    Text,
    Gps,
    ImageTelemetry,
}

impl WenetPacket {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Gps => "GPS",
            Self::ImageTelemetry => "Image GPS",
        }
    }
}

/// A Wenet text or telemetry packet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WenetTelemetry {
    pub mode: WenetMode,
    pub packet: WenetPacket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_id: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_kmh: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub climb_ms: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub satellites: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_temp_c: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radio_temp_c: Option<f32>,
}

impl WenetTelemetry {
    fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(callsign) = &self.callsign {
            parts.push(callsign.clone());
        }
        if let Some(message) = &self.text {
            parts.push(message.clone());
        }
        if let Some(alt) = self.altitude_m {
            parts.push(format!("{alt:.0} m"));
        }
        if let Some(climb) = self.climb_ms {
            parts.push(format!("{climb:+.1} m/s"));
        }
        if let (Some(lat), Some(lon)) = (self.lat, self.lon) {
            parts.push(format!("{lat:.5}, {lon:.5}"));
        }
        if parts.is_empty() {
            parts.push(self.packet.label().to_owned());
        }
        parts.join(" · ")
    }
}

impl EventPayload for WenetTelemetry {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: self.callsign.clone(),
            place: lat_lon(self.lat, self.lon).map(|at| {
                Place::at(at)
                    .heading(bearing(self.heading_deg.map(f64::from)))
                    .altitude(self.altitude_m.map(|metres| metres as f32))
            }),
            fields: fields(&[
                ("packet", "Packet", text_of(self.packet.label())),
                ("callsign", "Callsign", text(self.callsign.as_deref())),
                ("message_id", "Message", count(self.message_id)),
                ("text", "Text", text(self.text.as_deref())),
                ("image", "Image", count(self.image_id)),
                (
                    "time",
                    "Time",
                    trimmed(self.time.as_deref()).map(FieldValue::Time),
                ),
                ("position", "Position", position(self.lat, self.lon)),
                (
                    "altitude",
                    "Altitude",
                    number(self.altitude_m, Unit::Metres, 0),
                ),
                (
                    "climb",
                    "Climb",
                    number(self.climb_ms, Unit::MetresPerSecond, 1),
                ),
                (
                    "speed",
                    "Speed",
                    number(self.speed_kmh, Unit::KilometresPerHour, 0),
                ),
                (
                    "heading",
                    "Heading",
                    number(self.heading_deg, Unit::Degrees, 0),
                ),
                ("satellites", "Satellites", count(self.satellites)),
                ("cpu", "CPU", number(self.cpu_temp_c, Unit::Celsius, 1)),
                (
                    "radio",
                    "Radio",
                    number(self.radio_temp_c, Unit::Celsius, 1),
                ),
                ("mode", "Mode", text_of(self.mode.label())),
            ]),
            ..Presentation::of(self.summary())
        }
    }
}

impl EventContract for WenetTelemetry {
    const KIND: EventKind = EventKind::named("wenet");
}

/// A picture drawn from SSDV packets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WenetImage {
    pub seq: u32,
    pub mode: WenetMode,
    pub callsign: String,
    pub image_id: u8,
    pub width: u16,
    pub height: u16,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
    /// JPEG quality level, 0 to 7.
    pub quality: u8,
    pub sampling: String,
    pub packets: u32,
    pub packets_lost: u32,
    pub frames_failed: u32,
}

impl WenetImage {
    fn summary(&self) -> String {
        let mut parts = vec![
            format!("{} image {}", self.callsign, self.image_id),
            format!("{} × {}", self.width, self.height),
            format!("{} packets", self.packets),
        ];
        if self.packets_lost > 0 {
            parts.push(format!("{} lost", self.packets_lost));
        }
        parts.join(" · ")
    }
}

impl EventPayload for WenetImage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: Some(self.callsign.clone()),
            fields: picture_fields(
                &[
                    ("callsign", "Callsign", text(Some(&self.callsign))),
                    ("image", "Image", count(Some(self.image_id))),
                    ("size", "Size", picture_size(self.width, self.height)),
                    ("quality", "Quality", count(Some(self.quality))),
                    ("sampling", "Sampling", text(Some(&self.sampling))),
                    ("packets", "Packets", count(Some(self.packets))),
                    (
                        "packets_lost",
                        "Packets lost",
                        count(Some(self.packets_lost)),
                    ),
                    (
                        "frames_failed",
                        "Frames failed",
                        count(Some(self.frames_failed)),
                    ),
                    ("mode", "Mode", text_of(self.mode.label())),
                ],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(self.summary())
        }
    }
}

impl EventContract for WenetImage {
    const KIND: EventKind = EventKind::named("wenet_image");
}

pub(crate) fn wenet_kinds() -> Vec<KindSpec> {
    vec![
        KindSpec::of::<WenetTelemetry>("Wenet", "Balloons", Hue::Orange)
            .caps(&[Capability::Log, Capability::Station, Capability::Geo])
            .symbol(Symbol::BALLOON)
            .sighting()
            .build(),
        KindSpec::of::<WenetImage>("Wenet pictures", "Pictures", Hue::Orange)
            .caps(&[Capability::Log, Capability::Picture])
            .view(KindView::Picture)
            .build(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix() -> WenetTelemetry {
        WenetTelemetry {
            mode: WenetMode::Uart,
            packet: WenetPacket::Gps,
            callsign: Some("VK5ARG".to_owned()),
            message_id: None,
            text: None,
            image_id: None,
            time: Some("2026-01-01T00:00:00Z".to_owned()),
            lat: Some(-34.9),
            lon: Some(138.6),
            altitude_m: Some(21_000.0),
            speed_kmh: Some(40.0),
            heading_deg: Some(90.0),
            climb_ms: Some(5.2),
            satellites: Some(9),
            cpu_temp_c: None,
            radio_temp_c: None,
        }
    }

    #[test]
    fn empty_settings_parse() {
        assert_eq!(
            serde_json::from_str::<WenetParams>("{}").expect("settings"),
            WenetParams::default()
        );
    }

    #[test]
    fn a_fix_places_the_balloon() {
        let view = fix().present(&WenetTelemetry::KIND);
        assert_eq!(view.entity.as_deref(), Some("VK5ARG"));
        let place = view.place.expect("a place");
        assert!((place.at.lat + 34.9).abs() < 1e-9);
        assert_eq!(view.field("Satellites"), count(Some(9)).as_ref());
        assert!(view.summary.contains("21000 m"));
    }

    #[test]
    fn a_picture_shows_its_losses() {
        let view = WenetImage {
            seq: 1,
            mode: WenetMode::I2s,
            callsign: "VK5ARG".to_owned(),
            image_id: 3,
            width: 640,
            height: 480,
            lines: 480,
            complete: false,
            duration_ms: 9_000,
            quality: 6,
            sampling: "4:2:0".to_owned(),
            packets: 400,
            packets_lost: 2,
            frames_failed: 1,
        }
        .present(&WenetImage::KIND);
        assert_eq!(view.field("Packets lost"), count(Some(2)).as_ref());
        assert_eq!(view.field("Mode"), text_of("v2 96k").as_ref());
        assert!(view.summary.contains("2 lost"));
    }
}
