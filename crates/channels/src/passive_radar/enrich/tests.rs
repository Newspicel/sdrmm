use jiff::Timestamp;
use sdrmm_dsp::radar::bistatic::{self, Geodetic};
use sdrmm_wire::{
    Attitude, EventContract, PositionFix, ProcessorParams, RadarAoa, RadarAxes, RadarDetection,
    RadarHealth, ReferenceHealth, TrackChange, TrackTruth,
    present::{EventPayload, TrackFix},
    radar::LIGHT_SPEED_M_S,
};

use super::{
    truth::{AircraftTable, Baseline, MAX_AIRCRAFT, associate, solver},
    *,
};
use crate::{AdsbMessage, array_processor::ArrayPlace};

const HEADING_DEG: f64 = 30.0;
const RX: Geodetic = Geodetic {
    lat_deg: 52.0,
    lon_deg: 13.0,
    alt_m: 0.0,
};
const TX: Geodetic = Geodetic {
    lat_deg: 52.27,
    lon_deg: 13.0,
    alt_m: 0.0,
};
const CARRIER_HZ: f64 = 94_500_000.0;
const CRUISE_FT: i32 = 32_000;

fn axes() -> RadarAxes {
    RadarAxes {
        sample_rate_hz: 200_000.0,
        carrier_hz: CARRIER_HZ,
        range_step_m: 750.0,
        gates: 200,
        doppler_step_hz: 1.0,
        doppler_rows: 401,
        batches: 512,
        cpi_ms: 500.0,
        hop_ms: 500.0,
        lanes: 4,
    }
}

fn wavelength_m() -> f64 {
    LIGHT_SPEED_M_S / CARRIER_HZ
}

fn stamp(at: Timestamp) -> String {
    format!("{at:.3}")
}

fn epoch() -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("a valid instant")
}

fn later(at: Timestamp, seconds: f64) -> Timestamp {
    let millis = (seconds * 1_000.0).round() as i64;
    Timestamp::from_millisecond(at.as_millisecond() + millis).expect("a valid instant")
}

fn cruise_alt_m() -> f64 {
    f64::from(CRUISE_FT) * 0.3048
}

fn fix(message: &AdsbMessage) -> TrackFix {
    message
        .present(&AdsbMessage::KIND)
        .track
        .expect("ADS-B carries a track")
}

fn position(icao: &str, at: Geodetic) -> TrackFix {
    fix(&position_message(icao, at))
}

fn velocity(icao: &str, speed_kt: f64, track_deg: f64, climb_fpm: i32) -> TrackFix {
    fix(&velocity_message(icao, speed_kt, track_deg, climb_fpm))
}

fn position_message(icao: &str, at: Geodetic) -> AdsbMessage {
    AdsbMessage {
        icao: icao.to_owned(),
        df: 17,
        callsign: Some("DLH4AB ".to_owned()),
        altitude_ft: Some(CRUISE_FT),
        lat: Some(at.lat_deg),
        lon: Some(at.lon_deg),
        ..AdsbMessage::default()
    }
}

fn velocity_message(icao: &str, speed_kt: f64, track_deg: f64, climb_fpm: i32) -> AdsbMessage {
    AdsbMessage {
        icao: icao.to_owned(),
        df: 17,
        ground_speed_kt: Some(speed_kt),
        track_deg: Some(track_deg),
        vertical_rate_fpm: Some(climb_fpm),
        ..AdsbMessage::default()
    }
}

fn aircraft_at() -> Geodetic {
    Geodetic {
        lat_deg: 52.1,
        lon_deg: 13.3,
        alt_m: cruise_alt_m(),
    }
}

fn baseline() -> Baseline {
    Baseline { rx: RX, tx: TX }
}

fn range_m(point: Geodetic) -> f64 {
    bistatic::bistatic_range_m(
        bistatic::ecef(TX),
        bistatic::ecef(RX),
        bistatic::ecef(point),
    )
}

fn aoa(azimuth_deg: f32) -> Option<RadarAoa> {
    Some(RadarAoa {
        azimuth_deg,
        bearing_deg: None,
        sigma_deg: 2.0,
        quality: 0.9,
        mirror_deg: None,
    })
}

fn track(id: u32, range_km: f32, doppler_hz: f32, azimuth_deg: f32) -> RadarTrack {
    RadarTrack {
        id,
        state: TrackState::Confirmed,
        range_km,
        doppler_hz,
        range_sigma_m: 150.0,
        snr_db: 18.0,
        looks: 5,
        aoa: aoa(azimuth_deg),
        ..RadarTrack::default()
    }
}

