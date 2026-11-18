use super::*;

#[test]
fn radar_problems_name_their_labels() {
    let problems = RadarProblem::ALL;
    let labels: Vec<&str> = problems.iter().map(RadarProblem::label).collect();
    assert_eq!(
        labels,
        [
            "No array",
            "No transmitter",
            "No array position",
            "No heading",
            "Phase unknown",
            "Outside cal table",
            "Overloaded",
            "Reference lost",
            "",
        ]
    );
    let refused = RadarProblem::Refused("CPI too large".to_owned());
    assert_eq!(refused.label(), "CPI too large");
    let json = serde_json::to_value(&refused).expect("serialize");
    assert_eq!(json["kind"], "refused");
    assert_eq!(json["detail"], "CPI too large");
    for problem in RadarProblem::ALL {
        let json = serde_json::to_value(&problem).expect("serialize");
        assert_eq!(json["kind"], problem.kind());
    }
}

fn full_update() -> RadarUpdate {
    let aoa = RadarAoa {
        azimuth_deg: 12.0,
        bearing_deg: Some(99.0),
        sigma_deg: 3.0,
        quality: 0.8,
        mirror_deg: Some(168.0),
    };
    RadarUpdate {
        seq: 42,
        at: "2026-09-28T12:00:00Z".to_owned(),
        axes: RadarAxes {
            sample_rate_hz: 266_666.0,
            carrier_hz: 98.5e6,
            range_step_m: 1_124.0,
            gates: 72,
            doppler_step_hz: 2.0,
            doppler_rows: 255,
            batches: 256,
            cpi_ms: 500.0,
            hop_ms: 500.0,
            lanes: 5,
        },
        detections: vec![RadarDetection {
            range_km: 32.5,
            doppler_hz: 120.0,
            range_rate_mps: -365.0,
            snr_db: 17.0,
            cells: 6,
            aoa: Some(aoa),
            track_id: Some(7),
        }],
        tracks: vec![RadarTrack {
            id: 7,
            state: TrackState::Coasting,
            range_km: 32.5,
            range_rate_mps: -365.0,
            doppler_hz: 120.0,
            accel_mps2: 1.0,
            range_sigma_m: 300.0,
            rate_sigma_mps: 4.0,
            snr_db: 17.0,
            looks: 9,
            misses: 1,
            aoa: Some(aoa),
            fix: Some(RadarFix {
                lat: 52.4,
                lon: 13.2,
                alt_m: 9_000.0,
                alt_from_truth: true,
                major_m: 2_000.0,
                minor_m: 500.0,
                orientation_deg: 40.0,
            }),
            truth: Some(TrackMatch {
                id: "3C6444".to_owned(),
                callsign: Some("DLH4AB".to_owned()),
            }),
            trail: vec![RadarTrailPoint {
                range_km: 33.0,
                doppler_hz: 118.0,
            }],
        }],
        truth: vec![TrackTruth {
            id: "3C6444".to_owned(),
            callsign: Some("DLH4AB".to_owned()),
            range_km: 32.4,
            doppler_hz: 121.0,
            bearing_deg: 98.0,
            lat: 52.4,
            lon: 13.2,
            altitude_m: 9_100.0,
            age_s: 0.5,
            in_view: true,
            track_id: Some(7),
        }],
        health: RadarHealth {
            suppression_db: vec![42.0; 4],
            unsuppressed_groups: 1,
            dropped_samples: 2,
            dropped_cpis: 3,
            discarded_cpis: 4,
            dropped_reports: 5,
            lagged_updates: 6,
            truncated_detections: 7,
            dropped_tracks: 8,
            gpu_failures: 9,
            load: 0.4,
            front_load: 0.2,
            compute_ms: 120.0,
            noise_floor_db: -80.0,
            cfar_looks: 1,
            range_correlation: 1.3,
            gpu: true,
            threads: 4,
            aoa: AoaState::Ready,
            table_out_of_range: true,
            reference: ReferenceHealth {
                mode: ReferenceMode::Cma,
                locked: true,
                quality_db: 20.0,
                fallback_frames: 1,
            },
        },
        geometry: Some(RadarGeometry {
            receiver: RadarSite {
                lat: 52.5,
                lon: 13.4,
                altitude_m: 40.0,
            },
            transmitter: RadarSite {
                lat: 52.52,
                lon: 13.41,
                altitude_m: 368.0,
            },
            baseline_km: 2.3,
            heading_deg: Some(87.0),
        }),
        problems: vec![RadarProblem::NoHeading],
        events: Vec::new(),
    }
}

