use super::{
    EventPayload, Field, Place, Presentation, Unit, bearing, count, fields, flag, joined, lat_lon,
    number, position, repaired, text, text_of, trimmed,
};
use crate::decode::{AisMessage, EventKind, NavtexMessage};

const AIS_NO_HEADING: u16 = 511;
const AIS_NO_COURSE: f64 = 360.0;

impl EventPayload for AisMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
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
        ("MMSI", text_of(m.mmsi)),
        ("Message type", count(Some(m.msg_type))),
        ("Channel", text_of(m.ais_channel)),
        ("Name", text(m.name.as_deref())),
        ("Call sign", text(m.call_sign.as_deref())),
        ("Destination", text(m.destination.as_deref())),
        ("Position", position(m.lat, m.lon)),
        ("SOG", number(m.sog_kt, Unit::Knots, 1)),
        ("COG", number(course(m), Unit::Degrees, 0)),
        ("Heading", number(true_heading(m), Unit::Degrees, 0)),
        ("Nav status", count(m.nav_status)),
        ("NMEA", text(Some(&m.nmea))),
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

impl EventPayload for NavtexMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: self.station.map(String::from),
            fields: navtex_fields(self),
            ..Presentation::of(navtex_summary(self))
        }
    }
}

fn navtex_fields(n: &NavtexMessage) -> Vec<Field> {
    let subject = n
        .subject_name
        .clone()
        .or_else(|| n.subject.map(String::from));
    fields(&[
        ("Header", text(n.header().as_deref())),
        ("Station", n.station.and_then(text_of)),
        ("Subject", text(subject.as_deref())),
        (
            "Serial",
            n.serial.and_then(|serial| text_of(format!("{serial:02}"))),
        ),
        ("Complete", flag(Some(n.complete))),
        ("Repaired", repaired(n.errors_corrected)),
        ("Text", text(Some(&n.text))),
    ])
}

fn navtex_summary(n: &NavtexMessage) -> String {
    let mut parts = Vec::new();
    if let Some(id) = n.header() {
        parts.push(id);
    }
    if let Some(subject) = &n.subject_name {
        parts.push(subject.clone());
    }
    parts.push(n.text.replace('\n', " ").trim().to_owned());
    parts.retain(|p| !p.is_empty());
    joined(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::present::FieldValue;

    fn ais(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<AisMessage>(json)
            .unwrap()
            .present(EventKind::Ais)
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
    fn navtex_keeps_its_text_and_the_header_it_came_under() {
        let view = serde_json::from_value::<NavtexMessage>(serde_json::json!({
            "station": "D", "subject": "A", "subject_name": "Navigational warning",
            "serial": 7, "text": "GALE WARNING\nGERMAN BIGHT", "errors_corrected": 3,
            "complete": false,
        }))
        .unwrap()
        .present(EventKind::Navtex);
        assert_eq!(view.field("Header"), text(Some("DA07")).as_ref());
        assert_eq!(
            view.field("Subject"),
            text(Some("Navigational warning")).as_ref()
        );
        assert_eq!(view.field("Serial"), text(Some("07")).as_ref());
        assert_eq!(view.field("Complete"), flag(Some(false)).as_ref());
        assert_eq!(view.field("Repaired"), count(Some(3)).as_ref());
        assert_eq!(
            view.field("Text"),
            text(Some("GALE WARNING\nGERMAN BIGHT")).as_ref()
        );
    }
}
