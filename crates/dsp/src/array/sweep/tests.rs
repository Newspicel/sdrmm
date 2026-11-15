use super::{fit::LIKELIHOOD_FLOOR, *};
use crate::{math::rng::Rng, math::special::wrap_deg};

const TICK_S: f64 = 0.05;

struct Walk {
    legs: Vec<(f64, f64)>,
    start_deg: f64,
}

impl Walk {
    fn new(start_deg: f64) -> Self {
        Self {
            legs: Vec::new(),
            start_deg,
        }
    }

    fn hold(mut self, seconds: f64) -> Self {
        self.legs.push((seconds, 0.0));
        self
    }

    fn turn(mut self, degrees: f64, rate_dps: f64) -> Self {
        self.legs
            .push((degrees.abs() / rate_dps, degrees.signum() * rate_dps));
        self
    }

    fn duration(&self) -> f64 {
        self.legs.iter().map(|leg| leg.0).sum()
    }

    fn heading(&self, t: f64) -> f64 {
        let mut heading = self.start_deg;
        let mut left = t.max(0.0);
        for &(seconds, rate) in &self.legs {
            let used = left.min(seconds);
            heading += rate * used;
            left -= used;
        }
        heading
    }
}

struct Run {
    lag_s: f64,
    bearing_deg: f64,
    noise_db: f64,
    sigma_deg: f32,
    pattern_gain: f64,
    seed: u64,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            lag_s: 0.1,
            bearing_deg: 123.0,
            noise_db: 2.0,
            sigma_deg: 1.0,
            pattern_gain: 1.0,
            seed: 17,
        }
    }
}

fn pattern(offset_deg: f64) -> f64 {
    let estimator = SweepEstimator::new(SweepConfig::default());
    estimator.template_at(offset_deg)
}

fn sweep(
    estimator: &mut SweepEstimator,
    walk: &Walk,
    run: &Run,
) -> Vec<Result<SweepBearing, SweepReject>> {
    sweep_with_lags(estimator, walk, run)
        .into_iter()
        .map(|(result, _)| result)
        .collect()
}

/// Each closed sweep with the lag the estimator held right after it.
fn sweep_with_lags(
    estimator: &mut SweepEstimator,
    walk: &Walk,
    run: &Run,
) -> Vec<(Result<SweepBearing, SweepReject>, f64)> {
    let mut rng = Rng::mixed(run.seed);
    let mut out = SweepBearing::default();
    let mut results = Vec::new();
    let ticks = (walk.duration() / TICK_S).ceil() as usize + 1;
    for tick in 0..ticks {
        let t = tick as f64 * TICK_S;
        estimator.push_heading(HeadingSample {
            t_s: t,
            heading_deg: norm_deg(walk.heading(t)),
            sigma_deg: run.sigma_deg,
        });
        let heard = t + TICK_S / 2.0;
        let level = -60.0
            + run.pattern_gain * pattern(walk.heading(heard - run.lag_s) - run.bearing_deg)
            + run.noise_db * rng.normal();
        let sample = LevelSample {
            t_s: heard,
            level_db: level as f32,
        };
        if let Some(result) = estimator.push_level(sample, &mut out) {
            results.push((result.map(|()| out.clone()), estimator.lag_s()));
        }
    }
    results
}

fn one_sweep(run: &Run) -> SweepBearing {
    let walk = Walk::new(10.0).hold(1.0).turn(270.0, 90.0).hold(1.5);
    let mut estimator = SweepEstimator::new(SweepConfig::default());
    let results = sweep(&mut estimator, &walk, run);
    assert_eq!(results.len(), 1, "{results:?}");
    results[0].clone().unwrap()
}

#[test]
fn a_sixty_degree_yagi_sweep_finds_the_bearing() {
    let bearing = one_sweep(&Run::default());
    assert!(
        wrap_deg(bearing.bearing_deg - 123.0).abs() < 3.0,
        "{bearing:?}"
    );
    assert_eq!(bearing.rotation, Rotation::Clockwise);
    assert!(bearing.fit > 0.8);
    assert!((bearing.span_deg - 270.0).abs() < 10.0);
    assert!((bearing.rate_dps - 90.0).abs() < 10.0);
    assert!(bearing.confidence > 0.3 && bearing.confidence < 1.0);
}

#[test]
fn opposite_sweeps_estimate_and_remove_the_lag() {
    let walk = (0..3)
        .fold(Walk::new(10.0).hold(1.0), |walk, _| {
            walk.turn(270.0, 90.0).turn(-270.0, 90.0)
        })
        .turn(270.0, 90.0)
        .hold(1.5);
    let run = Run {
        lag_s: 0.15,
        noise_db: 0.5,
        seed: 3,
        ..Run::default()
    };
    let mut estimator = SweepEstimator::new(SweepConfig::default());
    let (bearings, lags): (Vec<f64>, Vec<f64>) = sweep_with_lags(&mut estimator, &walk, &run)
        .into_iter()
        .map(|(result, lag)| (result.expect("bearing").bearing_deg, lag))
        .unzip();
    assert_eq!(bearings.len(), 7, "{bearings:?}");
    let first_bias = wrap_deg(bearings[0] - 123.0);
    assert!((first_bias - 4.5).abs() < 1.0, "first bias {first_bias}");
    assert!((lags[1] - 0.125).abs() < 0.01, "{lags:?}");
    assert!((lags[5] - 0.15).abs() < 0.01, "{lags:?}");
    assert!(wrap_deg(bearings[6] - 123.0).abs() < 3.0, "{bearings:?}");
}

