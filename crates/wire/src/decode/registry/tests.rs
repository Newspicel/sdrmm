use serde_json::json;

use super::*;
use crate::{
    LatLon,
    decode::{
        platform_kinds,
        test_kinds::{self, FEED, ProbeStation, ProbeText, STATION, TEXT, TRACK},
    },
};

fn local() -> EventCatalog {
    EventCatalog::new(platform_kinds().into_iter().chain(test_kinds::kinds()))
        .with_feeds(test_kinds::feeds())
}

fn set(kinds: &[EventKind]) -> KindSet {
    kinds.iter().cloned().collect()
}

const HOME: LatLon = LatLon {
    lat: 50.4,
    lon: 6.6,
};

#[test]
fn a_kind_is_a_plain_string_on_the_wire() {
    let kind = EventKind::named("probe_text");
    assert_eq!(
        serde_json::to_value(&kind).expect("json"),
        json!("probe_text")
    );
    let back: EventKind = serde_json::from_value(json!("probe_text")).expect("parses");
    assert_eq!(back, kind);
    assert_eq!(EventKind::new("probe_text"), kind);
    assert_eq!(kind.to_string(), "probe_text");
    assert_eq!(format!("{kind:?}"), "EventKind(probe_text)");
    assert!(kind == "probe_text");
    let ghost: EventKind = serde_json::from_value(json!("ghost")).expect("any name parses");
    assert_eq!(ghost.as_str(), "ghost");
}

#[test]
fn an_event_is_its_kind_and_its_data() {
    let event = ProbeText::event("CQ");
    let json = serde_json::to_value(&event).expect("json");
    assert_eq!(
        json,
        json!({ "kind": "probe_text", "data": { "text": "CQ" } })
    );
    let back: DecoderEvent = serde_json::from_value(json).expect("parses");
    assert_eq!(back, event);
    assert_eq!(event.kind(), TEXT);
}

#[test]
fn read_checks_the_kind_and_payload_does_not() {
    let text = ProbeText {
        text: "CQ".to_owned(),
    };
    let event = DecoderEvent::of(&text);
    assert!(event.is::<ProbeText>());
    assert_eq!(event.read::<ProbeText>(), Some(text.clone()));
    assert_eq!(event.read::<ProbeStation>(), None);

    let shared = DecoderEvent::tagged(EventKind::named("probe_other"), &text);
    assert_eq!(shared.kind, "probe_other");
    assert!(!shared.is::<ProbeText>());
    assert_eq!(shared.read::<ProbeText>(), None);
    assert_eq!(shared.payload::<ProbeText>(), Some(text));
    assert_eq!(shared.payload::<ProbeStation>(), None);
}

#[test]
fn a_kind_set_does_set_algebra() {
    let left = set(&[STATION, TEXT]);
    let right = set(&[STATION, TRACK]);
    assert!(KindSet::EMPTY.is_empty());
    assert_eq!(left.len(), 2);
    assert!(left.contains(&TEXT));
    assert!(!left.contains(&TRACK));
    assert_eq!(left.intersection(&right), KindSet::EMPTY.with(STATION));
    assert_eq!(left.without(&right), KindSet::EMPTY.with(TEXT));
    assert_eq!(left.union(&right), set(&[STATION, TEXT, TRACK]));
    assert_eq!(left.union(&left), left);
    assert_eq!(left.clone().with(STATION), left);
}

#[test]
fn a_kind_set_is_a_sorted_list_on_the_wire() {
    let kinds = set(&[TRACK, EventKind::CALL, STATION]);
    assert_eq!(
        serde_json::to_value(&kinds).expect("json"),
        json!(["call", "probe_station", "probe_track"])
    );
    let back: KindSet =
        serde_json::from_value(json!(["probe_track", "call", "probe_station", "call"]))
            .expect("parses");
    assert_eq!(back, kinds);
}

#[test]
fn the_catalog_derives_sets_from_capabilities() {
    let catalog = local();
    assert_eq!(catalog.with_cap(Capability::Track), set(&[TRACK]));
    assert_eq!(
        catalog.with_cap(Capability::Voice),
        set(&[EventKind::CALL, EventKind::DV])
    );
    assert!(catalog.has(&STATION, Capability::Geo));
    assert!(!catalog.has(&EventKind::named("ghost"), Capability::Log));
    assert_eq!(catalog.parse("probe_text"), Some(TEXT));
    assert_eq!(catalog.parse("ghost"), None);
    assert_eq!(catalog.entries().len(), catalog.all().len());
    assert_eq!(
        catalog.all().len(),
        platform_kinds().len() + test_kinds::kinds().len()
    );
}

#[test]
fn an_entry_serializes_what_its_spec_says() {
    let catalog = local();
    let entry = catalog.entry(&STATION).expect("probe station");
    let json = serde_json::to_value(entry).expect("json");
    assert_eq!(json["kind"], "probe_station");
    assert_eq!(json["name"], "Probe station");
    assert_eq!(json["noun"], "Stations");
    assert_eq!(json["caps"], json!(["log", "station", "geo"]));
    assert_eq!(json["symbol"], "plane");
    assert_eq!(json["hue"], "teal");
    assert_eq!(
        json["lifetime"],
        json!({ "type": "sighting", "ttl_s": SIGHTING_S })
    );
}

