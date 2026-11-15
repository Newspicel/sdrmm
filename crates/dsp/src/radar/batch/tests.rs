use std::f64::consts::TAU;

use num_complex::Complex;

use super::*;
use crate::array::manifold::{Direction, Geometry};
use crate::math::db::Decibels;
use crate::math::special::sinc;
use crate::radar::RadarDspError;
use crate::radar::fixtures::{correlate, run_caf};
use crate::scene::{ArrayScene, SceneEcho, SceneSignal, SceneSource};
use crate::testutil::complex_noise;

type C32 = Complex<f32>;

#[test]
fn batch_correlation_equals_direct_sum_at_every_gate() {
    let shape = BatchShape::new(32, 100, 40, 0, 0, 1).unwrap();
    let lanes = vec![
        complex_noise(shape.window(), 1.0, 3),
        complex_noise(shape.window(), 1.0, 4),
    ];
    let caf = run_caf(shape, None, &lanes, DopplerTaper::Rectangular);
    let mut worst = 0.0f32;
    let mut scale = 0.0f32;
    for b in 0..shape.batches {
        for tau in 0..shape.gates {
            let direct = correlate(&shape, &lanes[0], &lanes[1], b, tau as i64);
            worst = worst.max((caf.gate(&shape, 0, b, tau) - direct).norm());
            scale = scale.max(direct.norm());
        }
    }
    assert!(worst < 1e-4 * scale, "{worst} {scale}");
}

fn two_element_scene(fs: f64, echo: SceneEcho) -> Vec<Vec<C32>> {
    let geometry = Geometry::ula(0.5, 2, 90.0).unwrap();
    let mut scene = ArrayScene::new(geometry, 100e6, fs)
        .with_source(SceneSource::new(
            Direction::horizon(0.0),
            0.0,
            SceneSignal::Broadband,
        ))
        .with_echo(echo)
        .with_noise_db(-30.0)
        .with_seed(11);
    scene.render(20_000).unwrap()
}

#[test]
fn an_echo_lands_on_its_delay_and_doppler() {
    let fs = 200e3;
    let shape = BatchShape::new(64, 250, 40, 0, 0, 1).unwrap();
    let echo = SceneEcho::new(0, Direction::horizon(60.0), -20.0, 23.0, 37.5);
    let lanes = two_element_scene(fs, echo);
    let caf = run_caf(shape, None, &lanes, DopplerTaper::Hann);
    assert_eq!(caf.peak(&shape, 0, 3), (23, 32 + 3));
    assert_eq!(caf.peak(&shape, 0, 0), (0, 32));
}

#[test]
fn closing_targets_have_positive_doppler() {
    let fs = 200e3;
    let shape = BatchShape::new(64, 250, 40, 0, 0, 1).unwrap();
    for (rate, above) in [(-90.0, true), (90.0, false)] {
        let echo = SceneEcho::bistatic(
            0,
            Direction::horizon(60.0),
            -20.0,
            30_000.0,
            rate,
            fs,
            100e6,
        );
        let lanes = two_element_scene(fs, echo);
        let caf = run_caf(shape, None, &lanes, DopplerTaper::Hann);
        let (tau, row) = caf.peak(&shape, 0, 5);
        assert_eq!(tau, 20);
        assert_eq!(row > 32, above, "{rate}: {row}");
        assert_eq!(row.abs_diff(32), 2);
    }
}

fn shifted_copy(reference: &[C32], delay: usize, doppler: f64, fs: f64) -> Vec<C32> {
    (0..reference.len())
        .map(|n| {
            let source = if n >= delay {
                reference[n - delay]
            } else {
                C32::default()
            };
            source * C32::from_polar(1.0, (TAU * doppler * n as f64 / fs) as f32)
        })
        .collect()
}

#[test]
fn straddle_loss_follows_sinc() {
    let fs = 100e3;
    let shape = BatchShape::new(64, 100, 16, 0, 0, 1).unwrap();
    let reference = complex_noise(shape.window() + 64, 1.0, 5);
    let doppler = shape.batches as f64 / (4.0 * shape.samples() as f64 / fs);
    let still = run_caf(
        shape,
        None,
        &[reference.clone(), shifted_copy(&reference, 7, 0.0, fs)],
        DopplerTaper::Rectangular,
    );
    let moving = run_caf(
        shape,
        None,
        &[reference.clone(), shifted_copy(&reference, 7, doppler, fs)],
        DopplerTaper::Rectangular,
    );
    let row = 32 + shape.batches / 4;
    assert_eq!(moving.peak(&shape, 0, 0), (7, row));
    let loss = (moving.cell(&shape, 0, 7, row).norm() / still.cell(&shape, 0, 7, 32).norm())
        .amplitude_to_db();
    let expected = (sinc(0.25) as f32).amplitude_to_db();
    assert!((loss - expected).abs() < 0.3, "{loss} vs {expected}");
}

