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

fn ridge_drive(echo_db: f64) -> (Drive, LatLon) {
    let echo = geo::offset_m(HOME, -300.0, 4_500.0);
    let mut drive = valley_road();
    drive
        .world
        .ridges
        .push(ridge(800.0, 1_500.0, 3_200.0, 25.0));
    drive.world.reflectors.push(Reflector {
        at: echo,
        height_m: 200.0,
        gain_db: echo_db,
    });
    (drive, echo)
}

fn guided_to(steps: &[Step]) -> Vec<(f64, LatLon)> {
    steps
        .iter()
        .filter_map(|step| {
            let nav = step.nav?;
            (nav.kind == NavTargetKind::Estimate).then_some((
                step.at_s,
                LatLon {
                    lat: nav.lat,
                    lon: nav.lon,
                },
            ))
        })
        .collect()
}

#[test]
fn a_strong_ridge_echo_never_leads_to_a_ghost_and_fades_at_the_end() {
    let (drive, echo) = ridge_drive(-6.0);
    let steps = drive.run(&TriangulationParams::default());
    let echoed = bearing_errors(&steps)
        .iter()
        .filter(|error| **error > 20.0)
        .count();
    assert!(echoed > steps.len() / 4, "{echoed} of {}", steps.len());
    let ghosts: Vec<(f64, f64)> = guided_to(&steps)
        .into_iter()
        .filter(|(_, at)| {
            geo::distance_m(*at, drive.emitter) > 600.0 && geo::distance_m(*at, echo) > 600.0
        })
        .map(|(at_s, at)| (at_s, geo::distance_m(at, drive.emitter)))
        .collect();
    assert!(ghosts.is_empty(), "{ghosts:?}");
    let last = steps.last().expect("steps");
    assert!(final_miss_m(&drive, &steps) < 60.0);
    assert!(
        last.emitters
            .iter()
            .all(|emitter| geo::distance_m(place(emitter), drive.emitter) < 600.0),
        "{:?}",
        last.emitters
    );
}

#[test]
fn a_typical_ridge_echo_only_ever_leads_to_the_transmitter() {
    let (drive, _) = ridge_drive(-15.0);
    let steps = drive.run(&TriangulationParams::default());
    let guided = guided_to(&steps);
    assert!(!guided.is_empty());
    let wrong: Vec<(f64, f64)> = guided
        .into_iter()
        .map(|(at_s, at)| (at_s, geo::distance_m(at, drive.emitter)))
        .filter(|(_, miss)| *miss > 150.0)
        .collect();
    assert!(wrong.is_empty(), "{wrong:?}");
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

fn square_loop() -> Drive {
    let corner = |east: f64, north: f64| geo::offset_m(HOME, east, north);
    let mut drive = Drive::new(
        Route::new(vec![
            corner(0.0, 0.0),
            corner(0.0, 4_000.0),
            corner(3_000.0, 4_000.0),
            corner(3_000.0, 0.0),
            corner(0.0, 0.0),
        ]),
        corner(1_500.0, 2_500.0),
    );
    drive.speed_mps = 12.0;
    drive
}

#[test]
fn a_loop_learns_a_mount_offset() {
    let mut drive = square_loop();
    drive.compass.bias_deg = -8.0;
    let steps = drive.run(&TriangulationParams::default());
    let align = steps
        .last()
        .and_then(|step| step.align_deg)
        .expect("an alignment");
    assert!((align + 8.0).abs() < 1.0, "{align}");
    assert!(final_miss_m(&drive, &steps) < 60.0);
}

#[test]
fn no_offset_is_learned_without_one() {
    for drive in [valley_road(), ridge_drive(-6.0).0] {
        let steps = drive.run(&TriangulationParams::default());
        let learned: Vec<f32> = steps.iter().filter_map(|step| step.align_deg).collect();
        assert!(learned.is_empty(), "{learned:?}");
    }
}

#[test]
fn a_straight_pass_leaves_an_offset_to_the_ellipse() {
    let mut drive = valley_road();
    drive.compass.bias_deg = 5.0;
    let steps = drive.run(&TriangulationParams::default());
    assert!(steps.iter().all(|step| step.align_deg.is_none()));
    let estimate = last_estimate(&steps);
    let away = sigmas_away(&estimate, drive.emitter);
    assert!(away < 3.0, "{away:.1} sigmas: {estimate:?}");
}
