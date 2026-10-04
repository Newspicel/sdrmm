use num_complex::Complex;
use sdrmm_channels::array_processor::{
    ArrayBlock, ArrayCtx, ArrayProcessor, CalView, CorrectionView, GeoFix, OutputSlots, Pose,
    ProcessorOutput, create_processor, geometry_of,
};
use sdrmm_dsp::manifold::Direction;
use sdrmm_dsp::scene::{ArrayScene, HeadingTrack, SceneSignal, SceneSource};
use sdrmm_wire::{
    ArrayGeometry, ArrayTuningMode, Attitude, Coherence, DecoderEvent, DfBearing, DfEstimate,
    DfParams, HeadingSource, NavTarget, NavTargetKind, PositionFix, ProcessorParams,
    ProcessorReading, RdsUpdate, TriangulationParams, Winding,
    geo::{self, LatLon},
};

use super::NodeFusion;

const RATE: f64 = 48_000.0;
const CENTER_HZ: f64 = 433.92e6;
const OFFSET_HZ: f64 = 12_000.0;
const BANDWIDTH_HZ: f64 = 10_000.0;
const RADIUS_M: f64 = 0.2939;
const LANES: usize = 5;
const BLOCK: usize = 2_400;
const BLOCK_S: f64 = BLOCK as f64 / RATE;
const REPORT_MS: u32 = 500;
const SNR_AT_KM_DB: f64 = 20.0;
const START_NS: u64 = 1_790_000_000_000_000_000;
const AT: &str = "2026-01-01T00:00:00Z";
const HOME: LatLon = LatLon {
    lat: 51.5,
    lon: 7.0,
};

struct Route {
    points: Vec<LatLon>,
    lengths: Vec<f64>,
}

impl Route {
    fn new(points: Vec<LatLon>) -> Self {
        let lengths = points
            .windows(2)
            .map(|pair| geo::distance_m(pair[0], pair[1]))
            .collect();
        Self { points, lengths }
    }

    fn total_m(&self) -> f64 {
        self.lengths.iter().sum()
    }

    fn at(&self, along_m: f64) -> LatLon {
        let mut left = along_m.clamp(0.0, self.total_m());
        for (index, length) in self.lengths.iter().enumerate() {
            if left <= *length {
                let heading = geo::bearing_deg(self.points[index], self.points[index + 1]);
                return geo::destination(self.points[index], heading, left);
            }
            left -= length;
        }
        self.points[self.points.len() - 1]
    }

    fn heading_deg(&self, along_m: f64) -> f64 {
        geo::bearing_deg(self.at(along_m - 5.0), self.at(along_m + 5.0))
    }
}

#[derive(Clone, Copy)]
struct Compass {
    bias_deg: f64,
    sigma_deg: f32,
}

const GOOD_COMPASS: Compass = Compass {
    bias_deg: 0.0,
    sigma_deg: 2.0,
};

struct Rig {
    centers: Vec<f64>,
    scene: ArrayScene,
    processor: Box<dyn ArrayProcessor>,
    report: Option<ProcessorReading>,
    events: Vec<DecoderEvent>,
    unix_ns: u64,
}

impl Rig {
    fn kraken() -> Self {
        let geometry = ArrayGeometry::Uca {
            radius_m: RADIUS_M,
            first_deg: 0.0,
            winding: Winding::Clockwise,
        };
        let positions = geometry.positions(LANES).expect("kraken positions");
        let centers = vec![CENTER_HZ; LANES];
        let scene = ArrayScene::new(
            geometry_of(&geometry, LANES).expect("kraken geometry"),
            CENTER_HZ,
            RATE,
        )
        .with_noise_db(0.0)
        .with_seed(11)
        .with_source(SceneSource::new(
            Direction::horizon(0.0),
            0.0,
            SceneSignal::Tone {
                offset_hz: OFFSET_HZ,
            },
        ));
        let df = DfParams {
            offset_hz: OFFSET_HZ,
            bandwidth_hz: BANDWIDTH_HZ,
            report_ms: REPORT_MS,
            ..DfParams::default()
        };
        let processor = {
            let ctx = ArrayCtx {
                node: "roof",
                lanes: LANES,
                sample_rate: RATE,
                center_hz: CENTER_HZ,
                lane_centers_hz: &centers,
                geometry: &geometry,
                positions_m: &positions,
                manifold: None,
                tier: Coherence::PhaseCoherent,
                tuning: ArrayTuningMode::Together,
                max_block: BLOCK,
            };
            create_processor(&ctx, &ProcessorParams::Df(df)).expect("a df processor")
        };
        Self {
            centers,
            scene,
            processor,
            report: ProcessorReading::empty("df"),
            events: vec![DecoderEvent::Rds(RdsUpdate::default()); 2],
            unix_ns: START_NS,
        }
    }

    fn block(&mut self, emitter_deg: f64, snr_db: f64, pose: Pose) -> Vec<DfBearing> {
        let heading = pose.heading_deg.unwrap_or(0.0);
        self.scene.sources[0].direction = Direction::horizon(emitter_deg);
        self.scene.sources[0].power_db = snr_db as f32;
        self.scene.heading = HeadingTrack::Fixed(heading);
        let rendered = self.scene.render(BLOCK).expect("a rendered block");
        let lanes: Vec<&[Complex<f32>]> = rendered.iter().map(Vec::as_slice).collect();
        let block = ArrayBlock {
            lanes: &lanes,
            corrected: true,
            correction: CorrectionView::identity(),
            first_index: 0,
            unix_ns: self.unix_ns,
            generation: 0,
            gap_before: false,
            centers_hz: &self.centers,
            cal: CalView::default(),
            pose,
        };
        let mut out = ProcessorOutput::new(OutputSlots {
            report: self.report.as_mut(),
            surface: None,
            events: &mut self.events,
            lanes: &mut [],
        });
        self.processor.process(&block, &mut out);
        let tally = out.tally();
        self.unix_ns += (BLOCK_S * 1e9) as u64;
        self.events[..tally.events]
            .iter()
            .filter_map(|event| match event {
                DecoderEvent::Df(bearing) => Some(bearing.clone()),
                _ => None,
            })
            .collect()
    }
}

