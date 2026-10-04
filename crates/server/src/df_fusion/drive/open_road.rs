use sdrmm_wire::{NavTargetKind, TriangulationParams, geo};

use super::*;

#[test]
fn every_bearing_points_at_the_transmitter_while_driving() {
    let steps = north_road(2_000.0, 3_000.0, 6_000.0).run(&TriangulationParams::default());
    assert!(steps.len() > 500, "{}", steps.len());
    let worst = worst_bearing_error(&steps);
    assert!(worst < 3.0, "worst bearing error {worst:.1} deg");
}

#[test]
fn driving_past_a_transmitter_locates_it() {
    let drive = north_road(2_000.0, 3_000.0, 6_000.0);
    let steps = drive.run(&TriangulationParams::default());
    let last = steps.last().expect("steps");
    let estimate = last.estimate.expect("an estimate");
    let miss = geo::distance_m(place(&estimate), drive.emitter);
    assert!(miss < 150.0, "missed by {miss:.0} m: {estimate:?}");
    assert!(estimate.converged, "{estimate:?}");
}

#[test]
fn nav_leads_toward_the_transmitter_and_never_behind() {
    let drive = north_road(2_000.0, 3_000.0, 6_000.0);
    let steps = drive.run(&TriangulationParams::default());
    let mut wrong = Vec::new();
    for step in &steps {
        let Some(nav) = step.nav else {
            continue;
        };
        let off = error_deg(nav.bearing_deg, step.truth_deg);
        if off > 45.0 {
            wrong.push((step.at_s, nav.kind, off));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} targets point away: {:?}",
        wrong.len(),
        steps.len(),
        &wrong[..wrong.len().min(8)]
    );
}

#[test]
fn nav_settles_on_the_estimate_once_it_has_crossed_bearings() {
    let drive = north_road(2_000.0, 3_000.0, 6_000.0);
    let steps = drive.run(&TriangulationParams::default());
    let last = steps.last().and_then(|step| step.nav).expect("a target");
    assert_eq!(last.kind, NavTargetKind::Estimate, "{last:?}");
    let target = LatLon {
        lat: last.lat,
        lon: last.lon,
    };
    let miss = geo::distance_m(target, drive.emitter);
    assert!(miss < 150.0, "target {miss:.0} m off");
}

#[test]
fn nav_revisions_stay_rare_while_the_target_holds_still() {
    let drive = north_road(2_000.0, 3_000.0, 6_000.0);
    let steps = drive.run(&TriangulationParams::default());
    let revisions: Vec<(f64, u32)> = steps
        .iter()
        .filter_map(|step| step.nav.map(|nav| (step.at_s, nav.revision)))
        .collect();
    let (first_s, first) = revisions.first().copied().expect("targets");
    let (last_s, last) = revisions.last().copied().expect("targets");
    let per_minute = f64::from(last - first) / ((last_s - first_s) / 60.0);
    assert!(
        per_minute < 6.0,
        "{per_minute:.1} reroutes a minute over {:.0} s",
        last_s - first_s
    );
}

#[test]
fn a_loop_with_turns_keeps_true_bearings_and_finds_the_transmitter() {
    let corner = |east: f64, north: f64| geo::offset_m(HOME, east, north);
    let emitter = corner(1_500.0, 2_500.0);
    let mut drive = Drive::new(
        Route::new(vec![
            corner(0.0, 0.0),
            corner(0.0, 4_000.0),
            corner(3_000.0, 4_000.0),
            corner(3_000.0, 0.0),
            corner(0.0, 0.0),
        ]),
        emitter,
    );
    drive.speed_mps = 12.0;
    let steps = drive.run(&TriangulationParams::default());
    let worst = worst_bearing_error(&steps);
    assert!(worst < 5.0, "worst bearing error {worst:.1} deg");
    let estimate = steps
        .last()
        .and_then(|step| step.estimate)
        .expect("an estimate");
    let miss = geo::distance_m(place(&estimate), emitter);
    assert!(miss < 100.0, "missed by {miss:.0} m: {estimate:?}");
}

#[test]
fn driving_away_the_nav_turns_back_toward_the_transmitter() {
    let drive = Drive::new(
        Route::new(vec![HOME, geo::destination(HOME, 0.0, 3_000.0)]),
        geo::offset_m(HOME, 1_500.0, -2_000.0),
    );
    let steps = drive.run(&TriangulationParams::default());
    let last = steps.last().expect("steps");
    let nav = last.nav.expect("a target");
    assert!(error_deg(nav.bearing_deg, last.truth_deg) < 20.0, "{nav:?}");
    assert!(geo::wrap_180(nav.bearing_deg).abs() > 90.0, "{nav:?}");
}
