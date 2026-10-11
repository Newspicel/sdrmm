use super::*;

const PLAIN: Mount = Mount {
    azimuth_max_deg: 360.0,
    elevation_max_deg: 90.0,
    flips: false,
};

const OVERLAP: Mount = Mount {
    azimuth_max_deg: 450.0,
    ..PLAIN
};

const FLIPPING: Mount = Mount {
    elevation_max_deg: 180.0,
    flips: true,
    ..PLAIN
};

fn pass(points: &[(f64, f64)]) -> Vec<PassPoint> {
    points
        .iter()
        .zip(0..)
        .map(|(&(azimuth_deg, elevation_deg), index)| PassPoint {
            at: index * 60,
            azimuth_deg,
            elevation_deg,
        })
        .collect()
}

fn sky(azimuth_deg: f64, elevation_deg: f64) -> SkyPoint {
    SkyPoint {
        azimuth_deg,
        elevation_deg,
    }
}

fn north_crossing() -> Vec<PassPoint> {
    pass(&[
        (300.0, 0.0),
        (330.0, 20.0),
        (0.0, 35.0),
        (30.0, 20.0),
        (60.0, 0.0),
    ])
}

fn overhead() -> Vec<PassPoint> {
    pass(&[
        (90.0, 0.0),
        (90.0, 45.0),
        (90.0, 89.0),
        (268.0, 45.0),
        (268.0, 0.0),
    ])
}

#[test]
fn a_pass_that_stays_south_is_tracked_directly() {
    let track = pass(&[(120.0, 0.0), (160.0, 40.0), (200.0, 70.0), (240.0, 0.0)]);
    let plan = plan(&track, 120, FLIPPING, 120.0).expect("fits");
    assert!(!plan.flip_before && !plan.flip_after);
    assert_eq!(plan.path[0].1, 120.0);
}

#[test]
fn an_overlap_rotator_crosses_north_without_unwinding() {
    let plan = plan(&north_crossing(), 120, OVERLAP, 300.0).expect("fits past north");
    let azimuths: Vec<f64> = plan.path.iter().map(|&(_, az)| az).collect();
    assert_eq!(azimuths, vec![300.0, 330.0, 360.0, 390.0, 420.0]);
    let (target, flipped) = steer(sky(15.0, 30.0), 150, Some(&plan), OVERLAP, 0.0);
    assert!(!flipped);
    assert_eq!(target, sky(375.0, 30.0));
}

#[test]
fn a_plain_rotator_flips_over_the_top_instead_of_crossing_north() {
    assert_eq!(plan(&north_crossing(), 120, PLAIN, 300.0), None);
    let plan = plan(&north_crossing(), 120, FLIPPING, 120.0).expect("fits flipped");
    assert!(plan.flip_before && plan.flip_after);
    let (target, flipped) = steer(sky(330.0, 20.0), 60, Some(&plan), FLIPPING, 0.0);
    assert!(flipped);
    assert_eq!(target, sky(150.0, 160.0));
}

#[test]
fn an_overhead_pass_keeps_the_azimuth_and_swings_the_elevation_over() {
    let plan = plan(&overhead(), 150, FLIPPING, 90.0).expect("fits");
    assert!(!plan.flip_before && plan.flip_after);
    let rising = steer(sky(90.0, 60.0), 90, Some(&plan), FLIPPING, 0.0);
    let setting = steer(sky(270.0, 60.0), 200, Some(&plan), FLIPPING, 0.0);
    assert_eq!(rising, (sky(90.0, 60.0), false));
    assert_eq!(setting, (sky(90.0, 120.0), true));
}

#[test]
fn without_a_flip_an_overhead_pass_swings_the_azimuth() {
    let plan = plan(&overhead(), 150, PLAIN, 90.0).expect("fits");
    let setting = steer(sky(270.0, 60.0), 200, Some(&plan), PLAIN, 0.0);
    assert_eq!(setting, (sky(270.0, 60.0), false));
}

#[test]
fn a_plan_is_kept_for_the_same_pass_on_the_same_mount() {
    let plan = plan(&overhead(), 150, PLAIN, 90.0).expect("fits");
    assert!(plan.serves(PLAIN, 170));
    assert!(!plan.serves(PLAIN, 5_000));
    assert!(!plan.serves(OVERLAP, 150));
}

#[test]
fn without_a_plan_the_nearest_turn_is_taken() {
    let (target, _) = steer(sky(10.0, 20.0), 0, None, OVERLAP, 355.0);
    assert_eq!(target, sky(370.0, 20.0));
    let (target, _) = steer(sky(10.0, 20.0), 0, None, PLAIN, 355.0);
    assert_eq!(target, sky(10.0, 20.0));
}

#[test]
fn targets_stay_inside_the_mount() {
    let level = Mount {
        elevation_max_deg: 0.0,
        ..PLAIN
    };
    assert_eq!(level.reach(sky(400.0, 45.0)), sky(360.0, 0.0));
}
