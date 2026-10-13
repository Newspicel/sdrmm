use super::*;

const OLD_POSITION: &[&str] = &[
    "adsb",
    "ais",
    "aprs",
    "df",
    "df_fix",
    "dsc",
    "dv",
    "hfdl",
    "inmarsat_aero",
    "inmarsat_stdc",
    "iridium",
    "lora",
    "remote_id",
    "sarsat",
    "radar",
    "vdl2",
];

fn tags(set: KindSet) -> Vec<&'static str> {
    let mut tags: Vec<&str> = set.iter().map(EventKind::as_str).collect();
    tags.sort_unstable();
    tags
}

fn sorted(tags: &[&'static str]) -> Vec<&'static str> {
    let mut tags = tags.to_vec();
    tags.sort_unstable();
    tags
}

#[test]
fn every_kind_has_one_tag_for_serde_as_str_and_parse() {
    for &kind in EventKind::ALL {
        let json = serde_json::to_value(kind).expect("json");
        assert_eq!(json, kind.as_str());
        assert_eq!(EventKind::parse(kind.as_str()), Some(kind));
        assert_eq!(kind.to_string(), kind.as_str());
        let back: EventKind = serde_json::from_value(json).expect("parses");
        assert_eq!(back, kind);
    }
    assert_eq!(EventKind::parse("teletext"), None);
}

#[test]
fn an_event_carries_the_tag_of_its_kind() {
    let event = DecoderEvent::Rtty(RttyText {
        text: "CQ".to_owned(),
    });
    let json = serde_json::to_value(&event).expect("json");
    assert_eq!(json["kind"], event.kind().as_str());
    let shared = DecoderEvent::Ft4(WsjtMessage {
        text: "CQ DL1ABC JO31".to_owned(),
        snr_db: -12.0,
        audio_hz: 1_200.0,
        time_offset_s: 0.1,
        hard_errors: 0,
    });
    assert_eq!(shared.kind(), EventKind::Ft4);
    assert_eq!(serde_json::to_value(&shared).expect("json")["kind"], "ft4");
}

#[test]
fn derived_facets_equal_the_old_table() {
    let mut position = OLD_POSITION.to_vec();
    position.extend(["radiosonde", "nav", "radar_sites", "satellite"]);
    assert_eq!(tags(KindSet::with_cap(Capability::Geo)), sorted(&position));
    assert_eq!(
        tags(KindSet::with_cap(Capability::Voice)),
        sorted(&["call", "dv"])
    );
    assert_eq!(
        tags(KindSet::with_cap(Capability::Duration)),
        sorted(&["call", "transmission"])
    );
}

#[test]
fn rows_keep_their_invariants() {
    for &kind in EventKind::ALL {
        let info = kind.info();
        assert!(!info.name.is_empty() && !info.noun.is_empty(), "{kind}");
        assert_eq!(
            info.has(Capability::Station),
            info.lifetime != Lifetime::Instant,
            "{kind}: Station holds iff the kind is not instant"
        );
        assert_eq!(
            info.has(Capability::Geo),
            info.symbol.is_some(),
            "{kind}: Geo holds iff the row has a symbol"
        );
    }
}

#[test]
fn only_producer_kinds_skip_the_log() {
    let unlogged: Vec<EventKind> = EventKind::ALL
        .iter()
        .copied()
        .filter(|kind| !kind.has(Capability::Log))
        .collect();
    assert_eq!(
        unlogged,
        [
            EventKind::Broadcast,
            EventKind::RadarSites,
            EventKind::Nav,
            EventKind::Satellite
        ]
    );
}

#[test]
fn the_user_example_row_reads_as_specified() {
    let adsb = EventKind::Adsb.info();
    assert_eq!(adsb.name, "ADS-B");
    assert_eq!(adsb.noun, "Aircraft");
    assert_eq!(
        adsb.caps,
        &[Capability::Log, Capability::Station, Capability::Geo]
    );
    assert_eq!(adsb.symbol, Some(Symbol::Plane));
    assert_eq!(adsb.hue, Hue::Teal);
    assert_eq!(adsb.lifetime, Lifetime::Sighting { ttl_s: 300 });
}

#[test]
fn a_kind_set_does_set_algebra() {
    let air = KindSet::from_iter([EventKind::Adsb, EventKind::Acars]);
    let geo = KindSet::with_cap(Capability::Geo);
    assert!(KindSet::EMPTY.is_empty());
    assert_eq!(air.len(), 2);
    assert!(air.contains(EventKind::Acars));
    assert!(!air.contains(EventKind::Ais));
    assert_eq!(air.intersection(geo), KindSet::EMPTY.with(EventKind::Adsb));
    assert_eq!(air.without(geo), KindSet::EMPTY.with(EventKind::Acars));
    assert_eq!(air.union(geo).len(), geo.len() + 1);
    assert_eq!(air.union(air), air);
    assert_eq!(air.with(EventKind::Adsb), air);
    let every: KindSet = EventKind::ALL.iter().copied().collect();
    assert_eq!(every.len(), EventKind::ALL.len());
    assert_eq!(every.iter().collect::<Vec<_>>(), EventKind::ALL);
}

#[test]
fn a_kind_set_is_a_sorted_list_on_the_wire() {
    let set = KindSet::from_iter([EventKind::Call, EventKind::Transmission, EventKind::Adsb]);
    let json = serde_json::to_value(set).expect("json");
    assert_eq!(json, serde_json::json!(["transmission", "adsb", "call"]));
    let back: KindSet =
        serde_json::from_value(serde_json::json!(["call", "adsb", "call", "transmission"]))
            .expect("parses");
    assert_eq!(back, set);
    assert!(serde_json::from_value::<KindSet>(serde_json::json!(["teletext"])).is_err());
}

#[test]
fn an_entry_owns_what_its_row_says() {
    let entry = EventKind::Call.entry();
    let json = serde_json::to_value(&entry).expect("json");
    assert_eq!(json["kind"], "call");
    assert_eq!(json["name"], "Call");
    assert_eq!(json["noun"], "Calls");
    assert_eq!(
        json["caps"],
        serde_json::json!(["log", "station", "voice", "duration", "clip"])
    );
    assert!(json.get("symbol").is_none());
    assert_eq!(json["hue"], "accent");
    assert_eq!(
        json["lifetime"],
        serde_json::json!({ "type": "sighting", "ttl_s": 300 })
    );
    assert_eq!(EventKind::entries().len(), EventKind::ALL.len());
}