#[test]
fn radar_update_round_trips_json() {
    let update = full_update();
    let json = serde_json::to_value(&update).expect("serialize");
    assert_eq!(json["problems"][0]["kind"], "no_heading");
    assert_eq!(json["tracks"][0]["state"], "coasting");
    assert!(json.get("events").is_none());
    assert_eq!(json["health"]["table_out_of_range"], true);
    let back: RadarUpdate = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, update);
}

#[test]
fn a_health_written_before_the_table_flag_reads_as_covered() {
    let mut json = serde_json::to_value(full_update().health).expect("serialize");
    json.as_object_mut()
        .map(|health| health.remove("table_out_of_range"));
    let health: RadarHealth = serde_json::from_value(json).expect("deserialize");
    assert!(!health.table_out_of_range);
    let problem = serde_json::to_value(RadarProblem::TableOutOfRange).expect("serialize");
    assert_eq!(problem, serde_json::json!({"kind": "table_out_of_range"}));
}

#[test]
fn track_events_never_reach_the_wire() {
    let mut update = full_update();
    update.events.push(RadarTrackEvent {
        track_id: 7,
        change: TrackChange::Confirmed,
        range_km: 32.5,
        range_rate_mps: -365.0,
        doppler_hz: 120.0,
        snr_db: 17.0,
        bearing_deg: None,
        lat: None,
        lon: None,
        target: None,
    });
    let json = serde_json::to_string(&update).expect("serialize");
    let back: RadarUpdate = serde_json::from_str(&json).expect("deserialize");
    assert!(back.events.is_empty());
}

#[test]
fn a_reserved_update_holds_every_list_at_its_limit() {
    let update = RadarUpdate::reserved();
    assert!(update.detections.capacity() >= MAX_RADAR_DETECTIONS);
    assert!(update.tracks.capacity() >= MAX_RADAR_TRACKS);
    assert!(update.truth.capacity() >= MAX_RADAR_TRUTH);
    assert!(update.events.capacity() >= MAX_RADAR_TRACKS);
    assert!(update.health.suppression_db.capacity() >= MAX_ARRAY_LANES as usize);
    assert!(update.detections.is_empty() && update.tracks.is_empty());
}

#[test]
fn a_track_lists_range_motion_and_match() {
    let track = RadarTrackEvent {
        track_id: 7,
        change: TrackChange::Update,
        range_km: 12.345,
        range_rate_mps: -150.0,
        doppler_hz: 42.5,
        snr_db: 17.25,
        bearing_deg: None,
        lat: Some(50.0),
        lon: Some(8.5),
        target: Some("3C6444".to_owned()),
    };
    let view = track.present(&EventKind::RADAR);
    assert_eq!(
        view.labels(),
        [
            "Track",
            "Change",
            "Range",
            "Range rate",
            "Doppler",
            "SNR",
            "Position",
            "Target"
        ]
    );
    assert_eq!(view.field("Track"), text(Some("T7")).as_ref());
    assert_eq!(
        view.field("Range"),
        number(Some(12.345_f32), Unit::Kilometres, 2).as_ref()
    );
    assert_eq!(
        view.field("Position"),
        position(Some(50.0), Some(8.5)).as_ref()
    );
}

fn sites(ranges_km: Vec<f32>) -> RadarSites {
    RadarSites {
        geometry: Some(RadarGeometry {
            receiver: RadarSite {
                lat: 52.5,
                lon: 13.4,
                altitude_m: 40.0,
            },
            transmitter: RadarSite {
                lat: 52.52,
                lon: 13.41,
                altitude_m: 360.0,
            },
            baseline_km: 2.3,
            heading_deg: None,
        }),
        ranges_km,
    }
}

#[test]
fn radar_sites_pin_both_ends_and_ring_each_echo() {
    let view = sites(vec![12.5, 0.0]).present(&EventKind::RADAR_SITES);

    let pins: Vec<&str> = view
        .shapes
        .iter()
        .filter_map(|shape| match shape {
            Shape::Pin { label, .. } => Some(label.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(pins, ["Receiver", "Illuminator"]);
    let rings: Vec<(f32, f32)> = view
        .shapes
        .iter()
        .filter_map(|shape| match shape {
            Shape::Ellipse {
                major_m, minor_m, ..
            } => Some((*major_m, *minor_m)),
            _ => None,
        })
        .collect();
    assert_eq!(rings.len(), 1, "a zero range draws no ring");
    let (major, minor) = rings[0];
    assert!(major > minor && major > 6_250.0, "{major} {minor}");
    assert!(!view.gone);
}

#[test]
fn radar_sites_without_geometry_leave_the_map() {
    let view = RadarSites::default().present(&EventKind::RADAR_SITES);
    assert!(view.gone);
    assert_eq!(view.entity.as_deref(), Some(RADAR_SITES_ENTITY));
}