#[test]
fn flat_levels_are_rejected_as_low_contrast() {
    let walk = Walk::new(10.0).hold(1.0).turn(270.0, 90.0).hold(1.5);
    let run = Run {
        noise_db: 0.5,
        pattern_gain: 0.0,
        ..Run::default()
    };
    let mut estimator = SweepEstimator::new(SweepConfig::default());
    let results = sweep(&mut estimator, &walk, &run);
    assert_eq!(results, vec![Err(SweepReject::LowContrast)]);
}

#[test]
fn a_short_swing_is_rejected() {
    let walk = Walk::new(80.0).hold(1.0).turn(90.0, 90.0).hold(1.5);
    let mut estimator = SweepEstimator::new(SweepConfig::default());
    let results = sweep(&mut estimator, &walk, &Run::default());
    assert_eq!(results, vec![Err(SweepReject::ShortSpan)]);
    assert!(estimator.covered_deg() >= 90.0);
    assert!(estimator.covered_deg() <= 110.0);
    assert_eq!(estimator.phase(), SweepPhase::Idle);
}

#[test]
fn heading_sigma_widens_the_bearing_sigma() {
    let sharp = one_sweep(&Run::default());
    let vague = one_sweep(&Run {
        sigma_deg: 10.0,
        ..Run::default()
    });
    assert!(
        vague.sigma_deg >= sharp.sigma_deg + 5.0,
        "{} vs {}",
        vague.sigma_deg,
        sharp.sigma_deg
    );
    assert!(vague.confidence < sharp.confidence);
}

#[test]
fn the_sweep_likelihood_leaves_the_heading_to_the_fusion() {
    let sharp = one_sweep(&Run::default());
    let vague = one_sweep(&Run {
        sigma_deg: 10.0,
        ..Run::default()
    });
    assert!(vague.sigma_deg >= sharp.sigma_deg + 5.0);
    assert_eq!(vague.likelihood, sharp.likelihood);
    let peak = sharp.bearing_deg.round() as usize % 360;
    let near = sharp.likelihood[(peak + 3) % 360];
    assert!(near < sharp.likelihood[peak], "{near}");
}

#[test]
fn the_sweep_likelihood_respects_front_to_back() {
    let bearing = one_sweep(&Run::default());
    let floor = LIKELIHOOD_FLOOR.ln() as f32;
    let peak = bearing.bearing_deg.round() as usize % 360;
    let back = (peak + 180) % 360;
    assert!(
        bearing.likelihood[back] - floor < 0.05,
        "{}",
        bearing.likelihood[back]
    );
    assert!(
        bearing.likelihood[peak] > -0.5,
        "{}",
        bearing.likelihood[peak]
    );
    assert!(
        bearing
            .likelihood
            .iter()
            .all(|&value| value <= 0.0 && value >= floor)
    );
}

#[test]
fn a_fast_spin_is_rejected_as_too_fast() {
    let walk = Walk::new(0.0).hold(1.0).turn(720.0, 600.0).hold(1.5);
    let mut estimator = SweepEstimator::new(SweepConfig::default());
    let results = sweep(&mut estimator, &walk, &Run::default());
    assert_eq!(results.first(), Some(&Err(SweepReject::TooFast)));
}

#[test]
fn levels_without_a_heading_are_dropped() {
    let mut estimator = SweepEstimator::new(SweepConfig::default());
    let mut out = SweepBearing::default();
    let sample = LevelSample {
        t_s: 3.0,
        level_db: -50.0,
    };
    assert_eq!(estimator.push_level(sample, &mut out), None);
    assert_eq!(estimator.phase(), SweepPhase::Idle);
    assert_eq!(estimator.dropped_levels(), 1);
    estimator.push_heading(HeadingSample {
        t_s: 1.0,
        heading_deg: 350.0,
        sigma_deg: 2.0,
    });
    estimator.push_heading(HeadingSample {
        t_s: 1.5,
        heading_deg: 10.0,
        sigma_deg: 4.0,
    });
    estimator.push_heading(HeadingSample {
        t_s: 1.2,
        heading_deg: 0.0,
        sigma_deg: 4.0,
    });
    assert_eq!(estimator.stale_headings(), 1);
    let middle = estimator.heading_at(1.25).unwrap();
    assert!(middle.heading_deg.abs() < 1e-9 || (middle.heading_deg - 360.0).abs() < 1e-9);
    assert!((middle.sigma_deg - 3.0).abs() < 1e-6);
    assert!(estimator.heading_at(2.1).is_none());
    assert!((estimator.heading_at(1.7).unwrap().heading_deg - 18.0).abs() < 1e-9);
}

#[test]
fn opposite_turns_inside_one_sweep_close_it_at_the_turn() {
    let walk = Walk::new(10.0)
        .hold(1.0)
        .turn(270.0, 90.0)
        .turn(-270.0, 90.0)
        .hold(1.5);
    let mut estimator = SweepEstimator::new(SweepConfig::default());
    let results = sweep(&mut estimator, &walk, &Run::default());
    assert_eq!(results.len(), 2, "{results:?}");
    let rotations: Vec<_> = results
        .iter()
        .map(|result| result.as_ref().map(|bearing| bearing.rotation))
        .collect();
    assert_eq!(
        rotations,
        vec![Ok(Rotation::Clockwise), Ok(Rotation::Counterclockwise)]
    );
}
