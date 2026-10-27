use std::f64::consts::TAU;

use super::*;
use crate::manifold::Winding;
use crate::scene::wave::History;

const FREQ: f64 = 433.92e6;

fn kraken() -> Geometry {
    Geometry::uca(0.35, 5, 0.0, Winding::Clockwise).unwrap()
}

fn noisy_scene(seed: u64) -> ArrayScene {
    ArrayScene::new(kraken(), FREQ, 2.4e6)
        .with_source(SceneSource::new(
            Direction::horizon(137.0),
            0.0,
            SceneSignal::Noise {
                offset_hz: 1e5,
                bandwidth_hz: 2e5,
            },
        ))
        .with_source(SceneSource {
            delay_samples: 3.4,
            ..SceneSource::new(
                Direction::horizon(20.0),
                -6.0,
                SceneSignal::Fm {
                    offset_hz: -3e5,
                    deviation_hz: 5e3,
                    rate_hz: 1e3,
                },
            )
        })
        .with_noise_db(-20.0)
        .with_seed(seed)
}

#[test]
fn scene_is_reproducible_from_its_seed() {
    let mut first = noisy_scene(5);
    let mut second = noisy_scene(5);
    second.lane_delay_samples = vec![0.0; 5];
    let whole = first.render(3000).unwrap();
    let mut pieces = vec![Vec::new(); 5];
    for len in [1000, 1, 1999] {
        for (lane, block) in second.render(len).unwrap().into_iter().enumerate() {
            pieces[lane].extend(block);
        }
    }
    assert_eq!(whole, pieces);
    assert_eq!(second.rendered(), 3000);
    let other = noisy_scene(6).render(3000).unwrap();
    assert_ne!(whole, other);
}

fn inner(a: &[Complex<f32>], b: &[Complex<f32>]) -> f32 {
    let dot: Complex<f32> = a.iter().zip(b).map(|(x, y)| x.conj() * y).sum();
    let norms =
        a.iter().map(Complex::norm_sqr).sum::<f32>() * b.iter().map(Complex::norm_sqr).sum::<f32>();
    dot.norm() / norms.sqrt()
}

fn snapshot(lanes: &[Vec<Complex<f32>>], index: usize) -> Vec<Complex<f32>> {
    lanes.iter().map(|lane| lane[index]).collect()
}

#[test]
fn noise_fm_keeps_its_envelope_and_deviation() {
    let fs = 250e3;
    let deviation = 25e3;
    let signal = SceneSignal::NoiseFm {
        offset_hz: 0.0,
        deviation_hz: deviation,
        bandwidth_hz: 15e3,
    };
    let mut scene = ArrayScene::new(kraken(), FREQ, fs).with_source(SceneSource::new(
        Direction::horizon(0.0),
        0.0,
        signal,
    ));
    let lanes = scene.render(100_000).unwrap();
    assert!(lanes[0].iter().all(|x| (x.norm() - 1.0).abs() < 1e-4));
    let squares: f64 = lanes[0]
        .windows(2)
        .map(|pair| {
            let hz = f64::from((pair[1] * pair[0].conj()).arg()) * fs / TAU;
            hz * hz
        })
        .sum();
    let rms = (squares / (lanes[0].len() - 1) as f64).sqrt();
    assert!((rms / deviation - 1.0).abs() < 0.1, "{rms}");
}

#[test]
fn scene_single_source_matches_the_steering_vector() {
    let offset = 2.5e4;
    let mut scene = ArrayScene::new(kraken(), FREQ, 1e6).with_source(SceneSource::new(
        Direction::new(222.0, 10.0),
        6.0,
        SceneSignal::Tone { offset_hz: offset },
    ));
    let lanes = scene.render(64).unwrap();
    let mut expected = [Complex::new(0.0f32, 0.0); 5];
    steer(
        kraken().positions(),
        FREQ + offset,
        Direction::new(222.0, 10.0),
        &mut expected,
    );
    for index in [0, 17, 63] {
        let x = snapshot(&lanes, index);
        let ratio = x[0] / expected[0];
        for (value, want) in x.iter().zip(&expected) {
            assert!((value - want * ratio).norm() < 1e-4, "{index}");
        }
        assert!((ratio.norm() - 10f32.powf(0.3)).abs() < 1e-3);
    }
    let step = snapshot(&lanes, 1)[0] / snapshot(&lanes, 0)[0];
    assert!((f64::from(step.arg()) - TAU * offset / 1e6).abs() < 1e-5);
}

#[test]
fn scene_rotating_heading_moves_the_body_bearing() {
    let fs = 1e3;
    let mut scene = ArrayScene::new(kraken(), FREQ, fs)
        .with_source(SceneSource::new(
            Direction::horizon(30.0),
            0.0,
            SceneSignal::Tone { offset_hz: 0.0 },
        ))
        .with_heading(HeadingTrack::Rotating {
            start_deg: 0.0,
            rate_dps: 90.0,
        });
    let lanes = scene.render(1001).unwrap();
    let mut body = [Complex::new(0.0f32, 0.0); 5];
    let mut fixed = [Complex::new(0.0f32, 0.0); 5];
    steer(
        kraken().positions(),
        FREQ,
        Direction::horizon(30.0),
        &mut fixed,
    );
    for (index, body_deg) in [(0, 30.0), (500, -15.0), (1000, -60.0)] {
        steer(
            kraken().positions(),
            FREQ,
            Direction::horizon(body_deg),
            &mut body,
        );
        let x = snapshot(&lanes, index);
        assert!(inner(&body, &x) > 0.9999, "{index}");
        if index > 0 {
            assert!(inner(&fixed, &x) < 0.99, "{index}");
        }
    }
}

