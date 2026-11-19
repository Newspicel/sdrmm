use sdrmm_wire::{
    AimNode, Attitude, DecoderEvent, NO_CHANNEL, PatchNode, Position, SatelliteFix,
    SatelliteStatus, present::Place,
};

use super::*;

const MUNICH: LatLon = LatLon {
    lat: 48.137,
    lon: 11.575,
};

fn station() -> PositionFix {
    PositionFix {
        latitude: MUNICH.lat,
        longitude: MUNICH.lon,
        altitude_m: Some(500.0),
        accuracy_m: None,
        speed_mps: None,
        track_deg: None,
        time: String::new(),
        attitude: Attitude::default(),
    }
}

fn sonde(entity: &str, altitude_m: Option<f32>) -> Presentation {
    let mut place = Place::at(LatLon {
        lat: MUNICH.lat + 0.1,
        lon: MUNICH.lon,
    });
    place.altitude_m = altitude_m;
    Presentation {
        entity: Some(entity.to_owned()),
        place: Some(place),
        ..Presentation::default()
    }
}

fn node(id: &str) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body: NodeBody::Aim(AimNode::default()),
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn routed_to(sink: &str, view: Presentation) -> DecodedRecord {
    DecodedRecord {
        origin: None,
        device_set: 0,
        channel: NO_CHANNEL,
        at: String::new(),
        freq_hz: 0.0,
        event: DecoderEvent::of(&SatelliteFix::of(&SatelliteStatus::default())),
        sinks: vec![sink.to_owned()],
        view: Some(view),
        callsign: None,
    }
}

fn wiring(entity: &str, position: Option<&str>) -> Wiring {
    Wiring {
        node: "aim".to_owned(),
        settings: AimParams {
            entity: entity.to_owned(),
        },
        position: position.map(str::to_owned),
    }
}

#[test]
fn only_the_followed_entity_becomes_the_target() {
    let now = Instant::now();
    let settings = AimParams {
        entity: "S1".to_owned(),
    };
    assert!(target_of(&settings, &sonde("S1", None), now).is_some());
    assert!(target_of(&settings, &sonde("S2", None), now).is_none());
    let unplaced = Presentation {
        entity: Some("S1".to_owned()),
        ..Presentation::default()
    };
    assert!(target_of(&settings, &unplaced, now).is_none());
}

#[test]
fn a_sonde_aloft_to_the_north_is_pointed_at_up_and_north() {
    let now = Instant::now();
    let target = target_of(&AimParams::default(), &sonde("S1", Some(20_000.0)), now);
    let (status, sky) = aim(
        &wiring("", Some("gps")),
        Some(&station()),
        target.as_ref(),
        now,
    );
    let sky = sky.expect("aimed");
    assert!(sky.visible);
    assert!(sky.look.azimuth_deg < 1.0 || sky.look.azimuth_deg > 359.0);
    assert!(sky.look.elevation_deg > 50.0 && sky.look.elevation_deg < 70.0);
    assert!(sky.look.range_km > 20.0 && sky.look.range_km < 25.0);
    assert_eq!(status.entity.as_deref(), Some("S1"));
    assert!(status.error.is_none());
    assert!(status.waiting.is_none());
}

#[test]
fn a_target_without_altitude_stands_as_high_as_the_antenna() {
    let now = Instant::now();
    let target = target_of(&AimParams::default(), &sonde("S1", None), now);
    let (_, sky) = aim(
        &wiring("", Some("gps")),
        Some(&station()),
        target.as_ref(),
        now,
    );
    let sky = sky.expect("aimed");
    assert!(sky.look.elevation_deg.abs() < 0.2);
}

#[test]
fn each_missing_piece_is_a_wait_not_a_fault() {
    let now = Instant::now();
    let target = target_of(&AimParams::default(), &sonde("S1", None), now);
    let waiting = |(status, sky): (AimStatus, Option<SkyTrack>)| {
        assert!(sky.is_none());
        assert!(status.error.is_none());
        status.waiting
    };
    assert_eq!(
        waiting(aim(
            &wiring("", None),
            Some(&station()),
            target.as_ref(),
            now
        ))
        .as_deref(),
        Some(WIRE_A_POSITION)
    );
    assert_eq!(
        waiting(aim(&wiring("", Some("gps")), None, target.as_ref(), now)).as_deref(),
        Some(WAITING_FOR_FIX)
    );
    assert_eq!(
        waiting(aim(&wiring("", Some("gps")), Some(&station()), None, now)).as_deref(),
        Some(NO_TARGET)
    );
}

#[test]
fn a_record_reaches_only_the_aims_it_was_routed_to() {
    let hub = AimHub::default();
    let graph = PatchGraph {
        nodes: vec![node("aim"), node("other")],
        edges: Vec::new(),
    };
    let record = routed_to("aim", sonde("S1", Some(1_000.0)));
    hub.observe(&graph, &record, Instant::now());
    let targets = locked(&hub.targets);
    assert!(targets.contains_key("aim"));
    assert!(!targets.contains_key("other"));
}
