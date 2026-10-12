use sdrmm_wire::{DecoderEvent, EventOrigin, WsjtMessage};

use super::*;

const NOW: i64 = 1_786_881_600_000;

const BERLIN: LatLon = LatLon {
    lat: 52.5,
    lon: 13.0,
};

fn at(lat: f64, lon: f64) -> LatLon {
    LatLon { lat, lon }
}

fn heard(key: &str) -> Heard {
    Heard {
        key: key.to_owned(),
        at_ms: NOW,
        at: "2026-08-16T12:00:00Z".to_owned(),
        callsign: "W1AW".to_owned(),
        grid: "FN42".to_owned(),
        from: at(42.5, -71.0),
        freq_hz: 14_074_000.0,
        snr_db: -12.0,
    }
}

fn observation(key: &str, control: LatLon) -> Observation {
    Observation {
        heard: heard(key),
        distance_km: 6_000.0,
        hops: 2,
        muf3000_mhz: Some(16.0),
        control: vec![control],
    }
}

fn ft8(text: &str) -> DecodedRecord {
    DecodedRecord {
        origin: Some(EventOrigin {
            node: "ft8".to_owned(),
            transmission: 0,
        }),
        device_set: 0,
        channel: 0,
        at: "2026-08-16T12:00:00Z".to_owned(),
        freq_hz: 14_074_000.0,
        event: DecoderEvent::Ft8(WsjtMessage {
            text: text.to_owned(),
            snr_db: -12.0,
            audio_hz: 1_500.0,
            time_offset_s: 0.0,
            hard_errors: 0,
        }),
        sinks: Vec::new(),
        view: None,
        callsign: None,
    }
}

fn heard_from(record: &DecodedRecord) -> Option<Heard> {
    let view = record.event.present();
    heard_of(record, view.spot.as_ref()?)
}

#[test]
fn great_circles_measure_the_equator_and_berlin_to_tokyo() {
    let quarter = std::f64::consts::FRAC_PI_2 * EARTH_RADIUS_KM;
    assert!((great_circle_km(at(0.0, 0.0), at(0.0, 90.0)) - quarter).abs() < 1e-3);
    let tokyo = great_circle_km(BERLIN, at(35.68, 139.77));
    assert!((8_900.0..9_000.0).contains(&tokyo), "{tokyo}");
}

#[test]
fn a_path_is_halved_at_its_midpoint() {
    let mid = along_great_circle(at(0.0, 0.0), at(0.0, 80.0), 0.5);
    assert!((mid.lon - 40.0).abs() < 1e-6);
    assert_eq!(
        along_great_circle(at(0.0, 0.0), at(0.0, 0.0), 0.5),
        at(0.0, 0.0)
    );
}

#[test]
fn the_hop_geometry_reproduces_the_textbook_m3000() {
    assert!((obliquity_factor(3_000.0, 300.0) - 3.2798).abs() < 1e-3);
    assert!((obliquity_factor(1.0, 300.0) - 1.0).abs() < 1e-4);
    assert!((3_800.0..3_900.0).contains(&max_hop_km(300.0)));
    assert_eq!(hop_count(2_000.0, 300.0), 1);
    assert_eq!(hop_count(8_000.0, 300.0), 3);
    assert_eq!(hop_count(0.0, 300.0), 1);
}

#[test]
fn the_measured_muf_follows_the_hop_length() {
    let same = muf3000_mhz(14_074_000.0, 3_000.0, 300.0).expect("a 3000 km path");
    assert!((same - 14.074).abs() < 1e-6);
    let short = muf3000_mhz(14_074_000.0, 1_000.0, 300.0).expect("short");
    let long = muf3000_mhz(14_074_000.0, 8_000.0, 300.0).expect("long");
    assert!((24.0..26.0).contains(&short), "{short}");
    assert!(long > 14.074 && long < 15.5, "{long}");
    assert_eq!(
        muf3000_mhz(14_074_000.0, MIN_MUF_PATH_KM - 1.0, 300.0),
        None
    );
    assert_eq!(muf3000_mhz(0.0, 3_000.0, 300.0), None);
    assert_eq!(muf3000_mhz(f64::NAN, 3_000.0, 300.0), None);
}

#[test]
fn control_points_sit_at_each_hop_midpoint() {
    let single = control_points(at(0.0, 0.0), at(0.0, 60.0), 1);
    assert_eq!(single.len(), 1);
    assert!((single[0].lon - 30.0).abs() < 1e-6);
    let triple = control_points(at(0.0, 0.0), at(0.0, 60.0), 3);
    assert!((triple[0].lon - 10.0).abs() < 1e-6);
    assert!((triple[2].lon - 50.0).abs() < 1e-6);
}