fn event(track_id: u32, change: TrackChange) -> RadarTrackEvent {
    RadarTrackEvent {
        track_id,
        change,
        range_km: 24.0,
        range_rate_mps: -80.0,
        doppler_hz: 25.0,
        snr_db: 18.0,
        bearing_deg: None,
        lat: None,
        lon: None,
        target: None,
    }
}

fn cpi(seq: u64, tracks: Vec<RadarTrack>, events: Vec<RadarTrackEvent>) -> RadarUpdate {
    RadarUpdate {
        seq,
        at: stamp(Timestamp::now()),
        axes: axes(),
        tracks,
        events,
        ..RadarUpdate::default()
    }
}

#[test]
fn truth_maps_an_aircraft_to_delay_and_doppler() {
    let mut table = AircraftTable::default();
    let now = epoch();
    let at = aircraft_at();
    let (speed_kt, track_deg, climb_fpm) = (450.0, 60.0, 1_200);
    table.observe(now, &position("3c6444", at));
    table.observe(now, &velocity("3c6444", speed_kt, track_deg, climb_fpm));

    let mut truth = Vec::new();
    table.truth(now, baseline(), &axes(), &mut truth);

    let [seen] = truth.as_slice() else {
        panic!("one aircraft in view, got {truth:?}");
    };
    let speed = speed_kt * 0.514_444;
    let (sin, cos) = f64::to_radians(track_deg).sin_cos();
    let enu = [speed * sin, speed * cos, f64::from(climb_fpm) * 0.005_08];
    let moved = |seconds: f64| {
        range_m(bistatic::geodetic_from_enu(
            at,
            [enu[0] * seconds, enu[1] * seconds, enu[2] * seconds],
        ))
    };
    let rate_mps = (moved(0.5) - moved(-0.5)) / 1.0;
    let doppler_hz = -rate_mps / wavelength_m();

    assert!((f64::from(seen.range_km) * 1_000.0 - range_m(at)).abs() < 1.0);
    assert!(
        (f64::from(seen.doppler_hz) - doppler_hz).abs() < 0.05,
        "{} Hz against {doppler_hz} Hz",
        seen.doppler_hz
    );
    assert_eq!(seen.id, "3C6444");
    assert_eq!(seen.callsign.as_deref(), Some("DLH4AB"));
    assert!(seen.in_view);
    assert!((f64::from(seen.altitude_m) - cruise_alt_m()).abs() < 0.01);
    let bearing = bistatic::bearing_deg(RX, at);
    assert!((f64::from(seen.bearing_deg) - bearing).abs() < 0.01);

    table.truth(later(now, 4.0), baseline(), &axes(), &mut truth);
    assert!(
        (f64::from(truth[0].range_km) * 1_000.0 - moved(4.0)).abs() < 2.0,
        "a fresh position is carried forward by its velocity"
    );
    assert!((truth[0].age_s - 4.0).abs() < 1e-3);

    table.truth(later(now, 31.0), baseline(), &axes(), &mut truth);
    assert!(truth.is_empty(), "a position older than 30 s is no truth");
}

#[test]
fn truth_without_velocity_uses_the_range_history() {
    let mut table = AircraftTable::default();
    let start = aircraft_at();
    let step = |second: u32| {
        let point = bistatic::geodetic_from_enu(start, [220.0 * f64::from(second), 0.0, 0.0]);
        Geodetic {
            alt_m: cruise_alt_m(),
            ..point
        }
    };
    let mut truth = Vec::new();
    table.observe(epoch(), &position("4ca123", step(0)));
    table.truth(epoch(), baseline(), &axes(), &mut truth);
    assert!(truth.is_empty(), "one position gives no Doppler");

    for second in 1..=5 {
        table.observe(
            later(epoch(), f64::from(second)),
            &position("4ca123", step(second)),
        );
    }
    let now = later(epoch(), 5.0);
    table.truth(now, baseline(), &axes(), &mut truth);

    let [seen] = truth.as_slice() else {
        panic!("the aircraft has a history, got {truth:?}");
    };
    let middle_rate = range_m(step(3)) - range_m(step(2));
    assert!(
        (f64::from(seen.doppler_hz) + middle_rate / wavelength_m()).abs() < 1e-3,
        "the median of five range steps"
    );
    assert!((f64::from(seen.range_km) * 1_000.0 - range_m(step(5))).abs() < 1.0);
}

