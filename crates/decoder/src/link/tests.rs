use std::collections::BTreeMap;

use sdrmm_wire::LatLon;

use super::*;

const ALL: Fields = Fields::station(&["from", "tail", "icao"]);

#[expect(
    clippy::needless_pass_by_value,
    reason = "callers pass a json! literal"
)]
fn sample(details: Value) -> DataLinkMessage {
    serde_json::from_value(serde_json::json!({
        "message_type": "Position report", "station": " .D-AIBC ", "text": " FL370 ",
        "crc_ok": true, "fec_corrected": 3, "snr_db": 12.25,
        "frequency_error_hz": -41.5, "lat": 52.5, "lon": 13.4, "details": details,
    }))
    .expect("data-link sample")
}

#[test]
fn normalizes_nested_protocol_fields() {
    let value = serde_json::json!({
        "type": "hfdl",
        "kind": "position",
        "details": {"icao": "ABC123", "lat": 1.5, "lon": -2.5}
    });
    assert_eq!(find_string(&value, &["kind"]), Some("position".to_owned()));
    assert_eq!(find_string(&value, &["icao"]), Some("ABC123".to_owned()));
    assert_eq!(find_number(&value, &["lat"]), Some(1.5));
    assert_eq!(find_number(&value, &["lon"]), Some(-2.5));
    let message = super::message(&value, &ALL, Quality::default(), Some(&[0xab, 0xcd]));
    assert_eq!(message.message_type, "position");
    assert_eq!(message.station.as_deref(), Some("ABC123"));
    assert_eq!(message.raw.as_deref(), Some("abcd"));
}

#[test]
fn reaches_fields_nested_inside_arrays() {
    let value = serde_json::json!({
        "reports": [{"icao": "ABC123", "lat": 1.5, "lon": -2.5}]
    });
    assert_eq!(find_string(&value, &["icao"]), Some("ABC123".to_owned()));
    assert_eq!(find_number(&value, &["lat"]), Some(1.5));
    assert_eq!(find_number(&value, &["lon"]), Some(-2.5));
}

#[test]
fn prefers_the_earlier_name_over_a_later_one() {
    let value = serde_json::json!({"tail": "VT-ANB", "from": "A1B2C3"});
    assert_eq!(
        find_string(&value, &["from", "tail"]),
        Some("A1B2C3".to_owned())
    );
    assert_eq!(
        find_string(&value, &["tail", "from"]),
        Some("VT-ANB".to_owned())
    );
}

#[test]
fn picks_siblings_in_a_stable_order() {
    let first = serde_json::json!({"a": {"lat": 1.0}, "b": {"lat": 2.0}});
    let second = serde_json::json!({"b": {"lat": 2.0}, "a": {"lat": 1.0}});
    assert_eq!(find_number(&first, &["lat"]), Some(1.0));
    assert_eq!(find_number(&second, &["lat"]), Some(1.0));
}

#[test]
fn only_the_declared_station_fields_name_the_station() {
    let value = serde_json::json!({"mmsi": 211_000_000, "tail": "D-AIBC"});
    let ship = Fields::station(&["mmsi"]);
    assert_eq!(
        super::message(&value, &ship, Quality::default(), None)
            .station
            .as_deref(),
        Some("211000000")
    );
    assert_eq!(
        super::message(&value, &Fields::station(&[]), Quality::default(), None).station,
        None
    );
}

#[test]
fn a_serialization_failure_never_looks_decoded() {
    let mut body = BTreeMap::new();
    body.insert(vec![1u8], 2u8);
    let quality = Quality {
        crc_ok: true,
        fec_corrected: Some(2),
        snr_db: Some(12.5),
        frequency_error_hz: Some(-3.0),
    };
    let failed = super::message(&body, &ALL, quality, Some(&[0xab, 0xcd]));
    assert_eq!(failed.message_type, SERIALIZATION_ERROR);
    assert!(!failed.crc_ok);
    assert_eq!(failed.station, None);
    assert_eq!(failed.raw.as_deref(), Some("abcd"));
    assert!(
        super::message(
            &serde_json::json!({"type": "undecoded"}),
            &ALL,
            quality,
            None
        )
        .crc_ok
    );
}

#[test]
fn every_data_link_kind_shows_quality_place_and_text() {
    let view = sample(serde_json::json!({ "level": 3 })).present(&EventKind::named("hfdl"));
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
    let view = sample(serde_json::json!({})).present(&EventKind::named("dsc"));
    assert_eq!(view.field("Details"), None);
}

#[test]
fn a_message_reads_back_from_its_event() {
    let message = sample(serde_json::json!({}));
    let event = sdrmm_wire::DecoderEvent::tagged(EventKind::named("vdl2"), &message);
    assert_eq!(read(&event, "vdl2"), Some(message));
    assert_eq!(read(&event, "hfdl"), None);
}