#[test]
fn a_fractional_lane_delay_shifts_the_signal() {
    let geometry = Geometry::ula(0.01, 2, 90.0).unwrap();
    let mut scene = ArrayScene::new(geometry, 1e6, 1e5)
        .with_source(SceneSource::new(
            Direction::horizon(0.0),
            0.0,
            SceneSignal::Noise {
                offset_hz: 0.0,
                bandwidth_hz: 3e4,
            },
        ))
        .with_seed(3);
    scene.lane_delay_samples = vec![0.0, 1.37];
    let lanes = scene.render(20_000).unwrap();
    let mut best = (0.0f64, 0.0f32);
    for step in 0..=300 {
        let lag = 1.0 + f64::from(step) * 0.002;
        let plan = DelayPlan::new(lag);
        let history = History {
            base: 0,
            data: lanes[0].iter().map(|&x| widen(x)).collect(),
        };
        let score: Complex<f64> = (200..19_000)
            .map(|n| plan.read(&history, n).conj() * widen(lanes[1][n as usize]))
            .sum();
        let score = score.norm() as f32;
        if score > best.1 {
            best = (lag, score);
        }
    }
    assert!((best.0 - 1.37).abs() < 0.01, "{best:?}");
}

#[test]
fn copies_dc_gain_and_distortion_shape_the_lanes() {
    fn tilt(element: usize, _azimuth: f64) -> Complex<f32> {
        Complex::from_polar(1.0, element as f32 * 0.1)
    }
    let geometry = Geometry::ula(0.3, 2, 90.0).unwrap();
    let mut scene = ArrayScene::new(geometry, 1e8, 1e4).with_source(SceneSource::new(
        Direction::horizon(0.0),
        0.0,
        SceneSignal::Tone { offset_hz: 100.0 },
    ));
    scene.lane_gain_db = vec![0.0, 6.0];
    scene.lane_phase_deg = vec![0.0, 90.0];
    scene.lane_dc = vec![Complex::new(0.5, 0.0), Complex::new(0.0, 0.0)];
    scene.distortion = Some(tilt);
    let lanes = scene.render(100).unwrap();
    let mean: Complex<f32> = lanes[0].iter().sum::<Complex<f32>>() / 100.0;
    assert!((mean - Complex::new(0.5, 0.0)).norm() < 0.05);
    let ratio = lanes[1][10] / (lanes[0][10] - Complex::new(0.5, 0.0));
    assert!((ratio.norm() - 10f32.powf(0.3)).abs() < 1e-3);
    assert!((ratio.arg() - (std::f32::consts::FRAC_PI_2 + 0.1)).abs() < 1e-3);
    let table = scene.distortion_table(&[1e8], 10.0).unwrap();
    let mut response = [Complex::new(0.0f32, 0.0); 2];
    table
        .response(1e8, Direction::horizon(0.0), &mut response)
        .unwrap();
    assert!(((response[1] / response[0]).arg() - 0.1).abs() < 1e-4);
}

#[test]
fn invalid_scenes_are_refused() {
    let mut scene = noisy_scene(1);
    scene.lane_gain_db = vec![0.0; 3];
    assert_eq!(
        scene.render(10).unwrap_err(),
        SceneError::LaneSetting("lane_gain_db")
    );
    let mut scene = noisy_scene(1);
    scene.sources[1].copy_of = Some(SceneCopy {
        source: 7,
        amplitude: 1.0,
        phase_deg: 0.0,
    });
    assert_eq!(scene.render(10).unwrap_err(), SceneError::Copy(1));
    let mut scene = noisy_scene(1);
    scene.sources[0].signal = SceneSignal::Noise {
        offset_hz: 0.0,
        bandwidth_hz: 0.0,
    };
    assert_eq!(scene.render(10).unwrap_err(), SceneError::Source(0));
    let mut scene = noisy_scene(1);
    scene.sample_rate = 0.0;
    assert_eq!(scene.render(10).unwrap_err(), SceneError::SampleRate);
    let mut scene = noisy_scene(1);
    scene.noise_db[2] = f32::NAN;
    assert_eq!(
        scene.render(10).unwrap_err(),
        SceneError::LaneSetting("noise_db")
    );
    let mut scene = noisy_scene(1).with_heading(HeadingTrack::Fixed(f64::INFINITY));
    assert_eq!(scene.render(10).unwrap_err(), SceneError::Heading);
}

#[test]
fn a_source_power_and_noise_power_are_per_sample() {
    let mut scene = ArrayScene::new(kraken(), FREQ, 1e6)
        .with_source(SceneSource::new(
            Direction::horizon(0.0),
            -3.0,
            SceneSignal::Broadband,
        ))
        .with_seed(2);
    let lanes = scene.render(40_000).unwrap();
    let power = lanes[2].iter().map(Complex::norm_sqr).sum::<f32>() / 40_000.0;
    assert!((10.0 * power.log10() + 3.0).abs() < 0.2, "{power}");
    let mut quiet = ArrayScene::new(kraken(), FREQ, 1e6).with_noise_db(-10.0);
    let lanes = quiet.render(40_000).unwrap();
    let power = lanes[4].iter().map(Complex::norm_sqr).sum::<f32>() / 40_000.0;
    assert!((10.0 * power.log10() + 10.0).abs() < 0.2, "{power}");
}