#[test]
fn the_aircraft_table_forgets_silence_and_stays_bounded() {
    let mut table = AircraftTable::default();
    for index in 0..=MAX_AIRCRAFT {
        let at = later(epoch(), index as f64 / 100.0);
        table.observe(at, &position(&format!("{index:06x}"), aircraft_at()));
    }
    assert_eq!(table.len(), MAX_AIRCRAFT);
    assert!(!table.knows("000000"), "the oldest entry made room");
    assert!(table.knows("000001") && table.knows(&format!("{MAX_AIRCRAFT:06x}")));

    table.prune(later(epoch(), 75.0));
    assert_eq!(table.len(), 0);
}

#[test]
fn truth_pairs_with_the_nearest_track() {
    let sighting = |icao: &str, range_km: f32, doppler_hz: f32, in_view: bool| TrackTruth {
        id: icao.to_owned(),
        range_km,
        doppler_hz,
        in_view,
        ..TrackTruth::default()
    };
    let mut truth = vec![
        sighting("near", 20.0, 50.0, true),
        sighting("far", 60.0, -100.0, true),
        sighting("hidden", 20.1, 50.2, false),
    ];
    let mut tracks = vec![track(7, 20.8, 46.0, 0.0), track(9, 20.3, 52.0, 0.0)];
    let axes = RadarAxes {
        range_step_m: 1_000.0,
        ..axes()
    };

    associate(&mut solver(), &mut truth, &mut tracks, &axes);

    assert_eq!(truth[0].track_id, Some(9));
    assert_eq!(
        tracks[1].truth.as_ref().map(|truth| truth.id.as_str()),
        Some("near")
    );
    assert_eq!(tracks[0].truth, None, "the farther track stays unpaired");
    assert_eq!(truth[1].track_id, None, "outside the gate");
    assert_eq!(truth[2].track_id, None, "out of view");
}

#[test]
fn problems_follow_the_pod_health() {
    let mut node = RadarEnricher::default();
    let sites = Sites::default();
    let mut params = PassiveRadarParams::default();
    let loaded = |load: f32| RadarUpdate {
        health: RadarHealth {
            load,
            aoa: AoaState::PhaseUnknown,
            reference: ReferenceHealth {
                mode: ReferenceMode::Cma,
                locked: false,
                ..ReferenceHealth::default()
            },
            ..RadarHealth::default()
        },
        ..cpi(1, Vec::new(), Vec::new())
    };
    let mut problems = Vec::new();
    for _ in 0..OVERLOAD_CPIS {
        let mut update = loaded(1.3);
        node.fill(&mut update, &sites, &params);
        problems = update.problems;
    }
    assert_eq!(
        problems,
        [
            RadarProblem::NoTransmitter,
            RadarProblem::NoReceiver,
            RadarProblem::PhaseUnknown,
            RadarProblem::Overloaded,
            RadarProblem::ReferenceLost,
        ]
    );
    let mut calm = loaded(0.4);
    node.fill(&mut calm, &sites, &params);
    assert!(!calm.problems.contains(&RadarProblem::Overloaded));

    params.aoa = false;
    let mut blind = loaded(0.4);
    node.fill(&mut blind, &sites, &params);
    assert!(!blind.problems.contains(&RadarProblem::PhaseUnknown));
}

#[test]
fn a_carrier_outside_the_array_table_is_a_problem() {
    let mut node = RadarEnricher::default();
    let sites = Sites::default();
    let mut params = PassiveRadarParams::default();
    let steered = |table_out_of_range| RadarUpdate {
        health: RadarHealth {
            aoa: AoaState::Ready,
            table_out_of_range,
            ..RadarHealth::default()
        },
        ..cpi(1, Vec::new(), Vec::new())
    };
    let mut outside = steered(true);
    node.fill(&mut outside, &sites, &params);
    assert_eq!(
        outside.problems,
        [
            RadarProblem::NoTransmitter,
            RadarProblem::NoReceiver,
            RadarProblem::TableOutOfRange,
        ]
    );
    let mut inside = steered(false);
    node.fill(&mut inside, &sites, &params);
    assert!(!inside.problems.contains(&RadarProblem::TableOutOfRange));
    params.aoa = false;
    let mut blind = steered(true);
    node.fill(&mut blind, &sites, &params);
    assert!(!blind.problems.contains(&RadarProblem::TableOutOfRange));
}

