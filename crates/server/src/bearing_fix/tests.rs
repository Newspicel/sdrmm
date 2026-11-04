use std::time::Duration;

use sdrmm_wire::{
    DfBearing, EventContract, NodeKind, PatchEdge, PatchNode, PortRef, Position, WorkspaceSnapshot,
    fusion::BearingSource,
};

use super::*;
use crate::{ServerOptions, Store, router_with_state, tests::state_over};

const RECEIVER: LatLon = LatLon {
    lat: 50.03,
    lon: 8.57,
};
const NOISE_DEG: [f32; 6] = [0.9, -1.4, 1.6, -0.7, 1.2, -1.8];
const GIVE_UP: Duration = Duration::from_secs(60);

fn station(bearing_deg: f64, range_m: f64) -> LatLon {
    geo::destination(RECEIVER, bearing_deg, range_m)
}

fn radial(from: LatLon, noise_deg: f32) -> BearingRay {
    BearingRay {
        from,
        true_deg: geo::bearing_deg(from, RECEIVER) as f32 + noise_deg,
        sigma_deg: Some(2.0),
        confidence: 0.9,
    }
}

fn miss_m(fix: &BearingFix) -> f64 {
    geo::distance_m(
        LatLon {
            lat: fix.lat,
            lon: fix.lon,
        },
        RECEIVER,
    )
}

fn sitting(name: &str) -> Station {
    ("vor".to_owned(), name.to_owned())
}

#[test]
fn clean_radials_from_far_stations_cross_at_the_receiver() {
    let rays = [
        radial(station(280.0, 110_000.0), 0.0),
        radial(station(20.0, 90_000.0), 0.0),
    ];
    let fix = fix_of(&rays).expect("a fix");
    assert!(miss_m(&fix) < 100.0, "{} m: {fix:?}", miss_m(&fix));
    assert_eq!(fix.stations, 2);
}

#[test]
fn radials_cross_the_antimeridian() {
    let receiver = LatLon {
        lat: 1.0,
        lon: -179.5,
    };
    let ray = |from: LatLon| BearingRay {
        from,
        true_deg: geo::bearing_deg(from, receiver) as f32,
        sigma_deg: Some(2.0),
        confidence: 0.9,
    };
    let rays = [
        ray(LatLon {
            lat: 0.0,
            lon: 179.5,
        }),
        ray(LatLon {
            lat: 0.0,
            lon: -178.5,
        }),
    ];
    let fix = fix_of(&rays).expect("a fix");
    let at = LatLon {
        lat: fix.lat,
        lon: fix.lon,
    };
    assert!(geo::distance_m(at, receiver) < 100.0, "{fix:?}");
}

#[test]
fn two_noisy_stations_hold_the_receiver_inside_three_sigma() {
    let stations = [station(300.0, 45_000.0), station(40.0, 60_000.0)];
    for noise in NOISE_DEG.chunks(2) {
        let rays = stations
            .iter()
            .zip(noise)
            .map(|(&from, &noise)| radial(from, noise))
            .collect::<Vec<_>>();
        let fix = fix_of(&rays).expect("a fix");
        let off = miss_m(&fix);
        assert!(off < 3_000.0, "{off} m: {fix:?}");
        assert!(off < 1.5 * fix.ellipse_major_m, "{off} m: {fix:?}");
    }
}

#[test]
fn a_third_noisy_station_shrinks_the_ellipse() {
    let stations = [
        station(300.0, 45_000.0),
        station(40.0, 60_000.0),
        station(170.0, 70_000.0),
    ];
    let rays = |count: usize| {
        stations
            .iter()
            .take(count)
            .zip(NOISE_DEG)
            .map(|(&from, noise)| radial(from, noise))
            .collect::<Vec<_>>()
    };
    let two = fix_of(&rays(2)).expect("two stations");
    let three = fix_of(&rays(3)).expect("three stations");
    assert!(miss_m(&three) < 3_000.0, "{three:?}");
    assert!(
        three.ellipse_major_m * three.ellipse_minor_m < two.ellipse_major_m * two.ellipse_minor_m,
        "{two:?} {three:?}"
    );
    assert_eq!(three.stations, 3);
    assert!(three.spread_m > 0.0);
}

#[test]
fn the_newest_bearing_from_a_station_replaces_its_last() {
    let mut crossing = Crossing::default();
    let west = station(270.0, 40_000.0);
    let north = station(0.0, 40_000.0);
    assert!(
        crossing
            .observe(sitting("W"), radial(west, 0.0), 0.0)
            .is_none()
    );
    let wrong = crossing
        .observe(sitting("N"), radial(north, 15.0), 0.5)
        .expect("two stations");
    assert!(miss_m(&wrong) > 5_000.0, "{wrong:?}");
    assert!(
        crossing
            .observe(sitting("N"), radial(north, 0.0), 1.0)
            .is_none(),
        "at most one fix a second"
    );
    let right = crossing
        .observe(sitting("N"), radial(north, 0.0), 1.6)
        .expect("a second on");
    assert!(miss_m(&right) < 100.0, "{right:?}");
    assert_eq!(right.stations, 2);
}

