use super::*;

const NORTH: SkyPoint = SkyPoint {
    azimuth_deg: 0.0,
    elevation_deg: 0.0,
};

fn rotator(position: SkyPoint, target: SkyPoint) -> RotatorStatus {
    RotatorStatus {
        node: "rot".to_owned(),
        position: Some(position),
        target: Some(target),
        ..RotatorStatus::default()
    }
}

fn bearing(azimuth_deg: f64) -> SkyPoint {
    SkyPoint {
        azimuth_deg,
        elevation_deg: 0.0,
    }
}

fn wiring() -> Wiring {
    Wiring {
        settings: SkyScanParams::default(),
        radio: Some(Radio {
            device_set: 0,
            stream: 0,
        }),
        rotator: Some(Rotator {
            node: "rot".to_owned(),
            tolerance_deg: 2.0,
            tracks: true,
        }),
    }
}

#[test]
fn it_arrives_once_the_rotator_took_the_point_and_stands_near_it() {
    assert!(arrived(&rotator(bearing(359.0), NORTH), NORTH, 2.0));
    assert!(!arrived(&rotator(bearing(350.0), NORTH), NORTH, 2.0));
    assert!(!arrived(&rotator(bearing(90.0), bearing(90.0)), NORTH, 2.0));
    assert!(!arrived(&RotatorStatus::default(), NORTH, 2.0));
}

#[test]
fn a_turntable_arrives_though_it_cannot_lift() {
    let up = SkyPoint {
        azimuth_deg: 45.0,
        elevation_deg: 30.0,
    };
    assert!(arrived(&rotator(bearing(45.0), bearing(45.0)), up, 5.0));
}

#[test]
fn bearings_compare_across_north() {
    assert!((azimuth_apart(359.0, 1.0) - 2.0).abs() < 1e-9);
    assert!(azimuth_apart(10.0, 370.0).abs() < 1e-9);
}

#[test]
fn a_scan_refuses_to_start_without_its_wires() {
    let mut session = Session {
        wiring: Wiring {
            radio: None,
            ..wiring()
        },
        ..Session::default()
    };
    assert!(matches!(session.plan(), Err(SkyScanRefusal::NoRadio)));
    session.wiring = Wiring {
        rotator: None,
        ..wiring()
    };
    assert!(matches!(session.plan(), Err(SkyScanRefusal::NoRotator)));
    session.wiring.rotator = wiring().rotator.map(|rotator| Rotator {
        tracks: false,
        ..rotator
    });
    assert!(matches!(session.plan(), Err(SkyScanRefusal::NotTracking)));
    session.wiring = wiring();
    assert_eq!(session.plan().map(|plan| plan.points.len()).ok(), Some(36));
}

#[test]
fn a_rewire_stops_a_running_scan_and_new_settings_clear_its_cells() {
    let hub = SkyScanHub::default();
    hub.apply(HashMap::from([("sky".to_owned(), wiring())]));
    let generation = {
        let mut sessions = hub.lock();
        let session = sessions.get_mut("sky").expect("session");
        session.status.running = true;
        session.generation
    };
    let cell = SkyCell {
        azimuth_deg: 0.0,
        elevation_deg: 0.0,
        level_dbfs: -50.0,
    };
    assert!(hub.record("sky", generation, cell, 100e6).is_some());
    let rewired = Wiring {
        radio: Some(Radio {
            device_set: 1,
            stream: 0,
        }),
        ..wiring()
    };
    let changed = hub.apply(HashMap::from([("sky".to_owned(), rewired.clone())]));
    let status = changed.first().expect("changed");
    assert!(!status.running);
    assert_eq!(status.error.as_deref(), Some(REWIRED));
    assert_eq!(status.cells.len(), 1);
    assert!(hub.record("sky", generation, cell, 100e6).is_none());
    assert!(hub.finish("sky", generation, None).is_none());
    let mut wider = rewired;
    wider.settings.step_deg = 20.0;
    let changed = hub.apply(HashMap::from([("sky".to_owned(), wider)]));
    let status = changed.first().expect("changed");
    assert!(status.cells.is_empty());
    assert_eq!((status.columns, status.rows), (18, 1));
}

#[test]
fn an_unchanged_patch_changes_nothing() {
    let hub = SkyScanHub::default();
    assert_eq!(
        hub.apply(HashMap::from([("sky".to_owned(), wiring())]))
            .len(),
        1
    );
    assert!(
        hub.apply(HashMap::from([("sky".to_owned(), wiring())]))
            .is_empty()
    );
    assert!(hub.apply(HashMap::new()).is_empty());
    assert!(hub.status("sky").is_none());
}
