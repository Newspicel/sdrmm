use sdrmm_wire::present::{
    EventPayload, Field, Place, Presentation, Unit, bearing, fields, lat_lon, number, position,
    text, trimmed,
};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec, KindView, Symbol};
use serde::{Deserialize, Serialize};

fn default_bss_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BssParams {
    #[serde(default = "default_bss_bandwidth_hz")]
    pub bandwidth_hz: f64,
}

impl Default for BssParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_bss_bandwidth_hz(),
        }
    }
}

/// One BSS packet: the chat, position and paging frame Benshi-built handhelds (Btech UV-Pro,
/// VGC VR-N76) send in place of AX.25.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BssPacket {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_m: Option<i16>,
    /// Benshi's own app reads it as km/h; no capture so far moved fast enough to confirm it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_kmh: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<u16>,
    /// The station asked to report its position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_request: Option<String>,
    /// The station asked to ring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_request: Option<String>,
    /// Fields of a type this decoder does not know, as `type:value` in hex.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<String>,
    pub frame: String,
}

impl EventPayload for BssPacket {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            place: lat_lon(self.lat, self.lon).map(|at| {
                Place::at(at)
                    .heading(bearing(self.heading_deg.map(f64::from)))
                    .altitude(self.altitude_m.map(f32::from))
            }),
            entity: trimmed(Some(&self.source)),
            callsign: trimmed(Some(&self.source)),
            fields: bss_fields(self),
            ..Presentation::of(bss_summary(self))
        }
    }
}

fn bss_fields(p: &BssPacket) -> Vec<Field> {
    fields(&[
        ("source", "Source", text(Some(&p.source))),
        ("destination", "To", text(p.destination.as_deref())),
        ("message", "Message", text(p.message.as_deref())),
        ("position", "Position", position(p.lat, p.lon)),
        (
            "altitude",
            "Altitude",
            number(p.altitude_m.map(f64::from), Unit::Metres, 0),
        ),
        (
            "speed",
            "Speed",
            number(p.speed_kmh.map(f64::from), Unit::KilometresPerHour, 0),
        ),
        (
            "heading",
            "Heading",
            number(p.heading_deg.map(f64::from), Unit::Degrees, 0),
        ),
        (
            "location_request",
            "Asks position of",
            text(p.location_request.as_deref()),
        ),
        ("call_request", "Rings", text(p.call_request.as_deref())),
        (
            "message_id",
            "Message ID",
            number(p.message_id.map(f64::from), Unit::None, 0),
        ),
        (
            "unknown",
            "Unknown fields",
            text(Some(&p.unknown.join(" "))),
        ),
        ("frame", "Frame", text(Some(&p.frame))),
    ])
}

fn bss_summary(p: &BssPacket) -> String {
    let mut line = p.source.clone();
    if let Some(to) = &p.destination {
        line.push_str(" → ");
        line.push_str(to);
    }
    let what = [
        p.message.clone(),
        p.location_request
            .as_ref()
            .map(|c| format!("position of {c}?")),
        p.call_request.as_ref().map(|c| format!("ring {c}")),
        lat_lon(p.lat, p.lon).map(|_| "position".to_owned()),
    ];
    let what: Vec<String> = what.into_iter().flatten().collect();
    if !what.is_empty() {
        line.push_str(": ");
        line.push_str(&what.join(" · "));
    }
    line
}

pub(crate) fn bss_kind() -> KindSpec {
    KindSpec::of::<BssPacket>("BSS", "Stations", Hue::Violet)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::PERSON)
        .sighting()
        .view(
            KindView::table(&["source", "message", "position", "altitude", "heading"])
                .sorted("source", false)
                .keyed("source"),
        )
        .build()
}

impl EventContract for BssPacket {
    const KIND: EventKind = EventKind::named("bss");
}

impl sdrmm_wire::DecoderSettings for BssParams {
    const TYPE_ID: &'static str = "bss";
}