struct Step {
    at_s: f64,
    truth_deg: f64,
    bearing: Option<DfBearing>,
    nav: Option<NavTarget>,
    estimate: Option<DfEstimate>,
}

struct Drive {
    route: Route,
    emitter: LatLon,
    speed_mps: f64,
    compass: Compass,
}

impl Drive {
    fn run(&self, params: &TriangulationParams) -> Vec<Step> {
        let mut rig = Rig::kraken();
        let mut fusion = NodeFusion::new(params);
        let mut steps = Vec::new();
        let mut previous_heading: Option<f64> = None;
        let total_s = self.route.total_m() / self.speed_mps;
        let mut at_s = 0.0;
        while at_s <= total_s {
            let along = at_s * self.speed_mps;
            let car = self.route.at(along);
            let heading_deg = self.route.heading_deg(along);
            let truth_deg = geo::bearing_deg(car, self.emitter);
            let yaw_rate = previous_heading.map(|from| geo::wrap_180(heading_deg - from) / BLOCK_S);
            previous_heading = Some(heading_deg);
            let pose = self.pose(car, heading_deg, yaw_rate);
            let range_km = geo::distance_m(car, self.emitter) / 1_000.0;
            let snr_db = (SNR_AT_KM_DB - 20.0 * range_km.max(0.05).log10()).clamp(-10.0, 40.0);
            let bearings = rig.block(truth_deg, snr_db, pose);
            let fix = self.fix(car, heading_deg);
            let mut outcome = fusion.guide(Some(&fix), at_s);
            for bearing in &bearings {
                if let Ok(seen) = fusion.observe(bearing, at_s, AT) {
                    outcome = seen;
                }
            }
            if !bearings.is_empty() {
                steps.push(Step {
                    at_s,
                    truth_deg,
                    bearing: bearings.last().cloned(),
                    nav: outcome.state.nav,
                    estimate: outcome.state.estimate,
                });
            }
            at_s += BLOCK_S;
        }
        steps
    }

    fn pose(&self, car: LatLon, heading_deg: f64, yaw_rate_dps: Option<f64>) -> Pose {
        Pose {
            heading_deg: Some(geo::wrap_360(heading_deg + self.compass.bias_deg)),
            heading_sigma_deg: self.compass.sigma_deg,
            yaw_rate_dps: yaw_rate_dps.map(|rate| rate as f32),
            fix: Some(GeoFix {
                lat: car.lat,
                lon: car.lon,
                altitude_m: None,
                accuracy_m: Some(5.0),
                speed_mps: Some(self.speed_mps as f32),
            }),
            moving: true,
            follows: true,
        }
    }

    fn fix(&self, car: LatLon, heading_deg: f64) -> PositionFix {
        PositionFix {
            latitude: car.lat,
            longitude: car.lon,
            altitude_m: None,
            accuracy_m: Some(5.0),
            speed_mps: Some(self.speed_mps),
            track_deg: Some(heading_deg),
            time: AT.to_owned(),
            attitude: Attitude {
                heading_deg: Some(heading_deg),
                heading_accuracy_deg: Some(f64::from(self.compass.sigma_deg)),
                heading_source: Some(HeadingSource::Course),
                ..Attitude::default()
            },
        }
    }
}

fn north_road(east_of_road_m: f64, ahead_m: f64, length_m: f64) -> Drive {
    let end = geo::destination(HOME, 0.0, length_m);
    let abeam = geo::destination(HOME, 0.0, ahead_m);
    Drive {
        route: Route::new(vec![HOME, end]),
        emitter: geo::destination(abeam, 90.0, east_of_road_m),
        speed_mps: 14.0,
        compass: GOOD_COMPASS,
    }
}

fn error_deg(got: f64, want: f64) -> f64 {
    geo::wrap_180(got - want).abs()
}

fn place(estimate: &DfEstimate) -> LatLon {
    LatLon {
        lat: estimate.lat,
        lon: estimate.lon,
    }
}

fn worst_bearing_error(steps: &[Step]) -> f64 {
    steps
        .iter()
        .filter_map(|step| {
            step.bearing
                .as_ref()
                .map(|bearing| error_deg(f64::from(bearing.bearing_deg), step.truth_deg))
        })
        .fold(0.0, f64::max)
}

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
    let drive = Drive {
        route: Route::new(vec![
            corner(0.0, 0.0),
            corner(0.0, 4_000.0),
            corner(3_000.0, 4_000.0),
            corner(3_000.0, 0.0),
            corner(0.0, 0.0),
        ]),
        emitter,
        speed_mps: 12.0,
        compass: GOOD_COMPASS,
    };
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
    let drive = Drive {
        route: Route::new(vec![HOME, geo::destination(HOME, 0.0, 3_000.0)]),
        emitter: geo::offset_m(HOME, 1_500.0, -2_000.0),
        speed_mps: 14.0,
        compass: GOOD_COMPASS,
    };
    let steps = drive.run(&TriangulationParams::default());
    let last = steps.last().expect("steps");
    let nav = last.nav.expect("a target");
    assert!(error_deg(nav.bearing_deg, last.truth_deg) < 20.0, "{nav:?}");
    assert!(geo::wrap_180(nav.bearing_deg).abs() > 90.0, "{nav:?}");
}
