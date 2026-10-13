use super::*;
use crate::{
    AdsbMessage, AprsPacket, DecoderEvent, EventKind, RadiosondeFrame, WsjtMessage,
    decode::{
        Capability,
        fixtures::{fixture, fixture_count},
    },
    lora::{LoraFrame, LoraPayload, MeshtasticContent},
    remote_id::{RemoteIdFrame, RemoteIdMessage},
};

mod shapes;

fn view(kind: EventKind) -> Presentation {
    fixture(kind).present()
}

#[test]
fn every_kind_has_one_fixture_of_its_own_kind() {
    assert_eq!(fixture_count(), EventKind::ALL.len());
    for kind in EventKind::ALL {
        assert_eq!(fixture(*kind).kind(), *kind);
    }
}

#[test]
fn every_kind_summarises_its_fixture() {
    for kind in EventKind::ALL {
        assert!(!view(*kind).summary.trim().is_empty(), "{kind} summary");
    }
}

#[test]
fn capabilities_match_the_presentation_both_ways() {
    for kind in EventKind::ALL {
        let view = view(*kind);
        let checks = [
            (Capability::Station, view.entity.is_some()),
            (Capability::Geo, view.place.is_some()),
            (Capability::Voice, view.voice.is_some()),
            (Capability::Duration, view.duration_ms.is_some()),
            (Capability::Text, view.text.is_some()),
            (Capability::Spot, view.spot.is_some()),
            (Capability::Bearing, view.bearing.is_some()),
        ];
        for (cap, shown) in checks {
            assert_eq!(kind.has(cap), shown, "{kind} {cap:?}");
        }
        assert!(
            kind.has(Capability::Clip) || view.clip.is_none(),
            "{kind} clip"
        );
        assert_eq!(
            kind.has(Capability::Geo),
            kind.info().symbol.is_some(),
            "{kind} symbol"
        );
    }
}

#[test]
fn clip_kinds_show_their_clip() {
    for kind in EventKind::ALL
        .iter()
        .filter(|kind| kind.has(Capability::Clip))
    {
        assert!(view(*kind).clip.is_some(), "{kind}");
    }
}

#[test]
fn stripping_a_clip_leaves_the_event_without_audio() {
    for kind in EventKind::ALL {
        let mut event = fixture(*kind);
        event.strip_clip();
        assert!(event.present().clip.is_none(), "{kind}");
    }
}

#[test]
fn the_label_falls_back_to_the_entity() {
    let rds = view(EventKind::Rds);
    assert_eq!(rds.label.as_deref(), Some("RADIO 1"));
    let pager = view(EventKind::Pocsag);
    assert_eq!(pager.label.as_deref(), Some("1234567"));
    assert_eq!(view(EventKind::Rtty).label, None);
}

#[test]
fn an_aircraft_is_named_placed_and_turned() {
    let view = view(EventKind::Adsb);
    assert_eq!(view.entity.as_deref(), Some("3C6444"));
    assert_eq!(view.label.as_deref(), Some("DLH400"));
    let place = view.place.expect("placed");
    assert_eq!(
        place.at,
        crate::LatLon {
            lat: 52.5,
            lon: 13.4
        }
    );
    assert_eq!(place.heading_deg, Some(270.0));
    assert_eq!(place.altitude_m.map(f32::round), Some(11_278.0));
}

#[test]
fn a_vessel_without_heading_points_along_its_course() {
    let place = view(EventKind::Ais).place.expect("placed");
    assert_eq!(place.heading_deg, Some(120.0));
}

#[test]
fn a_radar_track_is_keyed_by_track_and_named_by_its_aircraft() {
    let view = view(EventKind::Radar);
    assert_eq!(view.entity.as_deref(), Some("T7"));
    assert_eq!(view.label.as_deref(), Some("3C6444"));
}

