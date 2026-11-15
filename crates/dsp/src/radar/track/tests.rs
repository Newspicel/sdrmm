use super::*;

const LAMBDA: f64 = 2.997_924_58;
const DT: f64 = 0.5;

fn config() -> TrackerConfig {
    TrackerConfig {
        wavelength_m: LAMBDA,
        confirm_hits: 3,
        confirm_window: 5,
        coast_looks: 10,
        max_accel: 30.0,
        gate: 11.8,
        jerk: 5.0,
        range_resolution_m: 1124.22,
        doppler_resolution_hz: 3.0,
    }
}

fn measure(range_m: f64, rate_mps: f64) -> Measurement {
    Measurement {
        range_m,
        doppler_hz: -rate_mps / LAMBDA,
        snr: 100.0,
        aoa: None,
    }
}

fn target(look: u32, range_m: f64, rate_mps: f64) -> Measurement {
    measure(range_m + rate_mps * DT * f64::from(look), rate_mps)
}

struct Run {
    tracker: Tracker,
    assigned: Vec<Option<u32>>,
    ended: Vec<u32>,
}

impl Run {
    fn new(config: TrackerConfig) -> Self {
        Self {
            tracker: Tracker::new(config).unwrap(),
            assigned: Vec::new(),
            ended: Vec::new(),
        }
    }

    fn step(&mut self, measurements: &[Measurement]) -> TrackerStats {
        self.tracker
            .update(DT, measurements, &mut self.assigned, &mut self.ended)
    }

    fn views(&self) -> Vec<TrackView> {
        let mut views = Vec::new();
        self.tracker.for_each_confirmed(|view| views.push(*view));
        views
    }
}

#[test]
fn three_of_five_confirms_and_names() {
    let mut run = Run::new(config());
    for look in 0..2 {
        run.step(&[target(look, 20_000.0, -100.0)]);
        assert!(run.views().is_empty());
        assert_eq!(run.assigned, vec![None]);
    }
    run.step(&[target(2, 20_000.0, -100.0)]);
    let views = run.views();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].id, 1);
    assert_eq!(views[0].looks, 3);
    assert_eq!(run.assigned, vec![Some(1)]);
    assert!((views[0].range_rate_mps + 100.0).abs() < 2.0);
    run.step(&[]);
    run.step(&[target(4, 20_000.0, -100.0)]);
    let views = run.views();
    assert_eq!(views[0].id, 1);
    assert_eq!(views[0].trail_len, 5);
    assert!((views[0].trail[4].0 - 19.8).abs() < 0.05);
}

#[test]
fn tentatives_do_not_consume_ids() {
    let mut run = Run::new(config());
    for look in 0..6u32 {
        let clutter: Vec<Measurement> = (0..5)
            .map(|k| {
                let spread = f64::from(look * 5 + k);
                measure(3_000.0 + 7_000.0 * spread, 150.0 - 11.0 * spread)
            })
            .collect();
        run.step(&clutter);
        assert!(run.views().is_empty());
    }
    for look in 0..3 {
        run.step(&[target(look, 50_000.0, 80.0)]);
    }
    assert_eq!(run.views()[0].id, 1);
}

#[test]
fn range_rate_comes_from_doppler_on_the_first_look() {
    let mut run = Run::new(TrackerConfig {
        confirm_hits: 1,
        confirm_window: 1,
        ..config()
    });
    run.step(&[measure(30_000.0, 0.0), measure(40_000.0, 0.0)]);
    let mut measurement = measure(12_345.0, 0.0);
    measurement.doppler_hz = 40.0;
    run.step(&[measurement]);
    let views = run.views();
    let view = views.iter().find(|view| view.id == 3).unwrap();
    assert!((view.range_rate_mps + LAMBDA * 40.0).abs() < 1e-9);
    assert!((view.doppler_hz - 40.0).abs() < 1e-9);
    assert!((view.range_m - 12_345.0).abs() < 1e-9);
}

#[test]
fn a_turn_within_max_accel_is_followed() {
    let mut run = Run::new(config());
    let accel = 20.0;
    for look in 0..30u32 {
        let t = DT * f64::from(look);
        let rate = -150.0 + accel * t;
        let range = 40_000.0 - 150.0 * t + 0.5 * accel * t * t;
        run.step(&[measure(range, rate)]);
        if look >= 2 {
            let views = run.views();
            assert_eq!(views.len(), 1, "look {look}");
            assert_eq!(views[0].id, 1);
            assert_eq!(views[0].misses, 0, "look {look}");
        }
    }
    let view = run.views()[0];
    assert!((view.accel_mps2 - accel).abs() < 5.0, "{}", view.accel_mps2);
}

#[test]
fn an_impossible_doppler_jump_starts_no_track() {
    let jumping = |look: u32| Measurement {
        doppler_hz: 7.0 * f64::from(look),
        ..measure(20_000.0, 0.0)
    };
    let mut run = Run::new(config());
    for look in 0..12u32 {
        run.step(&[jumping(look)]);
    }
    assert!(run.views().is_empty());
    let mut agile = Run::new(TrackerConfig {
        max_accel: 200.0,
        ..config()
    });
    for look in 0..3u32 {
        agile.step(&[jumping(look)]);
    }
    assert_eq!(agile.views().len(), 1);
}