fn placed(heading_deg: Option<f64>) -> ArrayPlace {
    ArrayPlace {
        lat: RX.lat_deg,
        lon: RX.lon_deg,
        altitude_m: None,
        heading_deg,
    }
}

fn transmitter() -> Vec<(String, PositionFix)> {
    vec![(
        RADAR_TX_PORT.to_owned(),
        PositionFix {
            latitude: TX.lat_deg,
            longitude: TX.lon_deg,
            altitude_m: None,
            accuracy_m: None,
            speed_mps: None,
            track_deg: None,
            time: String::new(),
            attitude: Attitude::default(),
        },
    )]
}

fn completed(
    node: &mut RadarEnricher,
    array: Option<ArrayPlace>,
    positions: &[(String, PositionFix)],
    update: RadarUpdate,
) -> (RadarUpdate, Vec<Raised>) {
    let params = ProcessorParams::PassiveRadar(PassiveRadarParams::default());
    let ctx = EnrichCtx {
        params: &params,
        array,
        positions,
    };
    let mut reading = ProcessorReading::PassiveRadar(update);
    let raised = node.complete(&ctx, &mut reading);
    let ProcessorReading::PassiveRadar(done) = reading else {
        panic!("still a radar reading");
    };
    (done, raised)
}

fn track_events(raised: &[Raised]) -> Vec<RadarTrackEvent> {
    raised
        .iter()
        .filter_map(|raised| raised.event.read::<RadarTrackEvent>())
        .collect()
}

#[test]
fn true_bearing_adds_the_array_heading_and_fixes_the_track() {
    let mut node = RadarEnricher::default();
    let mut update = cpi(1, vec![track(3, 24.0, 20.0, 350.0)], Vec::new());
    update.detections.push(RadarDetection {
        range_km: 24.0,
        doppler_hz: 20.0,
        aoa: aoa(350.0),
        ..RadarDetection::default()
    });

    let (done, raised) = completed(
        &mut node,
        Some(placed(Some(HEADING_DEG))),
        &transmitter(),
        update,
    );

    let bearing = |aoa: Option<RadarAoa>| aoa.and_then(|aoa| aoa.bearing_deg);
    assert_eq!(bearing(done.detections[0].aoa), Some(20.0));
    assert_eq!(bearing(done.tracks[0].aoa), Some(20.0));
    assert!(done.problems.is_empty(), "{:?}", done.problems);
    let geometry = done.geometry.expect("both sites known");
    assert_eq!(geometry.heading_deg, Some(30.0));
    assert!((geometry.baseline_km - 30.0).abs() < 0.5);
    let fix = done.tracks[0].fix.expect("a fix");
    let assumed = PassiveRadarParams::default().assumed_altitude_m;
    assert!(!fix.alt_from_truth);
    let at = Geodetic {
        lat_deg: fix.lat,
        lon_deg: fix.lon,
        alt_m: f64::from(assumed),
    };
    assert!((bistatic::bearing_deg(RX, at) - 20.0).abs() < 0.01);
    assert!((range_m(at) - 24_000.0).abs() < 1.0);
    assert!(
        raised
            .first()
            .is_some_and(|first| first.event.read::<RadarSites>().is_some()),
        "fresh sites are told first"
    );
}

#[test]
fn no_transmitter_means_no_truth_and_no_fixes() {
    let mut node = RadarEnricher::default();
    node.observe(Timestamp::now(), &position("3c6444", aircraft_at()));
    let (done, _) = completed(
        &mut node,
        Some(placed(Some(HEADING_DEG))),
        &[],
        cpi(1, vec![track(3, 24.0, 20.0, 10.0)], Vec::new()),
    );
    assert_eq!(done.problems, [RadarProblem::NoTransmitter]);
    assert_eq!(done.geometry, None);
    assert!(done.truth.is_empty());
    assert_eq!(done.tracks[0].fix, None);
    assert_eq!(
        done.tracks[0].aoa.and_then(|aoa| aoa.bearing_deg),
        Some(40.0)
    );
}