#[test]
fn the_catalog_presents_through_the_kinds_codec() {
    let view = local().present(&ProbeStation::event("S1", Some("Tower"), Some(HOME)));
    assert_eq!(view.summary, "Tower");
    assert_eq!(view.entity.as_deref(), Some("S1"));
    assert_eq!(
        view.label.as_deref(),
        Some("S1"),
        "a label falls back to the entity"
    );
    assert_eq!(view.place.map(|place| place.at), Some(HOME));
}

#[test]
fn an_event_nobody_can_read_still_presents_and_says_why() {
    let catalog = local();
    let ghost = DecoderEvent::tagged(EventKind::named("ghost"), &json!({}));
    assert!(catalog.present(&ghost).summary.contains("ghost"));
    let broken = DecoderEvent::tagged(STATION, &json!(5));
    assert!(catalog.present(&broken).summary.contains("probe_station"));
}

#[test]
fn merge_overlays_what_the_newer_event_states() {
    let catalog = local();
    let mut older = ProbeStation::event("S1", None, Some(HOME));
    let newer = ProbeStation::event("S1", Some("Tower"), None);
    assert!(catalog.merge(&mut older, &newer));
    assert_eq!(
        older.read::<ProbeStation>(),
        Some(ProbeStation {
            id: "S1".to_owned(),
            name: Some("Tower".to_owned()),
            at: Some(HOME),
        })
    );
}

#[test]
fn events_of_different_kinds_never_merge() {
    let catalog = local();
    let mut older = ProbeStation::event("S1", None, Some(HOME));
    let before = older.clone();
    assert!(!catalog.merge(&mut older, &ProbeText::event("S1")));
    assert_eq!(older, before);
}

#[test]
fn an_unregistered_kind_merges_field_by_field() {
    let catalog = local();
    let ghost = |data: serde_json::Value| DecoderEvent::tagged(EventKind::named("ghost"), &data);
    let mut older = ghost(json!({ "a": 1, "b": 2 }));
    assert!(catalog.merge(&mut older, &ghost(json!({ "b": 3, "c": null }))));
    assert_eq!(older.data(), json!({ "a": 1, "b": 3 }));
}

#[test]
fn strip_clip_drops_the_audio_its_codec_knows() {
    let catalog = local();
    let mut heard = DecoderEvent::of(&test_kinds::transmission());
    assert!(heard.data().get("audio").is_some());
    catalog.strip_clip(&mut heard);
    assert!(heard.data().get("audio").is_none());
    assert_eq!(heard.data()["id"], 1);

    let mut ghost = DecoderEvent::tagged(EventKind::named("ghost"), &json!({ "audio": "kept" }));
    catalog.strip_clip(&mut ghost);
    assert_eq!(ghost.data(), json!({ "audio": "kept" }));
}

#[test]
fn a_feed_names_the_kinds_it_encodes() {
    let catalog = local();
    assert_eq!(catalog.feed_kinds(FEED), set(&[STATION]));
    assert_eq!(catalog.feed_kinds("ghost"), KindSet::EMPTY);
    let entries = catalog.feed_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].format, FEED);
    let encoder = &catalog.feed(FEED).expect("probe feed").encoder;
    let station = ProbeStation::event("S1", None, None);
    assert_eq!(encoder.encode(&station), Ok(b"S1".to_vec()));
    assert!(encoder.encode(&ProbeText::event("CQ")).is_err());
}

#[test]
fn the_process_catalog_answers_for_installed_kinds() {
    test_kinds::install();
    assert_eq!(EventKind::parse("probe_track"), Some(TRACK));
    assert!(TRACK.has(Capability::Track));
    assert_eq!(TRACK.name(), "Probe track");
    assert_eq!(EventKind::named("ghost").name(), "ghost");
    assert!(KindSet::all().contains(&EventKind::TRANSMISSION));
    let view = ProbeText::event("CQ").present();
    assert_eq!(view.text.as_deref(), Some("CQ"));
}

#[test]
fn a_repeated_field_key_is_numbered_so_each_names_one_field() {
    use crate::present::{Field, FieldValue, unique_keys};
    let text = |key: &str| Field::new(key, "Label", FieldValue::Text(key.to_owned()));
    let keys: Vec<String> = unique_keys(vec![text("volume"), text("volume_2"), text("volume")])
        .into_iter()
        .map(|field| field.key)
        .collect();
    assert_eq!(keys, ["volume", "volume_2", "volume_3"]);
}

#[test]
fn a_view_names_every_field_it_reads() {
    let table = KindView::table(&["callsign", "altitude"])
        .sorted("altitude", true)
        .keyed("icao");
    assert_eq!(table.fields(), ["callsign", "altitude", "altitude", "icao"]);
    assert_eq!(KindView::meter("per", 0.0, 100.0).fields(), ["per"]);
    assert_eq!(KindView::trend(&["nf", "y"], 60).fields(), ["nf", "y"]);
    assert!(KindView::Picture.fields().is_empty());
}