#[test]
fn a_decode_with_a_square_becomes_a_path_from_the_receiver() {
    let spot = heard_from(&ft8("CQ W1AW FN42")).expect("a CQ carries its square");
    let built = observe(&spot, BERLIN, 300.0);

    assert_eq!(built.heard.grid, "FN42");
    assert_eq!(built.heard.callsign, "W1AW");
    assert!((5_800.0..6_100.0).contains(&built.distance_km));
    assert_eq!(built.hops, 2);
    assert_eq!(built.control.len(), 2);
    assert!(built.muf3000_mhz.is_some());
    assert_eq!(heard_from(&ft8("W9XYZ W1AW RR73")), None);
}

#[test]
fn a_weight_halves_over_one_half_life() {
    assert!((decay_weight(0.0, 30) - 1.0).abs() < 1e-12);
    assert!((decay_weight(30.0 * 60_000.0, 30) - 0.5).abs() < 1e-9);
    assert!((decay_weight(60.0 * 60_000.0, 30) - 0.25).abs() < 1e-9);
}

#[test]
fn reflection_points_gather_into_their_square() {
    let mut second = observation("b", at(50.4, -19.5));
    second.heard.freq_hz = 21_074_000.0;
    second.heard.callsign = "K1ABC".to_owned();
    second.muf3000_mhz = Some(24.0);
    let mut far = observation("c", at(10.0, 10.0));
    far.muf3000_mhz = Some(9.0);

    let found = cells(&[observation("a", at(50.0, -20.0)), second, far], 30, NOW);

    assert_eq!(found.len(), 2);
    let busiest = &found[0];
    assert_eq!(busiest.decodes, 2);
    assert_eq!(busiest.callsigns, 2);
    assert!((busiest.best_freq_hz - 21_074_000.0).abs() < 1e-6);
    assert_eq!(busiest.measured_muf3000_mhz, Some(24.0));
    assert!((busiest.weight - 2.0).abs() < 1e-6);
}

#[test]
fn an_old_decode_weighs_less_and_a_cell_without_muf_stays_unmeasured() {
    let mut old = observation("old", at(50.0, -20.0));
    old.heard.at_ms = NOW - 60 * 60_000;
    old.muf3000_mhz = None;
    let found = cells(&[old, observation("new", at(10.0, 10.0))], 30, NOW);

    assert!((found[0].weight - 1.0).abs() < 1e-6);
    assert!((found[1].weight - 0.25).abs() < 1e-6);
    assert_eq!(found[1].measured_muf3000_mhz, None);
}

#[test]
fn paths_keep_the_newest_per_square_and_band() {
    let mut older = observation("old", at(0.0, 0.0));
    older.heard.at_ms = NOW - 1_000;
    let mut other = observation("other", at(0.0, 0.0));
    other.heard.grid = "JO62".to_owned();
    other.heard.at_ms = NOW - 500;

    let drawn = paths(
        &[older, observation("new", at(0.0, 0.0)), other],
        BERLIN,
        30,
        NOW,
    );

    assert_eq!(drawn.len(), 2);
    assert_eq!(drawn[0].key, "new");
    assert_eq!(drawn[0].from, BERLIN);
}

#[test]
fn the_summary_counts_what_was_heard() {
    let mut near = observation("b", at(0.0, 0.0));
    near.heard.grid = "JO62".to_owned();
    near.heard.callsign = "DL1ABC".to_owned();
    near.heard.freq_hz = 28_074_000.0;
    near.muf3000_mhz = Some(31.0);
    near.distance_km = 400.0;

    let summary = summary(&[observation("a", at(0.0, 0.0)), near]);

    assert_eq!(
        (
            summary.decodes,
            summary.grids,
            summary.callsigns,
            summary.bands
        ),
        (2, 2, 2, 2)
    );
    assert!((summary.best_freq_hz - 28_074_000.0).abs() < 1e-6);
    assert_eq!(summary.best_muf3000_mhz, Some(31.0));
    assert!((summary.farthest_km - 6_000.0).abs() < 1e-6);
}

#[test]
fn the_model_merges_once_stays_cleared_and_forgets_what_decayed() {
    let mut model = Propagation {
        receiver: Some(BERLIN),
        ..Propagation::default()
    };
    model.hear(heard("a"));
    model.hear(heard("a"));
    let mut ancient = heard("ancient");
    ancient.at_ms = NOW - 24 * 3_600_000;
    model.hear(ancient);

    assert_eq!(
        model.view(NOW).summary.decodes,
        1,
        "duplicates and decayed spots"
    );

    model.clear(NOW);
    model.hear(heard("before the clear"));
    let mut after = heard("after");
    after.at_ms = NOW + 1_000;
    model.hear(after);
    assert_eq!(model.view(NOW + 1_000).summary.decodes, 1);
}

#[test]
fn a_model_without_a_receiver_draws_nothing() {
    let mut model = Propagation::default();
    model.hear(heard("a"));

    let view = model.view(NOW);

    assert_eq!(view.receiver, None);
    assert!(view.cells.is_empty());
}
