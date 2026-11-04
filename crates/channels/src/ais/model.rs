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
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::lat_lon;
use sdrmm_wire::present::number;
use sdrmm_wire::present::position;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;
use sdrmm_wire::present::trimmed;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AisChannel {
    #[default]
    A,
    B,
}

impl AisChannel {
    #[must_use]
    pub fn letter(self) -> char {
        match self {
            Self::A => 'A',
            Self::B => 'B',
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AisParams {
    #[serde(default)]
    pub ais_channel: AisChannel,
}

labelled! {
    AisChannel { A => "A", B => "B" }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AisMessage {
    pub mmsi: u32,
    pub msg_type: u8,
    pub ais_channel: char,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_sign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sog_kt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cog_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nav_status: Option<u8>,
    pub nmea: String,
}

const AIS_NO_HEADING: u16 = 511;

const AIS_NO_COURSE: f64 = 360.0;

impl EventPayload for AisMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            label: trimmed(self.name.as_deref()).or_else(|| trimmed(self.call_sign.as_deref())),
            place: lat_lon(self.lat, self.lon).map(|at| Place::at(at).heading(ais_heading(self))),
            entity: Some(self.mmsi.to_string()),
            fields: ais_fields(self),
            ..Presentation::of(ais_summary(self))
        }
    }
}

fn ais_heading(m: &AisMessage) -> Option<f32> {
    bearing(true_heading(m)).or_else(|| bearing(course(m)))
}

fn true_heading(m: &AisMessage) -> Option<f64> {
    m.heading_deg
        .filter(|deg| *deg != AIS_NO_HEADING)
        .map(f64::from)
}

fn course(m: &AisMessage) -> Option<f64> {
    m.cog_deg.filter(|deg| *deg != AIS_NO_COURSE)
}

fn ais_fields(m: &AisMessage) -> Vec<Field> {
    fields(&[
        ("mmsi", "MMSI", text_of(m.mmsi)),
        ("message_type", "Message type", count(Some(m.msg_type))),
        ("channel", "Channel", text_of(m.ais_channel)),
        ("name", "Name", text(m.name.as_deref())),
        ("call_sign", "Call sign", text(m.call_sign.as_deref())),
        ("destination", "Destination", text(m.destination.as_deref())),
        ("position", "Position", position(m.lat, m.lon)),
        ("sog", "SOG", number(m.sog_kt, Unit::Knots, 1)),
        ("cog", "COG", number(course(m), Unit::Degrees, 0)),
        (
            "heading",
            "Heading",
            number(true_heading(m), Unit::Degrees, 0),
        ),
        ("nav_status", "Nav status", count(m.nav_status)),
        ("nmea", "NMEA", text(Some(&m.nmea))),
    ])
}

fn ais_summary(m: &AisMessage) -> String {
    let mut parts = vec![m.mmsi.to_string()];
    if let Some(name) = &m.name {
        parts.push(name.trim().to_owned());
    }
    if let (Some(lat), Some(lon)) = (m.lat, m.lon) {
        parts.push(format!("{lat:.4}, {lon:.4}"));
    }
    joined(parts)
}

pub(crate) fn ais_kind() -> KindSpec {
    KindSpec::of::<AisMessage>("AIS", "Vessels", Hue::Amber)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::SHIP)
        .sighting()
        .view(
            KindView::table(&[
                "mmsi",
                "name",
                "call_sign",
                "sog",
                "cog",
                "heading",
                "destination",
                "position",
            ])
            .sorted("name", false)
            .keyed("mmsi"),
        )
        .build()
}

impl EventContract for AisMessage {
    const KIND: EventKind = EventKind::named("ais");
}

impl sdrmm_wire::DecoderSettings for AisParams {
    const TYPE_ID: &'static str = "ais";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::present::FieldValue;

    #[test]
    fn a_vessel_without_heading_points_along_its_course() {
        let vessel: AisMessage = serde_json::from_str(
            r#"{"mmsi":211234560,"msg_type":1,"ais_channel":"A","name":"ELBE","lat":53.5,"lon":9.9,"cog_deg":120.0,"heading_deg":511,"nmea":"!AIVDM"}"#,
        )
        .expect("vessel fixture");
        let place = vessel
            .present(&EventKind::named("ais"))
            .place
            .expect("placed");
        assert_eq!(place.heading_deg, Some(120.0));
    }

    fn ais(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<AisMessage>(json)
            .unwrap()
            .present(&EventKind::named("ais"))
    }

    #[test]
    fn ais_drops_the_not_available_course_and_heading() {
        let view = ais(serde_json::json!({
            "mmsi": 211234560, "msg_type": 1, "ais_channel": "A", "nmea": "!AIVDM",
            "lat": 54.1, "lon": 10.2, "sog_kt": 12.34, "cog_deg": 360.0, "heading_deg": 511,
        }));
        assert_eq!(
            view.labels(),
            ["MMSI", "Message type", "Channel", "Position", "SOG", "NMEA"]
        );
        assert_eq!(view.field("MMSI"), text(Some("211234560")).as_ref());
        assert_eq!(
            view.field("SOG"),
            number(Some(12.34), Unit::Knots, 1).as_ref()
        );
    }

    #[test]
    fn ais_shows_identity_and_motion() {
        let view = ais(serde_json::json!({
            "mmsi": 211234560, "msg_type": 5, "ais_channel": "B", "nmea": "!AIVDM",
            "name": "NORDIC ", "call_sign": "DABC", "destination": "KIEL",
            "cog_deg": 87.6, "heading_deg": 90, "nav_status": 0,
        }));
        assert_eq!(view.field("Name"), text(Some("NORDIC")).as_ref());
        assert_eq!(view.field("Call sign"), text(Some("DABC")).as_ref());
        assert_eq!(view.field("Destination"), text(Some("KIEL")).as_ref());
        assert_eq!(
            view.field("COG"),
            number(Some(87.6), Unit::Degrees, 0).as_ref()
        );
        assert_eq!(
            view.field("Heading"),
            number(Some(90), Unit::Degrees, 0).as_ref()
        );
        assert_eq!(
            view.field("Nav status"),
            Some(&FieldValue::Number {
                value: 0.0,
                unit: Unit::None,
                digits: 0
            })
        );
    }

    #[test]
    fn the_vessel_table_names_fields_messages_show() {
        let sample = |json| sdrmm_wire::DecoderEvent::tagged(AisMessage::KIND, &json);
        let samples = [
            sample(serde_json::json!({
                "mmsi": 211234560, "msg_type": 1, "ais_channel": "A", "nmea": "!AIVDM",
                "lat": 54.3, "lon": 10.1, "sog_kt": 12.3, "cog_deg": 87.6, "heading_deg": 90,
            })),
            sample(serde_json::json!({
                "mmsi": 211234560, "msg_type": 5, "ais_channel": "B", "nmea": "!AIVDM",
                "name": "NORDIC", "call_sign": "DABC", "destination": "KIEL",
            })),
        ];
        crate::testutil::assert_views_hold(&ais_kind(), &samples);
    }
}