#[test]
fn a_silent_station_drops_out_after_the_hold() {
    let mut crossing = Crossing::default();
    let west = station(270.0, 40_000.0);
    let north = station(0.0, 40_000.0);
    crossing.observe(sitting("W"), radial(west, 0.0), 0.0);
    assert!(
        crossing
            .observe(sitting("N"), radial(north, 0.0), 1.0)
            .is_some()
    );
    assert!(
        crossing
            .observe(sitting("N"), radial(north, 0.0), HOLD_S + 5.0)
            .is_none(),
        "one station left"
    );
}

#[test]
fn an_unplaced_station_is_told_once_and_leaves_the_fix() {
    let mut crossing = Crossing::default();
    crossing.observe(sitting("W"), radial(station(270.0, 40_000.0), 0.0), 0.0);
    assert!(crossing.unplaced(sitting("W")));
    assert!(!crossing.unplaced(sitting("W")));
    assert!(crossing.lines.is_empty());
    crossing.observe(sitting("W"), radial(station(270.0, 40_000.0), 0.0), 2.0);
    assert!(crossing.unplaced(sitting("W")), "told again once placed");
}

fn node(id: &str, kind: &str) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body: NodeBody::default_for(NodeKind::parse(kind).expect("a known kind")),
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn events(from: &str, to: &str) -> PatchEdge {
    PatchEdge {
        from: PortRef {
            node: from.to_owned(),
            port: "events".to_owned(),
        },
        to: PortRef {
            node: to.to_owned(),
            port: "events".to_owned(),
        },
    }
}

fn wired_graph() -> PatchGraph {
    PatchGraph {
        nodes: vec![
            node("df", "df"),
            node("fix", "bearing_fix"),
            node("map", "map"),
        ],
        edges: vec![events("df", "fix"), events("fix", "map")],
    }
}

fn bench() -> AppState {
    let store = Arc::new(Store::open(None).expect("store"));
    let snapshot = WorkspaceSnapshot {
        graph: wired_graph(),
        ..WorkspaceSnapshot::empty()
    };
    let id = store.create_workspace("fix", &snapshot).expect("create");
    store.activate_workspace(id).expect("activate");
    let state = state_over(store);
    let (_, background) = router_with_state(state.clone(), &ServerOptions::default());
    background.detach();
    state
}

fn bearing_record(name: &str, from: LatLon) -> DecodedRecord {
    let bearing = DfBearing {
        bearing_deg: geo::bearing_deg(from, RECEIVER) as f32,
        confidence: 0.9,
        lat: Some(from.lat),
        lon: Some(from.lon),
        station_id: Some(name.to_owned()),
        node: "df".to_owned(),
        sigma_deg: 2.0,
        accuracy_m: None,
        heading_deg: None,
        heading_sigma_deg: None,
        relative_deg: None,
        mirror_deg: None,
        freq_hz: None,
        source: BearingSource::Array,
        moving: false,
        others: Vec::new(),
        likelihood: Vec::new(),
        snr_db: None,
    };
    DecodedRecord {
        origin: Some(EventOrigin {
            node: "df".to_owned(),
            transmission: 0,
        }),
        device_set: 0,
        channel: NO_CHANNEL,
        at: jiff::Timestamp::now().to_string(),
        freq_hz: 0.0,
        event: DecoderEvent::of(&bearing),
        sinks: Vec::new(),
        callsign: None,
        view: None,
    }
}

#[tokio::test]
async fn bearings_wired_in_come_out_as_a_fix_on_the_wire() {
    let state = bench();
    let mut decoded = state.decoded.subscribe();
    let deadline = tokio::time::Instant::now() + GIVE_UP;
    let fix = loop {
        for (name, from) in [
            ("W", station(270.0, 40_000.0)),
            ("N", station(10.0, 50_000.0)),
        ] {
            state.engine.publish_decoded(bearing_record(name, from));
        }
        let next = tokio::time::Instant::now() + Duration::from_millis(200);
        let found = loop {
            match tokio::time::timeout_at(next.min(deadline), decoded.recv()).await {
                Ok(Ok(Decoded::Record(routed))) if routed.record.event.kind == BearingFix::KIND => {
                    let origin = routed.record.origin.map(|origin| origin.node);
                    assert_eq!(origin.as_deref(), Some("fix"));
                    break routed.record.event.read::<BearingFix>();
                }
                Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
                Ok(Err(broadcast::error::RecvError::Closed)) => panic!("feed closed"),
                Err(_) => break None,
            }
        };
        if let Some(fix) = found {
            break fix;
        }
        assert!(tokio::time::Instant::now() < deadline, "no fix");
    };
    assert!(miss_m(&fix) < 100.0, "{fix:?}");
    assert_eq!(fix.stations, 2);
}