#[test]
fn pictures_name_no_station() {
    for kind in [
        EventKind::Sstv,
        EventKind::Apt,
        EventKind::Lrpt,
        EventKind::Wefax,
    ] {
        assert_eq!(view(kind).entity, None, "{kind}");
    }
}

fn ft8(text: &str) -> DecoderEvent {
    DecoderEvent::Ft8(WsjtMessage {
        text: text.to_owned(),
        snr_db: 0.0,
        audio_hz: 0.0,
        time_offset_s: 0.0,
        hard_errors: 0,
    })
}

#[test]
fn a_wsjt_message_names_its_sender() {
    let sender = |text: &str| ft8(text).present().callsign;
    assert_eq!(sender("CQ DL1ABC JO62").as_deref(), Some("DL1ABC"));
    assert_eq!(sender("CQ DX DL1ABC JO62").as_deref(), Some("DL1ABC"));
    assert_eq!(sender("CQ POTA W1AW FN31").as_deref(), Some("W1AW"));
    assert_eq!(sender("W1AW DL1ABC -12").as_deref(), Some("DL1ABC"));
    assert_eq!(sender("CQ"), None);
}

#[test]
fn only_amateur_decoders_carry_a_callsign() {
    let aprs = DecoderEvent::Aprs(AprsPacket {
        source: "DL1ABC-9".to_owned(),
        ..AprsPacket::default()
    });
    assert_eq!(aprs.present().callsign.as_deref(), Some("DL1ABC-9"));
    for kind in [EventKind::Wspr, EventKind::Ft8, EventKind::Ft4] {
        assert!(view(kind).callsign.is_some(), "{kind}");
    }
    assert_eq!(view(EventKind::Adsb).callsign, None);
}

fn adsb(data: serde_json::Value) -> DecoderEvent {
    let mut message = serde_json::json!({ "df": 17, "raw": "8d4840d6" });
    if let (Some(base), serde_json::Value::Object(extra)) = (message.as_object_mut(), data) {
        base.extend(extra);
    }
    DecoderEvent::Adsb(serde_json::from_value::<AdsbMessage>(message).expect("adsb"))
}

#[test]
fn partial_aircraft_frames_build_one_aircraft() {
    let mut aircraft = adsb(serde_json::json!({
        "icao": "abc123", "callsign": "DLH400", "type_code": 4
    }));
    let position = adsb(serde_json::json!({
        "icao": "abc123", "lat": 52.5, "lon": 13.4, "altitude_ft": 37_000, "type_code": 11
    }));
    assert!(aircraft.merge(&position));
    let DecoderEvent::Adsb(merged) = &aircraft else {
        panic!("still an aircraft");
    };
    assert_eq!(merged.callsign.as_deref(), Some("DLH400"));
    assert_eq!((merged.lat, merged.lon), (Some(52.5), Some(13.4)));
    assert_eq!(merged.altitude_ft, Some(37_000));
    assert_eq!(merged.type_code, Some(11));
}

#[test]
fn a_newer_sonde_frame_wins_where_it_has_a_value() {
    let DecoderEvent::Radiosonde(first) = fixture(EventKind::Radiosonde) else {
        panic!("a sonde");
    };
    let mut sonde = DecoderEvent::Radiosonde(first.clone());
    let climbed = DecoderEvent::Radiosonde(RadiosondeFrame {
        altitude_m: Some(12_050.0),
        lat: None,
        lon: None,
        ..first
    });
    assert!(sonde.merge(&climbed));
    let DecoderEvent::Radiosonde(merged) = sonde else {
        panic!("a sonde");
    };
    assert_eq!(merged.altitude_m, Some(12_050.0));
    assert_eq!((merged.lat, merged.lon), (Some(52.2), Some(14.1)));
}

#[test]
fn different_kinds_never_merge() {
    let mut aircraft = fixture(EventKind::Adsb);
    assert!(!aircraft.merge(&fixture(EventKind::Ais)));
    assert_eq!(aircraft, fixture(EventKind::Adsb));
}

