use super::{
    EventPayload, Field, FieldValue, Place, Presentation, Unit, bearing, count, feet_to_metres,
    fields, flag, joined, lat_lon, number, only_if, position, text, text_of, trimmed,
};
use crate::decode::{AcarsMessage, AdsbMessage, EventKind};

impl EventPayload for AdsbMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        let icao = self.icao.to_ascii_uppercase();
        Presentation {
            label: trimmed(self.callsign.as_deref()).or_else(|| Some(icao.clone())),
            place: lat_lon(self.lat, self.lon).map(|at| {
                Place::at(at)
                    .heading(bearing(self.track_deg))
                    .altitude(self.altitude_ft.map(|feet| feet_to_metres(feet.into())))
            }),
            fields: adsb_fields(self, &icao),
            entity: Some(icao),
            ..Presentation::of(adsb_summary(self))
        }
    }
}

fn adsb_fields(a: &AdsbMessage, icao: &str) -> Vec<Field> {
    fields(&[
        ("ICAO", Some(FieldValue::Text(icao.to_owned()))),
        ("Callsign", text(a.callsign.as_deref())),
        ("Format", count(Some(a.df))),
        ("Type code", count(a.type_code)),
        ("Altitude", number(a.altitude_ft, Unit::Feet, 0)),
        ("On ground", flag(a.on_ground)),
        ("Position", position(a.lat, a.lon)),
        ("Speed", number(a.ground_speed_kt, Unit::Knots, 1)),
        ("Track", number(a.track_deg, Unit::Degrees, 0)),
        ("Climb", number(a.vertical_rate_fpm, Unit::FeetPerMinute, 0)),
        ("Squawk", text(a.squawk.as_deref())),
        ("Raw", text(Some(&a.raw))),
    ])
}

fn adsb_summary(a: &AdsbMessage) -> String {
    let mut parts = vec![a.icao.clone()];
    if let Some(cs) = &a.callsign {
        parts.push(cs.trim().to_owned());
    }
    if let Some(alt) = a.altitude_ft {
        parts.push(format!("{alt} ft"));
    }
    if let (Some(lat), Some(lon)) = (a.lat, a.lon) {
        parts.push(format!("{lat:.4}, {lon:.4}"));
    }
    joined(parts)
}

impl EventPayload for AcarsMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            label: trimmed(self.flight.as_deref()),
            entity: Some(self.registration.clone()),
            place: lat_lon(self.lat, self.lon).map(Place::at),
            fields: acars_fields(self),
            ..Presentation::of(acars_summary(self))
        }
    }
}

fn acars_fields(a: &AcarsMessage) -> Vec<Field> {
    let direction = if a.downlink { "downlink" } else { "uplink" };
    fields(&[
        ("Registration", text(Some(&a.registration))),
        ("Flight", text(a.flight.as_deref())),
        ("Label", text(Some(&a.label))),
        ("Sublabel", text(a.sublabel.as_deref())),
        ("MFI", text(a.mfi.as_deref())),
        ("Mode", text_of(a.mode)),
        ("Block", text_of(a.block_id)),
        ("Direction", text_of(direction)),
        ("Sequence", text(a.seq_no.as_deref())),
        ("Ack", a.ack.map_or_else(|| text_of("NAK"), text_of)),
        ("Continues", only_if(a.more)),
        ("Reassembly", text(a.assstat.as_deref())),
        ("Position", position(a.lat, a.lon)),
        ("Text", text(Some(&a.text))),
    ])
}

fn acars_summary(a: &AcarsMessage) -> String {
    let mut parts = vec![a.registration.clone()];
    if let Some(flight) = &a.flight {
        parts.push(flight.trim().to_owned());
    }
    parts.push(format!("[{}]", a.label));
    let text = a.text.replace('\n', " ");
    let text = text.trim();
    if !text.is_empty() {
        parts.push(text.to_owned());
    }
    joined(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LatLon;

    fn adsb(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<AdsbMessage>(json)
            .unwrap()
            .present(EventKind::Adsb)
    }

    #[test]
    fn adsb_lists_only_what_the_frame_carried() {
        let view = adsb(serde_json::json!({ "icao": "3c6444", "df": 11, "raw": "5d" }));
        assert_eq!(view.labels(), ["ICAO", "Format", "Raw"]);
        assert_eq!(view.field("ICAO"), text(Some("3C6444")).as_ref());
        assert_eq!(view.field("Format"), count(Some(11)).as_ref());
    }

    #[test]
    fn adsb_types_every_reported_value() {
        let view = adsb(serde_json::json!({
            "icao": "3c6444", "df": 17, "raw": "8d3c6444", "callsign": " DLH123 ",
            "altitude_ft": 37000, "ground_speed_kt": 451.4, "track_deg": 271.6,
            "vertical_rate_fpm": -1088, "lat": 52.52, "lon": 13.405, "on_ground": false,
        }));
        assert_eq!(view.field("Callsign"), text(Some("DLH123")).as_ref());
        assert_eq!(
            view.field("Altitude"),
            number(Some(37000), Unit::Feet, 0).as_ref()
        );
        assert_eq!(
            view.field("Speed"),
            number(Some(451.4), Unit::Knots, 1).as_ref()
        );
        assert_eq!(
            view.field("Track"),
            number(Some(271.6), Unit::Degrees, 0).as_ref()
        );
        assert_eq!(
            view.field("Climb"),
            number(Some(-1088), Unit::FeetPerMinute, 0).as_ref()
        );
        assert_eq!(
            view.field("Position"),
            Some(&FieldValue::Position(LatLon {
                lat: 52.52,
                lon: 13.405
            }))
        );
        assert_eq!(view.field("On ground"), Some(&FieldValue::Flag(false)));
    }

    #[test]
    fn acars_names_direction_ack_and_continuation() {
        let message: AcarsMessage = serde_json::from_value(serde_json::json!({
            "mode": "2", "registration": "D-AIBC", "flight": "LH0400 ", "label": "H1",
            "block_id": "3", "downlink": true, "seq_no": "M01A",
            "text": "POS N52.5 E013.4\nFL370", "more": true,
        }))
        .unwrap();
        let view = message.present(EventKind::Acars);
        assert_eq!(view.field("Flight"), text(Some("LH0400")).as_ref());
        assert_eq!(view.field("Direction"), text(Some("downlink")).as_ref());
        assert_eq!(view.field("Sequence"), text(Some("M01A")).as_ref());
        assert_eq!(view.field("Ack"), text(Some("NAK")).as_ref());
        assert_eq!(view.field("Continues"), Some(&FieldValue::Flag(true)));
        assert_eq!(
            view.field("Text"),
            text(Some("POS N52.5 E013.4\nFL370")).as_ref()
        );
    }
}
