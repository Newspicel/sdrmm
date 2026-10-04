use sdrmm_wire::{
    NavTargetKind, TriangulationParams,
    geo::{self, Enu},
};

use super::*;

fn sigmas_away(estimate: &DfEstimate, truth: LatLon) -> f64 {
    let (east, north) = Enu::new(place(estimate)).to_enu(truth);
    let axis = estimate.ellipse_bearing_deg.to_radians();
    let along = east * axis.sin() + north * axis.cos();
    let across = east * axis.cos() - north * axis.sin();
    let major = estimate.ellipse_major_m / 2.0;
    let minor = (estimate.ellipse_minor_m / 2.0).max(1.0);
    (along / major).hypot(across / minor)
}

fn valley_road() -> Drive {
    north_road(2_000.0, 3_000.0, 6_000.0)
}

fn reflector(east: f64, north: f64, height_m: f64, gain_db: f64) -> Reflector {
    Reflector {
        at: geo::offset_m(HOME, east, north),
        height_m,
        gain_db,
    }
}

fn ridge(east: f64, from_north: f64, to_north: f64, loss_db: f64) -> Ridge {
    Ridge {
        from: geo::offset_m(HOME, east, from_north),
        to: geo::offset_m(HOME, east, to_north),
        loss_db,
    }
}

fn last_estimate(steps: &[Step]) -> DfEstimate {
    steps
        .last()
        .and_then(|step| step.estimate)
        .expect("an estimate")
}

#[test]
fn a_cliff_echo_in_a_valley_still_finds_the_transmitter() {
    let mut drive = valley_road();
    drive
        .world
        .reflectors
        .push(reflector(-400.0, 2_500.0, 100.0, -3.0));
    let steps = drive.run(&TriangulationParams::default());
    assert!(worst_bearing_error(&steps) < 2.0);
    let miss = final_miss_m(&drive, &steps);
    assert!(miss < 60.0, "missed by {miss:.0} m");
    assert!(last_estimate(&steps).converged);
}

#[test]
fn a_deep_valley_with_echoes_on_both_walls_crosses_at_the_transmitter() {
    let mut drive = valley_road();
    drive.world.ridges.push(ridge(600.0, -100.0, 6_100.0, 20.0));
    for north in [1_000.0, 2_500.0, 4_000.0, 5_500.0] {
        drive
            .world
            .reflectors
            .push(reflector(-500.0, north, 300.0, -3.0));
    }
    let steps = drive.run(&TriangulationParams::default());
    let miss = final_miss_m(&drive, &steps);
    assert!(miss < 100.0, "missed by {miss:.0} m");
    assert!(sigmas_away(&last_estimate(&steps), drive.emitter) < 2.0);
}

#[test]
fn a_ridge_shadow_leaves_its_echo_a_lesser_candidate() {
    let mut drive = valley_road();
    drive
        .world
        .ridges
        .push(ridge(800.0, 1_500.0, 3_200.0, 25.0));
    drive
        .world
        .reflectors
        .push(reflector(-300.0, 4_500.0, 200.0, -6.0));
    let steps = drive.run(&TriangulationParams::default());
    let echoed = bearing_errors(&steps)
        .iter()
        .filter(|error| **error > 20.0)
        .count();
    assert!(echoed > steps.len() / 4, "{echoed} of {}", steps.len());
    let miss = final_miss_m(&drive, &steps);
    assert!(miss < 100.0, "missed by {miss:.0} m");
    let first = steps.last().and_then(|step| step.emitters.first()).copied();
    let first = first.expect("an emitter");
    assert!(geo::distance_m(place(&first), drive.emitter) < 100.0);
}

#[test]
fn a_merged_wall_echo_at_walking_pace_is_not_trusted() {
    let mut drive = valley_road();
    drive.speed_mps = 3.0;
    drive.route = Route::new(vec![HOME, geo::destination(HOME, 0.0, 1_500.0)]);
    drive
        .world
        .reflectors
        .push(reflector(-60.0, 700.0, 20.0, 0.0));
    let steps = drive.run(&TriangulationParams::default());
    let trusted_wrong: Vec<(f64, f32)> = steps
        .iter()
        .filter_map(|step| {
            let bearing = step.bearing.as_ref()?;
            let error = error_deg(f64::from(bearing.bearing_deg), step.truth_deg);
            (error > 20.0 && bearing.confidence > 0.6).then_some((step.at_s, bearing.confidence))
        })
        .collect();
    assert!(trusted_wrong.is_empty(), "{trusted_wrong:?}");
}

#[test]
fn a_transmitter_on_a_mountain_top_reads_true() {
    let mut drive = valley_road();
    drive.world.emitter_height_m = 700.0;
    let steps = drive.run(&TriangulationParams::default());
    assert!(worst_bearing_error(&steps) < 1.0);
    let miss = final_miss_m(&drive, &steps);
    assert!(miss < 60.0, "missed by {miss:.0} m");
}

#[test]
fn driving_straight_at_it_claims_no_position() {
    let drive = Drive::new(
        Route::new(vec![HOME, geo::destination(HOME, 0.0, 4_000.0)]),
        geo::destination(HOME, 0.0, 6_000.0),
    );
    let steps = drive.run(&TriangulationParams::default());
    let last = steps.last().expect("steps");
    assert!(last.estimate.is_none(), "{:?}", last.estimate);
    assert!(last.emitters.is_empty(), "{:?}", last.emitters);
    let nav = last.nav.expect("a target");
    assert_eq!(nav.kind, NavTargetKind::Probe);
    assert!(error_deg(nav.bearing_deg, 0.0) < 5.0, "{nav:?}");
}

#[test]
fn a_heading_bias_moves_the_fix_inside_its_ellipse() {
    let mut drive = valley_road();
    drive.compass.bias_deg = 5.0;
    let steps = drive.run(&TriangulationParams::default());
    let estimate = last_estimate(&steps);
    let away = sigmas_away(&estimate, drive.emitter);
    assert!(away < 3.0, "{away:.1} sigmas: {estimate:?}");
}

#[test]
fn bursts_and_a_gps_gap_still_locate() {
    let mut drive = valley_road();
    drive.world.bursts = Some(Bursts {
        on_s: 3.0,
        every_s: 20.0,
    });
    drive.world.gps_gaps.push((100.0, 160.0));
    let steps = drive.run(&TriangulationParams::default());
    assert!(
        steps
            .iter()
            .all(|step| !(100.0..160.0).contains(&step.at_s))
    );
    let miss = final_miss_m(&drive, &steps);
    assert!(miss < 80.0, "missed by {miss:.0} m");
}

#[test]
fn a_far_transmitter_stays_honestly_unconverged() {
    let drive = north_road(9_000.0, 3_000.0, 6_000.0);
    let steps = drive.run(&TriangulationParams::default());
    let estimate = last_estimate(&steps);
    assert!(!estimate.converged, "{estimate:?}");
    assert!(sigmas_away(&estimate, drive.emitter) < 2.0, "{estimate:?}");
}

#[test]
fn passing_close_by_pins_it() {
    let drive = north_road(150.0, 3_000.0, 6_000.0);
    let steps = drive.run(&TriangulationParams::default());
    let miss = final_miss_m(&drive, &steps);
    assert!(miss < 40.0, "missed by {miss:.0} m");
}
