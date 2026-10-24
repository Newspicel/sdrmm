use super::*;
use crate::present::FieldValue;

const TORNADO: &str = "ZCZC-WXR-TOR-048201+0030-1232200-KEC55/NW-";

fn alert(text: &str) -> SameAlert {
    let Some(SameBody::Alert(alert)) = parse(text) else {
        panic!("{text} should parse as an alert");
    };
    alert
}

fn message(text: &str) -> SameMessage {
    SameMessage {
        body: parse(text).expect("parses"),
        copies: 3,
        raw: text.to_owned(),
    }
}

#[test]
fn a_tornado_warning_header_yields_every_field() {
    let alert = alert(TORNADO);
    assert_eq!(alert.originator, "WXR");
    assert_eq!(
        alert.originator_name.as_deref(),
        Some("National Weather Service")
    );
    assert_eq!(alert.event, "TOR");
    assert_eq!(alert.event_name.as_deref(), Some("Tornado Warning"));
    assert_eq!(alert.valid_minutes, 30);
    assert_eq!((alert.issued_day, alert.issued_hour), (123, 22));
    assert_eq!(alert.issued_minute, 0);
    assert_eq!(alert.station, "KEC55/NW");
    assert_eq!(
        alert.locations,
        vec![SameLocation {
            part: 0,
            state: 48,
            county: 201,
            name: Some("Harris, TX".to_owned()),
        }]
    );
}

#[test]
fn a_part_county_location_keeps_its_leading_digit() {
    let alert = alert("ZCZC-CIV-CEM-748201+0100-0010100-WABC/FM-");
    let location = &alert.locations[0];
    assert_eq!(location.part, 7);
    assert_eq!(location.code(), "748201");
    assert_eq!(location.label(), "south-west Harris, TX");
}

#[test]
fn an_unknown_county_falls_back_to_its_code() {
    let alert = alert("ZCZC-WXR-TOR-048999+0030-1232200-KEC55/NW-");
    assert_eq!(alert.locations[0].name, None);
    assert_eq!(alert.locations[0].label(), "048999");
}

#[test]
fn the_full_thirty_one_location_list_parses_and_a_thirty_second_does_not() {
    let codes = |count: usize| {
        (0..count)
            .map(|k| format!("0480{k:02}"))
            .collect::<Vec<_>>()
            .join("-")
    };
    let header = |count| format!("ZCZC-WXR-TOA-{}+0600-1232200-KEC55/NW-", codes(count));
    assert_eq!(alert(&header(MAX_SAME_LOCATIONS)).locations.len(), 31);
    assert_eq!(parse(&header(MAX_SAME_LOCATIONS + 1)), None);
}

#[test]
fn the_end_of_message_burst_carries_no_alert() {
    assert_eq!(parse("NNNN"), Some(SameBody::EndOfMessage));
    assert_eq!(message("NNNN").station(), None);
    assert_eq!(message("NNNN").summary(), "End of message");
}

#[test]
fn a_header_is_rejected_until_it_is_complete() {
    for len in 1..TORNADO.len() {
        assert_eq!(
            parse(&TORNADO[..len]),
            None,
            "{} parsed early",
            &TORNADO[..len]
        );
    }
    assert!(parse(TORNADO).is_some());
}

#[test]
fn malformed_headers_are_rejected() {
    for text in [
        "ZCZC-WXR-TOR-048201+0030-1232200-KEC55/NW",
        "ZCZC-WXR-TOR-048201+0030-1232200-KEC55/NW--",
        "ZCZC-WXR-TOR-048201+0030-1232200--",
        "ZCZC-wxr-TOR-048201+0030-1232200-KEC55/NW-",
        "ZCZC-WXR-TOR-04820A+0030-1232200-KEC55/NW-",
        "ZCZC-WXR-TOR-048201+0070-1232200-KEC55/NW-",
        "ZCZC-WXR-TOR-048201+0030-1232400-KEC55/NW-",
        "ZCZC-WXR-TOR-048201+0030-0002200-KEC55/NW-",
        "ZCZC-WXR-TOR-048201+0030-3672200-KEC55/NW-",
        "ZCZC-WXR-TOR-048201-0030-1232200-KEC55/NW-",
        "NNNNN",
        "",
    ] {
        assert_eq!(parse(text), None, "{text} should not parse");
    }
}

#[test]
fn a_space_padded_station_is_trimmed() {
    assert_eq!(
        alert("ZCZC-WXR-RWT-048201+0015-1232200-KGLD    -").station,
        "KGLD"
    );
}

#[test]
fn the_summary_names_the_event_the_area_and_the_station() {
    assert_eq!(
        message(TORNADO).summary(),
        "Tornado Warning · Harris, TX · KEC55/NW"
    );
}

#[test]
fn several_areas_collapse_to_the_first_and_a_count() {
    let alert = alert("ZCZC-WXR-TOA-048201-048157-048339+0600-1232200-KEC55/NW-");
    assert_eq!(alert.area(), "Harris, TX +2");
}

#[test]
fn the_valid_window_reads_in_hours_and_minutes() {
    let minutes = |valid_minutes| SameAlert {
        valid_minutes,
        ..alert(TORNADO)
    };
    assert_eq!(minutes(45).valid_for(), "45 min");
    assert_eq!(minutes(60).valid_for(), "1 h");
    assert_eq!(minutes(390).valid_for(), "6 h 30 min");
}

#[test]
fn the_presentation_lists_the_areas_and_the_burst_count() {
    let presented = message(TORNADO).present(EventKind::Same);
    assert_eq!(presented.entity.as_deref(), Some("KEC55/NW"));
    let field = |label: &str| {
        presented
            .fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| field.value.clone())
    };
    assert_eq!(
        field("Areas"),
        Some(FieldValue::Text("Harris, TX (048201)".to_owned()))
    );
    assert_eq!(field("Bursts"), Some(FieldValue::Text("3/3".to_owned())));
    assert_eq!(
        field("Valid for"),
        Some(FieldValue::Text("30 min".to_owned()))
    );
}
