use super::{
    EventPayload, Field, FieldValue, Place, Presentation, Unit, count, fields, flag, joined,
    lat_lon, number, position, repaired, text, text_of, trimmed,
};
use crate::decode::{
    DataLinkMessage, EotArming, EotBattery, EotMessage, EotReport, EotStatus, EventKind, HotRequest,
};

impl EventPayload for DataLinkMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: trimmed(self.station.as_deref()),
            place: lat_lon(self.lat, self.lon).map(Place::at),
            fields: data_link_fields(self),
            ..Presentation::of(data_link_summary(self))
        }
    }
}

fn data_link_fields(m: &DataLinkMessage) -> Vec<Field> {
    let integrity = if m.crc_ok { "verified" } else { "failed" };
    fields(&[
        ("Type", text(Some(&m.message_type))),
        ("Station", text(m.station.as_deref())),
        ("Integrity", text_of(integrity)),
        ("FEC repaired", count(m.fec_corrected)),
        ("SNR", number(m.snr_db, Unit::Db, 1)),
        ("Frequency error", number(m.frequency_error_hz, Unit::Hz, 1)),
        ("Position", position(m.lat, m.lon)),
        ("Text", text(m.text.as_deref())),
        ("Details", details(&m.details)),
        ("Raw", text(m.raw.as_deref())),
    ])
}

fn details(details: &serde_json::Value) -> Option<FieldValue> {
    let empty = details.is_null() || details.as_object().is_some_and(serde_json::Map::is_empty);
    if empty {
        return None;
    }
    serde_json::to_string_pretty(details)
        .ok()
        .and_then(|json| text(Some(&json)))
}

fn data_link_summary(m: &DataLinkMessage) -> String {
    let mut parts = vec![m.message_type.clone()];
    if let Some(station) = &m.station {
        parts.push(station.clone());
    }
    if let Some(text) = &m.text {
        let text = text.replace('\n', " ");
        if !text.trim().is_empty() {
            parts.push(text.trim().to_owned());
        }
    }
    joined(parts)
}

impl EventPayload for EotMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: Some(self.unit_address.to_string()),
            fields: eot_fields(self),
            ..Presentation::of(self.summary())
        }
    }
}

fn eot_fields(m: &EotMessage) -> Vec<Field> {
    let mut shown = match &m.report {
        EotReport::Head(request) => hot_fields(m, request),
        EotReport::Rear(status) => rear_fields(m, status),
    };
    shown.extend(fields(&[("Repaired", repaired(m.errors_corrected))]));
    shown
}

fn hot_fields(m: &EotMessage, request: &HotRequest) -> Vec<Field> {
    fields(&[
        ("Unit", text_of("Head (HOT)")),
        ("Address", text_of(m.unit_address)),
        ("Command", text_of(request.command_label())),
        ("Copies", text_of(format!("{}/3", request.copies))),
    ])
}

fn rear_fields(m: &EotMessage, s: &EotStatus) -> Vec<Field> {
    fields(&[
        ("Unit", text_of("Rear (EOT)")),
        ("Address", text_of(m.unit_address)),
        ("Brake pipe", number(Some(s.pressure_psig), Unit::Psi, 0)),
        ("Motion", flag(Some(s.motion))),
        ("Marker light", flag(Some(s.marker_light))),
        (
            "Marker battery",
            text_of(if s.marker_battery_low { "low" } else { "ok" }),
        ),
        ("Battery", text_of(eot_battery(s.battery))),
        (
            "Charge",
            number(Some(s.battery_charge_pct), Unit::Percent, 0),
        ),
        (
            "Valve circuit",
            text_of(if s.valve_ok { "ok" } else { "fail" }),
        ),
        ("Turbine", flag(Some(s.turbine))),
        ("Arming", text_of(eot_arming(s.arming))),
        ("Message type", count(Some(s.message_type))),
    ])
}

fn eot_battery(battery: EotBattery) -> &'static str {
    match battery {
        EotBattery::NotMonitored => "not monitored",
        EotBattery::VeryLow => "very low",
        EotBattery::Low => "low",
        EotBattery::Ok => "ok",
    }
}

fn eot_arming(arming: EotArming) -> &'static str {
    match arming {
        EotArming::Normal => "normal",
        EotArming::Arming => "arming",
        EotArming::Armed => "armed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LatLon;

    fn message(details: serde_json::Value) -> DataLinkMessage {
        serde_json::from_value(serde_json::json!({
            "message_type": "Position report", "station": " .D-AIBC ", "text": " FL370 ",
            "crc_ok": true, "fec_corrected": 3, "snr_db": 12.25,
            "frequency_error_hz": -41.5, "lat": 52.5, "lon": 13.4, "details": details,
        }))
        .unwrap()
    }

    #[test]
    fn every_data_link_kind_shows_quality_place_and_text() {
        let view = message(serde_json::json!({ "level": 3 })).present(EventKind::Hfdl);
        assert_eq!(
            view.labels(),
            [
                "Type",
                "Station",
                "Integrity",
                "FEC repaired",
                "SNR",
                "Frequency error",
                "Position",
                "Text",
                "Details"
            ]
        );
        assert_eq!(view.field("Station"), text(Some(".D-AIBC")).as_ref());
        assert_eq!(view.field("Integrity"), text(Some("verified")).as_ref());
        assert_eq!(view.field("SNR"), number(Some(12.25), Unit::Db, 1).as_ref());
        assert_eq!(
            view.field("Frequency error"),
            number(Some(-41.5), Unit::Hz, 1).as_ref()
        );
        assert_eq!(
            view.field("Position"),
            Some(&FieldValue::Position(LatLon {
                lat: 52.5,
                lon: 13.4
            }))
        );
        assert_eq!(view.field("Text"), text(Some("FL370")).as_ref());
        assert_eq!(
            view.field("Details"),
            text(Some("{\n  \"level\": 3\n}")).as_ref()
        );
    }

    #[test]
    fn empty_details_show_nothing() {
        let view = message(serde_json::json!({})).present(EventKind::Dsc);
        assert_eq!(view.field("Details"), None);
    }

    fn eot(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<EotMessage>(json)
            .unwrap()
            .present(EventKind::Eot)
    }

    #[test]
    fn an_eot_rear_unit_reports_its_brake_pipe_and_battery() {
        let view = eot(serde_json::json!({
            "unit_address": 23456, "errors_corrected": 1,
            "report": {
                "unit": "rear", "message_type": 0, "arming": "normal", "pressure_psig": 87,
                "battery": "low", "battery_charge_pct": 63, "valve_ok": true, "confirmed": false,
                "turbine": true, "motion": true, "marker_light": true,
                "marker_battery_low": false, "discretionary": false, "chaining": 3,
            },
        }));
        assert_eq!(
            view.field("Brake pipe"),
            number(Some(87), Unit::Psi, 0).as_ref()
        );
        assert_eq!(view.field("Battery"), text(Some("low")).as_ref());
        assert_eq!(view.field("Repaired"), count(Some(1)).as_ref());
    }

    #[test]
    fn a_hot_command_names_its_code_and_copies() {
        let view = eot(serde_json::json!({
            "unit_address": 23456, "errors_corrected": 0,
            "report": { "unit": "head", "command": "other", "code": 60, "copies": 2 },
        }));
        assert_eq!(view.field("Command"), text(Some("command 0x3c")).as_ref());
        assert_eq!(view.field("Copies"), text(Some("2/3")).as_ref());
        assert_eq!(view.field("Repaired"), None);
    }
}