#[test]
fn coasting_tracks_are_reported_then_ended() {
    let mut run = Run::new(TrackerConfig {
        coast_looks: 3,
        ..config()
    });
    for look in 0..3 {
        run.step(&[target(look, 20_000.0, 50.0)]);
    }
    for miss in 1..=3u32 {
        run.step(&[]);
        let views = run.views();
        assert_eq!(views.len(), 1);
        assert!(views[0].coasting);
        assert_eq!(views[0].misses, miss);
        assert!(run.ended.is_empty());
    }
    run.step(&[]);
    assert!(run.views().is_empty());
    assert_eq!(run.ended, vec![1]);
}

#[test]
fn crossing_targets_keep_their_ids() {
    let mut run = Run::new(config());
    let mut opener_id = None;
    for look in 0..40u32 {
        let opening = target(look, 30_000.0, 120.0);
        let closing = target(look, 34_000.0, -120.0);
        run.step(&[closing, opening]);
        if look == 2 {
            opener_id = run.assigned[1];
        }
        if look >= 2 {
            let views = run.views();
            assert_eq!(views.len(), 2, "look {look}");
            assert_eq!(run.assigned[1], opener_id, "look {look}");
            let opener = views.iter().find(|view| Some(view.id) == opener_id);
            assert!(opener.is_some_and(|view| view.range_rate_mps > 100.0));
        }
    }
}

#[test]
fn ended_ids_are_listed_once() {
    let mut run = Run::new(TrackerConfig {
        coast_looks: 0,
        ..config()
    });
    for look in 0..3 {
        run.step(&[target(look, 20_000.0, 50.0)]);
    }
    run.step(&[]);
    assert_eq!(run.ended, vec![1]);
    for _ in 0..5 {
        run.step(&[]);
        assert!(run.ended.is_empty());
    }
}

#[test]
fn reset_ends_every_confirmed_track() {
    let mut run = Run::new(config());
    for look in 0..3 {
        run.step(&[target(look, 20_000.0, 50.0), target(look, 60_000.0, -70.0)]);
    }
    assert_eq!(run.views().len(), 2);
    let mut ended = Vec::new();
    run.tracker.reset(&mut ended);
    ended.sort_unstable();
    assert_eq!(ended, vec![1, 2]);
    assert!(run.views().is_empty());
    for look in 0..3 {
        run.step(&[target(look, 20_000.0, 50.0)]);
    }
    assert_eq!(run.views()[0].id, 3);
}

#[test]
fn resumed_ids_continue_after_a_rebuild() {
    let mut first = Run::new(config());
    for look in 0..3 {
        first.step(&[target(look, 20_000.0, 50.0)]);
    }
    assert_eq!(first.tracker.next_id(), 2);
    let mut second = Run::new(config());
    second.tracker.resume_ids(first.tracker.next_id());
    second.tracker.resume_ids(1);
    for look in 0..3 {
        second.step(&[target(look, 30_000.0, 40.0)]);
    }
    assert_eq!(second.views()[0].id, 2);
}

#[test]
fn too_many_tentatives_drop_the_weakest_and_count() {
    let mut run = Run::new(config());
    let flood: Vec<Measurement> = (0..MAX_MEASUREMENTS + 10)
        .map(|k| Measurement {
            snr: 10.0 + k as f64,
            ..measure(1_000.0 + 3_000.0 * k as f64, 0.0)
        })
        .collect();
    let stats = run.step(&flood);
    assert_eq!(stats.ignored, 10);
    assert_eq!(run.tracker.tentative(), MAX_TENTATIVE);
    let strong = [Measurement {
        snr: 1e6,
        ..measure(900_000.0, 0.0)
    }];
    let stats = run.step(&strong);
    assert_eq!(stats.dropped, 1);
    assert_eq!(run.tracker.tentative(), MAX_TENTATIVE);
}

#[test]
fn aoa_is_a_quality_weighted_circular_mean() {
    let mut run = Run::new(config());
    for (look, azimuth) in [359.0f32, 1.0, 3.0].into_iter().enumerate() {
        let measurement = Measurement {
            aoa: Some(TrackAoa {
                azimuth_deg: azimuth,
                quality: 0.9,
                sigma_deg: 4.0,
            }),
            ..target(look as u32, 20_000.0, 50.0)
        };
        run.step(&[measurement]);
    }
    let aoa = run.views()[0].aoa.unwrap();
    assert!((aoa.azimuth_deg - 1.0).abs() < 1e-3, "{}", aoa.azimuth_deg);
    assert!((aoa.sigma_deg - 4.0 / 3f32.sqrt()).abs() < 1e-4);
}

#[test]
fn unsound_measurements_are_counted_not_tracked() {
    let mut run = Run::new(config());
    let broken = Measurement {
        range_m: f64::NAN,
        ..measure(20_000.0, 50.0)
    };
    let stats = run.step(&[broken, measure(30_000.0, 0.0)]);
    assert_eq!(stats.ignored, 1);
    assert_eq!(run.tracker.tentative(), 1);
    assert_eq!(run.assigned, vec![None, None]);
}

#[test]
fn invalid_configs_are_refused() {
    let over = TrackerConfig {
        confirm_hits: 6,
        ..config()
    };
    assert!(Tracker::new(over).is_err());
    let flat = TrackerConfig {
        wavelength_m: 0.0,
        ..config()
    };
    assert!(Tracker::new(flat).is_err());
    let mut tracker = Tracker::new(config()).unwrap();
    let wide = TrackerConfig {
        confirm_window: 17,
        ..config()
    };
    assert!(tracker.set_config(wide).is_err());
}