#[test]
fn hann_sidelobes_stay_below_31_db() {
    let shape = BatchShape::new(64, 16, 4, 0, 0, 1).unwrap();
    let mut kernel = BatchKernel::new(shape);
    let mut window = vec![0.0f32; 64];
    DopplerTaper::Hann.fill(&mut window);
    for offset in [5.4f64, 11.25, -20.4] {
        let mut series: Vec<C32> = (0..64)
            .map(|b| C32::from_polar(1.0, (TAU * offset * b as f64 / 64.0) as f32))
            .collect();
        kernel.doppler(&mut series, &window).unwrap();
        let (peak, top) = series
            .iter()
            .enumerate()
            .map(|(row, value)| (row, value.norm()))
            .fold(
                (0, 0.0f32),
                |best, next| if next.1 > best.1 { next } else { best },
            );
        assert_eq!(peak, (32.0 + offset).round() as usize);
        let sidelobe = series
            .iter()
            .enumerate()
            .filter(|(row, _)| row.abs_diff(peak) > 2)
            .map(|(_, value)| value.norm())
            .fold(0.0f32, f32::max);
        assert!((sidelobe / top).amplitude_to_db() < -31.0, "{offset}");
    }
    assert_eq!(DopplerTaper::Hann.enbw(), 1.5);
    let mut flat = vec![0.0f32; 8];
    DopplerTaper::Rectangular.fill(&mut flat);
    assert!(flat.iter().all(|&w| w == 1.0));
    let mut harris = vec![0.0f32; 64];
    DopplerTaper::BlackmanHarris.fill(&mut harris);
    let enbw =
        64.0 * harris.iter().map(|w| w * w).sum::<f32>() / harris.iter().sum::<f32>().powi(2);
    assert!((f64::from(enbw) - DopplerTaper::BlackmanHarris.enbw()).abs() < 0.01);
}

fn inverse(kernel: &mut BatchKernel, spectrum: &[C32]) -> Vec<C32> {
    let mut buffer = spectrum.to_vec();
    kernel.fft.inverse_scaled(&mut buffer);
    buffer
}

#[test]
fn sizes_cover_every_lag_without_aliasing() {
    let mut seed = 17;
    let mut checked = 0;
    for lead in 0..=3usize {
        for taps in 0..=5usize {
            if lead > 0 && taps == 0 {
                assert!(BatchShape::new(2, 4, 4, lead, taps, 1).is_err());
                continue;
            }
            for gates in [1usize, 2, 5, 9] {
                for batch_len in [1usize, 3, 8] {
                    let shape = BatchShape::new(3, batch_len, gates, lead, taps, 1).unwrap();
                    assert!(shape.fft_len.is_power_of_two() && shape.fft_len >= MIN_FFT);
                    seed += 2;
                    let reference = complex_noise(shape.window(), 1.0, seed);
                    let surveillance = complex_noise(shape.window(), 1.0, seed + 1);
                    let mut kernel = BatchKernel::new(shape);
                    let m = shape.fft_len;
                    let (mut spectrum, mut model, mut product) = (
                        vec![C32::default(); m],
                        vec![C32::default(); m],
                        vec![C32::default(); m],
                    );
                    for b in 0..shape.batches {
                        kernel
                            .reference(&reference, b, &mut spectrum, &mut model)
                            .unwrap();
                        kernel
                            .surveillance(&surveillance, b, &spectrum, &mut product)
                            .unwrap();
                        let cross = inverse(&mut kernel, &product);
                        for (index, value) in cross.iter().enumerate().take(shape.span()) {
                            let direct = correlate(
                                &shape,
                                &reference,
                                &surveillance,
                                b,
                                index as i64 - lead as i64,
                            );
                            assert!((value - direct).norm() < 1e-4 * (1.0 + direct.norm()));
                        }
                        if shape.eca() {
                            let auto = inverse(&mut kernel, &model);
                            let used = shape.span() + shape.order() - 1;
                            for (index, value) in auto.iter().enumerate().take(used) {
                                let lag = index as i64 - (shape.order() as i64 - 1);
                                let direct = correlate(&shape, &reference, &reference, b, lag);
                                assert!((value - direct).norm() < 1e-4 * (1.0 + direct.norm()));
                            }
                        }
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 100);
}

#[test]
fn worked_example_shapes_match_the_plan() {
    let fm = BatchShape::new(512, 260, 73, 2, 14, 4).unwrap();
    assert_eq!((fm.span(), fm.fft_len), (75, 512));
    let dab = BatchShape::new(1024, 1000, 548, 2, 103, 4).unwrap();
    assert_eq!((dab.span(), dab.fft_len), (550, 2048));
    let plain = BatchShape::new(512, 260, 73, 0, 0, 4).unwrap();
    assert_eq!((plain.pre(), plain.post(), plain.span()), (0, 72, 73));
    assert_eq!(fm.window(), 512 * 260 + 15 + 74);
    assert_eq!(
        BatchShape::new(4, 1 << 20, 4, 0, 0, 1),
        Err(RadarDspError::Size)
    );
    assert_eq!(
        BatchShape::new(4, 16, 4, 0, 0, 16),
        Err(RadarDspError::Shape)
    );
}

#[test]
fn mismatched_buffers_are_refused() {
    let shape = BatchShape::new(4, 16, 4, 1, 2, 1).unwrap();
    let mut kernel = BatchKernel::new(shape);
    let window = vec![C32::default(); shape.window() - 1];
    let mut buffer = vec![C32::default(); shape.fft_len];
    let mut model = vec![C32::default(); shape.fft_len];
    assert_eq!(
        kernel.reference(&window, 0, &mut buffer, &mut model),
        Err(RadarDspError::Shape)
    );
    let window = vec![C32::default(); shape.window()];
    assert_eq!(
        kernel.reference(&window, 4, &mut buffer, &mut model),
        Err(RadarDspError::Shape)
    );
    let mut gates = vec![C32::default(); 3];
    assert_eq!(
        kernel.residual(&buffer, &model, WeightsAt::none(), &mut gates),
        Err(RadarDspError::Shape)
    );
    let mut series = vec![C32::default(); 3];
    assert_eq!(
        kernel.doppler(&mut series, &[1.0; 4]),
        Err(RadarDspError::Shape)
    );
}
