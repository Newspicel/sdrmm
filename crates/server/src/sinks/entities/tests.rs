use sdrmm_wire::{AdsbMessage, DecoderEvent, EventKind, EventOrigin, RttyText};

use super::*;

fn from(source: &str, event: DecoderEvent) -> DecodedRecord {
    DecodedRecord {
        origin: Some(EventOrigin {
            node: source.to_owned(),
            transmission: 0,
        }),
        device_set: 1,
        channel: 1,
        at: "2026-09-21T10:00:00Z".to_owned(),
        freq_hz: 1_090_000_000.0,
        view: Some(event.present()),
        event,
        sinks: Vec::new(),
        callsign: None,
    }
}

fn adsb(callsign: Option<&str>, place: Option<(f64, f64)>) -> DecoderEvent {
    DecoderEvent::Adsb(AdsbMessage {
        icao: "3c6444".to_owned(),
        df: 17,
        callsign: callsign.map(str::to_owned),
        lat: place.map(|(lat, _)| lat),
        lon: place.map(|(_, lon)| lon),
        ..AdsbMessage::default()
    })
}

fn observe(entities: &mut Entities, record: &DecodedRecord, now: Instant) -> Change {
    let view = record.view.clone().expect("the router stamped a view");
    entities.observe(record, &view, now)
}

#[test]
fn a_callsign_frame_and_a_position_frame_build_one_plane() {
    let mut entities = Entities::default();
    let now = Instant::now();

    observe(
        &mut entities,
        &from("adsb", adsb(Some("DLH123"), None)),
        now,
    );
    let change = observe(
        &mut entities,
        &from("adsb", adsb(None, Some((52.5, 13.4)))),
        now,
    );

    let all = entities.all();
    assert_eq!(all.len(), 1);
    let plane = &all[0];
    assert_eq!(change.upserted.as_ref(), Some(&plane.key));
    assert_eq!(plane.key.id, "3C6444");
    assert_eq!(plane.frames, 2);
    assert_eq!(plane.view.label.as_deref(), Some("DLH123"));
    assert!(plane.view.place.is_some(), "the merged plane keeps its fix");
    assert_eq!(plane.trail.len(), 1);
    assert_eq!(plane.symbol, EventKind::Adsb.info().symbol);
}

#[test]
fn two_decoders_of_one_kind_are_two_groups() {
    let mut entities = Entities::default();
    let now = Instant::now();

    observe(
        &mut entities,
        &from("north", adsb(Some("DLH123"), None)),
        now,
    );
    observe(
        &mut entities,
        &from("south", adsb(Some("DLH123"), None)),
        now,
    );

    let sources: Vec<String> = entities
        .all()
        .into_iter()
        .map(|entity| entity.key.source)
        .collect();
    assert_eq!(sources, ["north", "south"]);
}

#[test]
fn an_instant_kind_never_becomes_an_entity() {
    let mut entities = Entities::default();
    let rtty = DecoderEvent::Rtty(RttyText {
        text: "CQ".to_owned(),
    });

    let change = observe(&mut entities, &from("rtty", rtty), Instant::now());

    assert_eq!(change, Change::default());
    assert!(entities.all().is_empty());
}

#[test]
fn a_sighting_ages_out_after_its_ttl() {
    let mut entities = Entities::default();
    let start = Instant::now();
    observe(
        &mut entities,
        &from("adsb", adsb(Some("DLH123"), None)),
        start,
    );

    assert!(
        entities
            .age_out(start + Duration::from_secs(299))
            .is_empty()
    );
    let gone = entities.age_out(start + Duration::from_secs(301));

    assert_eq!(gone.len(), 1);
    assert!(entities.all().is_empty());
}

#[test]
fn a_trail_grows_only_when_the_place_moves() {
    let mut trail = Vec::new();
    let here = LatLon {
        lat: 52.5,
        lon: 13.4,
    };

    push_trail(&mut trail, here);
    push_trail(
        &mut trail,
        LatLon {
            lat: 52.500_01,
            lon: 13.4,
        },
    );
    push_trail(
        &mut trail,
        LatLon {
            lat: 52.51,
            lon: 13.4,
        },
    );

    assert_eq!(trail.len(), 2);
    for step in 0..100 {
        push_trail(
            &mut trail,
            LatLon {
                lat: 53.0 + f64::from(step) * 0.01,
                lon: 13.4,
            },
        );
    }
    assert_eq!(trail.len(), TRAIL_POINTS);
}

#[test]
fn the_oldest_entities_leave_when_the_sink_is_full() {
    let mut entities = Entities::default();
    let start = Instant::now();
    let mut evicted = 0;
    for n in 0..=MAX_ENTITIES {
        let event = DecoderEvent::Adsb(AdsbMessage {
            icao: format!("{n:06x}"),
            ..AdsbMessage::default()
        });
        let at = start + Duration::from_millis(n as u64);
        evicted += observe(&mut entities, &from("adsb", event), at).evicted;
    }

    assert_eq!(evicted, 1);
    assert_eq!(entities.all().len(), MAX_ENTITIES);
    assert!(
        !entities
            .all()
            .iter()
            .any(|entity| entity.key.id == "000000"),
        "the first plane was the oldest"
    );
}