#[test]
fn a_heading_less_array_keeps_relative_azimuths() {
    let mut node = RadarEnricher::default();
    let (done, _) = completed(
        &mut node,
        Some(placed(None)),
        &transmitter(),
        cpi(1, vec![track(3, 24.0, 20.0, 10.0)], Vec::new()),
    );
    assert_eq!(done.problems, [RadarProblem::NoHeading]);
    assert_eq!(done.tracks[0].aoa.and_then(|aoa| aoa.bearing_deg), None);
    assert_eq!(done.tracks[0].fix, None);
}

#[test]
fn adsb_truth_pairs_and_lifts_the_fix_to_its_altitude() {
    let mut node = RadarEnricher::default();
    let now = Timestamp::now();
    node.observe(now, &position("3c6444", aircraft_at()));
    node.observe(now, &velocity("3c6444", 400.0, 270.0, 0));
    let array = Some(placed(Some(HEADING_DEG)));
    let (first, _) = completed(
        &mut node,
        array,
        &transmitter(),
        cpi(1, Vec::new(), Vec::new()),
    );
    let seen = first.truth.first().cloned().expect("the aircraft is seen");
    assert!(seen.in_view);

    let azimuth = (f64::from(seen.bearing_deg) - HEADING_DEG).rem_euclid(360.0) as f32;
    let (done, _) = completed(
        &mut node,
        array,
        &transmitter(),
        cpi(
            2,
            vec![track(5, seen.range_km, seen.doppler_hz, azimuth)],
            Vec::new(),
        ),
    );
    assert_eq!(
        done.tracks[0].truth.as_ref().map(|truth| truth.id.as_str()),
        Some("3C6444")
    );
    assert_eq!(done.truth[0].track_id, Some(5));
    let fix = done.tracks[0].fix.expect("a fix");
    assert!(fix.alt_from_truth);
    assert!((fix.alt_m - done.truth[0].altitude_m).abs() < 1.0);
}

#[test]
fn track_events_carry_the_last_fix_and_lost_tracks_keep_it() {
    let mut node = RadarEnricher::default();
    let array = Some(placed(Some(HEADING_DEG)));
    let mut raise = |seq, tracks, events| {
        let (_, raised) = completed(&mut node, array, &transmitter(), cpi(seq, tracks, events));
        track_events(&raised)
    };
    let confirmed = raise(
        1,
        vec![track(42, 24.0, 20.0, 10.0)],
        vec![event(42, TrackChange::Confirmed)],
    );
    let updated = raise(
        2,
        vec![track(42, 24.2, 20.0, 10.0)],
        vec![event(42, TrackChange::Update)],
    );
    let lost = raise(3, Vec::new(), vec![event(42, TrackChange::Lost)]);
    for event in [&confirmed[0], &updated[0], &lost[0]] {
        assert_eq!(event.bearing_deg, Some(40.0));
        assert!(event.lat.is_some() && event.lon.is_some());
    }
    assert_eq!(
        lost[0].lat, updated[0].lat,
        "a lost track keeps its last fix"
    );
}

#[test]
fn clearing_tracks_forgets_their_fixes() {
    let mut node = RadarEnricher::default();
    let array = Some(placed(Some(HEADING_DEG)));
    completed(
        &mut node,
        array,
        &transmitter(),
        cpi(1, vec![track(7, 24.0, 20.0, 10.0)], Vec::new()),
    );
    let mut latest =
        ProcessorReading::PassiveRadar(cpi(1, vec![track(7, 24.0, 20.0, 10.0)], Vec::new()));
    node.act(ProcessorAction::ClearTracks, Some(&mut latest));
    let ProcessorReading::PassiveRadar(cleared) = latest else {
        panic!("a radar reading");
    };
    assert!(cleared.tracks.is_empty());
    let (_, raised) = completed(
        &mut node,
        array,
        &transmitter(),
        cpi(2, Vec::new(), vec![event(7, TrackChange::Lost)]),
    );
    assert_eq!(track_events(&raised)[0].lat, None);
}

#[test]
fn a_refusal_names_its_problem_and_lost_reports_add_up() {
    let mut node = RadarEnricher::default();
    let (reading, _) = node.refused(&Refusal::Refused("No lanes".into()));
    let ProcessorReading::PassiveRadar(update) = reading else {
        panic!("a radar reading");
    };
    assert_eq!(update.problems, [RadarProblem::Refused("No lanes".into())]);
    node.lagged(5);
    node.lagged(2);
    let (done, _) = completed(&mut node, None, &[], cpi(1, Vec::new(), Vec::new()));
    assert_eq!(done.health.lagged_updates, 7);
}