fn lora_with(content: MeshtasticContent) -> LoraFrame {
    let DecoderEvent::Lora(mut frame) = fixture(EventKind::Lora) else {
        panic!("a lora frame");
    };
    if let Some(LoraPayload::Meshtastic(packet)) = frame.decoded.as_mut() {
        packet.content = Some(content);
    }
    frame
}

#[test]
fn a_lora_node_keeps_its_last_position_across_frames_without_one() {
    let DecoderEvent::Lora(placed) = fixture(EventKind::Lora) else {
        panic!("a lora frame");
    };
    let mut node = DecoderEvent::Lora(placed);
    let text = DecoderEvent::Lora(lora_with(MeshtasticContent::Text {
        text: "hi".to_owned(),
    }));
    assert!(node.merge(&text));
    let view = node.present();
    assert_eq!(view.entity.as_deref(), Some("!000000a1"));
    assert_eq!(
        view.place.map(|place| place.at),
        Some(crate::LatLon {
            lat: 47.5,
            lon: 8.25
        })
    );
}

fn drone(messages: Vec<RemoteIdMessage>, uas_id: Option<&str>) -> RemoteIdFrame {
    let DecoderEvent::RemoteId(frame) = fixture(EventKind::RemoteId) else {
        panic!("a drone");
    };
    RemoteIdFrame {
        messages,
        uas_id: uas_id.map(str::to_owned),
        ..frame
    }
}

#[test]
fn a_drone_keeps_earlier_messages_until_a_newer_one_of_the_same_kind() {
    let DecoderEvent::RemoteId(located) = fixture(EventKind::RemoteId) else {
        panic!("a drone");
    };
    let basic = RemoteIdMessage::BasicId {
        id_type: crate::remote_id::UasIdType::SerialNumber,
        ua_type: crate::remote_id::UaType::Rotorcraft,
        uas_id: "1581F5FJD239C00DW22E".to_owned(),
    };
    let operator = RemoteIdMessage::OperatorId {
        id_type: 0,
        operator_id: "FIN87astrdge12k8".to_owned(),
    };
    let mut first = drone(vec![basic, operator], Some("1581F5FJD239C00DW22E"));
    first.merge(&drone(located.messages, None));
    let kinds: Vec<_> = first
        .messages
        .iter()
        .map(|message| match message {
            RemoteIdMessage::Location(_) => "location",
            RemoteIdMessage::BasicId { .. } => "basic_id",
            RemoteIdMessage::OperatorId { .. } => "operator_id",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["location", "basic_id", "operator_id"]);
    assert_eq!(first.uas_id.as_deref(), Some("1581F5FJD239C00DW22E"));
    assert!(
        first
            .present(EventKind::RemoteId)
            .place
            .is_some_and(|place| place.heading_deg == Some(74.0))
    );
}

#[test]
fn helpers_drop_what_cannot_be_shown() {
    assert_eq!(trimmed(Some("  ")), None);
    assert_eq!(trimmed(Some(" DLH ")).as_deref(), Some("DLH"));
    assert_eq!(bearing(Some(-10.0)), Some(350.0));
    assert_eq!(bearing(Some(f64::NAN)), None);
    assert_eq!(lat_lon(Some(91.0), Some(0.0)), None);
    assert_eq!(lat_lon(Some(52.0), None), None);
    assert_eq!(number(Some(f64::INFINITY), Unit::Feet, 0), None);
    let shown = fields(&[
        ("Altitude", number(Some(37_000), Unit::Feet, 0)),
        ("Squawk", None),
    ]);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].label, "Altitude");
}

#[test]
fn every_kind_but_bare_text_lists_fields() {
    for kind in EventKind::ALL {
        let view = view(*kind);
        let bare_text = view.text.is_some() && view.text.as_deref() == Some(view.summary.as_str());
        assert!(!view.fields.is_empty() || bare_text, "{kind} has no fields");
    }
}
