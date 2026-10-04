use num_complex::Complex;
use sdrmm_channels::array_processor::{
    ArrayBlock, ArrayCtx, ArrayProcessor, CalView, CorrectionView, GeoFix, OutputSlots, Pose,
    ProcessorOutput, create_processor, geometry_of,
};
use sdrmm_dsp::manifold::Direction;
use sdrmm_dsp::scene::{ArrayScene, HeadingTrack, SceneCopy, SceneSignal, SceneSource};
use sdrmm_wire::{
    ArrayGeometry, ArrayTuningMode, Attitude, Coherence, DecoderEvent, DfBearing, DfEstimate,
    DfParams, HeadingSource, NavTarget, PositionFix, ProcessorParams, ProcessorReading, RdsUpdate,
    TriangulationParams, Winding,
    geo::{self, Enu, LatLon},
};

use super::NodeFusion;

mod open_road;
mod terrain;

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
const MAX_SNR_DB: f64 = 40.0;
const SILENT_DB: f32 = -200.0;
const LIGHT_M_S: f64 = 299_792_458.0;
const REFLECTION_PHASE_DEG: f64 = 180.0;
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

#[derive(Clone, Copy)]
struct Reflector {
    at: LatLon,
    height_m: f64,
    gain_db: f64,
}

#[derive(Clone, Copy)]
struct Ridge {
    from: LatLon,
    to: LatLon,
    loss_db: f64,
}

#[derive(Clone, Copy)]
struct Bursts {
    on_s: f64,
    every_s: f64,
}

#[derive(Default)]
struct World {
    emitter_height_m: f64,
    reflectors: Vec<Reflector>,
    ridges: Vec<Ridge>,
    bursts: Option<Bursts>,
    gps_gaps: Vec<(f64, f64)>,
}

#[derive(Clone, Copy, Debug)]
struct Arrival {
    azimuth_deg: f64,
    elevation_deg: f64,
    power_db: f64,
    length_m: f64,
    reflected: bool,
}

fn crosses(enu: &Enu, from: LatLon, to: LatLon, ridge: &Ridge) -> bool {
    let (p, q) = (enu.to_enu(from), enu.to_enu(to));
    let (a, b) = (enu.to_enu(ridge.from), enu.to_enu(ridge.to));
    let side = |o: (f64, f64), u: (f64, f64), v: (f64, f64)| {
        (u.0 - o.0) * (v.1 - o.1) - (u.1 - o.1) * (v.0 - o.0)
    };
    side(p, q, a) * side(p, q, b) < 0.0 && side(a, b, p) * side(a, b, q) < 0.0
}

impl World {
    fn loss_db(&self, from: LatLon, to: LatLon) -> f64 {
        let enu = Enu::new(HOME);
        self.ridges
            .iter()
            .filter(|ridge| crosses(&enu, from, to, ridge))
            .map(|ridge| ridge.loss_db)
            .sum()
    }

    fn transmitting(&self, at_s: f64) -> bool {
        self.bursts
            .is_none_or(|bursts| at_s.rem_euclid(bursts.every_s) < bursts.on_s)
    }

    fn gps(&self, at_s: f64) -> bool {
        !self
            .gps_gaps
            .iter()
            .any(|(from, to)| (*from..*to).contains(&at_s))
    }

    fn arrivals(&self, car: LatLon, emitter: LatLon) -> Vec<Arrival> {
        let direct_m = geo::distance_m(car, emitter);
        let mut arrivals = vec![Arrival {
            azimuth_deg: geo::bearing_deg(car, emitter),
            elevation_deg: self.emitter_height_m.atan2(direct_m).to_degrees(),
            power_db: path_db(direct_m) - self.loss_db(car, emitter),
            length_m: direct_m.hypot(self.emitter_height_m),
            reflected: false,
        }];
        for reflector in &self.reflectors {
            let near_m = geo::distance_m(car, reflector.at);
            let far_m = geo::distance_m(reflector.at, emitter);
            let length_m = near_m.hypot(reflector.height_m)
                + far_m.hypot(self.emitter_height_m - reflector.height_m);
            arrivals.push(Arrival {
                azimuth_deg: geo::bearing_deg(car, reflector.at),
                elevation_deg: reflector.height_m.atan2(near_m).to_degrees(),
                power_db: path_db(length_m) + reflector.gain_db
                    - self.loss_db(car, reflector.at)
                    - self.loss_db(reflector.at, emitter),
                length_m,
                reflected: true,
            });
        }
        arrivals
    }
}

fn path_db(length_m: f64) -> f64 {
    (SNR_AT_KM_DB - 20.0 * (length_m / 1_000.0).max(0.05).log10()).min(MAX_SNR_DB)
}

fn tone() -> SceneSignal {
    SceneSignal::Tone {
        offset_hz: OFFSET_HZ,
    }
}

fn sources(arrivals: &[Arrival], on: bool) -> Vec<SceneSource> {
    let wavelength_m = LIGHT_M_S / (CENTER_HZ + OFFSET_HZ);
    let Some(direct) = arrivals.first() else {
        return Vec::new();
    };
    let direction = |arrival: &Arrival| Direction::new(arrival.azimuth_deg, arrival.elevation_deg);
    let master_db = if on {
        direct.power_db as f32
    } else {
        SILENT_DB
    };
    let mut out = vec![SceneSource::new(direction(direct), master_db, tone())];
    for arrival in &arrivals[1..] {
        let extra = (arrival.length_m - direct.length_m) / wavelength_m;
        let flip = if arrival.reflected {
            REFLECTION_PHASE_DEG
        } else {
            0.0
        };
        let mut copy = SceneSource::new(direction(arrival), master_db, tone());
        copy.copy_of = Some(SceneCopy {
            source: 0,
            amplitude: 10f64.powf((arrival.power_db - direct.power_db) / 20.0) as f32,
            phase_deg: (flip - 360.0 * extra.fract()) as f32,
        });
        out.push(copy);
    }
    out
}

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
        .with_seed(11);
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

    fn block(&mut self, sources: Vec<SceneSource>, heading_deg: f64, pose: Pose) -> Vec<DfBearing> {
        self.scene.sources = sources;
        self.scene.heading = HeadingTrack::Fixed(heading_deg);
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
    emitters: Vec<DfEstimate>,
    align_deg: Option<f32>,
}

struct Drive {
    route: Route,
    emitter: LatLon,
    speed_mps: f64,
    compass: Compass,
    world: World,
}

impl Drive {
    fn new(route: Route, emitter: LatLon) -> Self {
        Self {
            route,
            emitter,
            speed_mps: 14.0,
            compass: GOOD_COMPASS,
            world: World::default(),
        }
    }

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
            let yaw_rate = previous_heading.map(|from| geo::wrap_180(heading_deg - from) / BLOCK_S);
            previous_heading = Some(heading_deg);
            let gps = self.world.gps(at_s);
            let pose = self.pose(car, heading_deg, yaw_rate, gps);
            let arrivals = self.world.arrivals(car, self.emitter);
            let on = self.world.transmitting(at_s);
            let bearings = rig.block(sources(&arrivals, on), heading_deg, pose);
            let fix = gps.then(|| self.fix(car, heading_deg));
            let mut outcome = fusion.guide(fix.as_ref(), at_s);
            for bearing in &bearings {
                if let Ok(seen) = fusion.observe(bearing, at_s, AT) {
                    outcome = seen;
                }
            }
            if !bearings.is_empty() {
                steps.push(Step {
                    at_s,
                    truth_deg: geo::bearing_deg(car, self.emitter),
                    bearing: bearings.last().cloned(),
                    nav: outcome.state.nav,
                    estimate: outcome.state.estimate,
                    emitters: outcome.state.emitters.clone(),
                    align_deg: outcome
                        .state
                        .stations
                        .first()
                        .and_then(|station| station.align_deg),
                });
            }
            at_s += BLOCK_S;
        }
        steps
    }

    fn pose(&self, car: LatLon, heading_deg: f64, yaw_rate_dps: Option<f64>, gps: bool) -> Pose {
        Pose {
            heading_deg: Some(geo::wrap_360(heading_deg + self.compass.bias_deg)),
            heading_sigma_deg: self.compass.sigma_deg,
            yaw_rate_dps: yaw_rate_dps.map(|rate| rate as f32),
            fix: gps.then_some(GeoFix {
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
    let abeam = geo::destination(HOME, 0.0, ahead_m);
    Drive::new(
        Route::new(vec![HOME, geo::destination(HOME, 0.0, length_m)]),
        geo::destination(abeam, 90.0, east_of_road_m),
    )
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

fn bearing_errors(steps: &[Step]) -> Vec<f64> {
    steps
        .iter()
        .filter_map(|step| {
            step.bearing
                .as_ref()
                .map(|bearing| error_deg(f64::from(bearing.bearing_deg), step.truth_deg))
        })
        .collect()
}

fn worst_bearing_error(steps: &[Step]) -> f64 {
    bearing_errors(steps).into_iter().fold(0.0, f64::max)
}

fn final_miss_m(drive: &Drive, steps: &[Step]) -> f64 {
    let estimate = steps
        .last()
        .and_then(|step| step.estimate)
        .expect("an estimate");
    geo::distance_m(place(&estimate), drive.emitter)
}
