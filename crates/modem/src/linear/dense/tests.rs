use num_complex::Complex;
use sdrmm_dsp::farrow;
use sdrmm_modem_test_support::ber::perf::assert_no_alloc;

use super::*;
use crate::{
    constellation::tables,
    linear::{LinearMod, LinearParams},
    pulse::{self, Norm},
    testutil::{add_noise, draws},
};

const SPS: usize = 16;

fn rrc() -> Vec<f32> {
    rrc_spanning(16)
}

fn rrc_spanning(span: usize) -> Vec<f32> {
    pulse::root_raised_cosine(SPS as f64, 0.35, span, Norm::Energy)
}

struct Link {
    clock_ppm: f64,
    cycles_per_symbol: f64,
    phase: f64,
    delay_samples: f64,
    gain: f32,
    echo: Option<(f32, f64)>,
    mer_db: Option<f64>,
}

const CLEAN: Link = Link {
    clock_ppm: 0.0,
    cycles_per_symbol: 0.0,
    phase: 0.0,
    delay_samples: 0.0,
    gain: 1.0,
    echo: None,
    mer_db: None,
};

/// `wave` later by `samples`, read on a clock `ppm` parts per million fast.
fn retimed(wave: &[Complex<f32>], samples: f64, ppm: f64) -> Vec<Complex<f32>> {
    let rate = 1.0 + ppm * 1e-6;
    let count = ((wave.len() as f64 - 3.0 + samples) / rate).max(0.0) as usize;
    (0..count)
        .map(|n| {
            let t = n as f64 * rate - samples;
            if t < 1.0 {
                return Complex::new(0.0, 0.0);
            }
            let i = t.floor() as usize;
            farrow(&wave[i - 1..i + 3], (t - t.floor()) as f32)
        })
        .collect()
}

/// A transmission of `labels` from `table` through `link`, shaped by `pulse`.
fn received_with(
    table: &Constellation,
    labels: &[u32],
    link: &Link,
    pulse: Vec<f32>,
) -> Vec<Complex<f32>> {
    let params = LinearParams::new(table.clone(), pulse, SPS).unwrap();
    let clean = LinearMod::transmission(&params, labels);
    let mut wave = retimed(&clean, link.delay_samples, link.clock_ppm);
    if let Some((level, lag)) = link.echo {
        let echo = retimed(&clean, link.delay_samples + lag, link.clock_ppm);
        for (s, e) in wave.iter_mut().zip(echo) {
            *s += e * level;
        }
    }
    for (n, s) in wave.iter_mut().enumerate() {
        let turn = std::f64::consts::TAU * link.cycles_per_symbol * n as f64 / SPS as f64;
        *s *= Complex::from_polar(link.gain, (link.phase + turn) as f32);
    }
    if let Some(mer) = link.mer_db {
        let power = f64::from(link.gain * link.gain) / SPS as f64;
        add_noise(&mut wave, 0x5eed, power * 10f64.powf(-mer / 10.0));
    }
    wave
}

fn received(table: &Constellation, labels: &[u32], link: &Link) -> Vec<Complex<f32>> {
    received_with(table, labels, link, rrc())
}

fn demodulate(table: &Constellation, wave: &[Complex<f32>], chunk: usize) -> Vec<Complex<f32>> {
    demodulate_with(table, wave, chunk, &rrc())
}

fn demodulate_with(
    table: &Constellation,
    wave: &[Complex<f32>],
    chunk: usize,
    filter: &[f32],
) -> Vec<Complex<f32>> {
    let mut demod =
        DenseQamDemod::new(table.clone(), filter, SPS as f64, DEFAULT_EQUALISER_TAPS).unwrap();
    let mut out = Vec::new();
    for piece in wave.chunks(chunk) {
        demod.process(piece, &mut out);
    }
    out
}

/// Symbol errors over the last `tail` symbols, at the alignment and quarter turn that fit best.
fn tail_errors(table: &Constellation, labels: &[u32], got: &[Complex<f32>], tail: usize) -> usize {
    let rotations = [
        Complex::new(1.0, 0.0),
        Complex::new(0.0, 1.0),
        Complex::new(-1.0, 0.0),
        Complex::new(0.0, -1.0),
    ];
    let from = got.len() - tail;
    (0..128)
        .flat_map(|lag| rotations.iter().map(move |&r| (lag, r)))
        .filter_map(|(lag, r)| {
            let sent = labels.get(from.checked_sub(lag)?..(from + tail).checked_sub(lag)?)?;
            Some(
                got[from..]
                    .iter()
                    .zip(sent)
                    .filter(|&(&y, &want)| table.hard_slice(y * r) != want)
                    .count(),
            )
        })
        .min()
        .unwrap_or(tail)
}

fn link_test(order: u32, link: &Link, symbols: usize, tail: usize) -> usize {
    let table = tables::qam_square(order).unwrap();
    let labels: Vec<u32> = draws(symbols, order as usize, 0x0d3e ^ u64::from(order));
    let wave = received(&table, &labels, link);
    let got = demodulate(&table, &wave, 4096);
    tail_errors(&table, &labels, &got, tail)
}

#[test]
fn a_65536_point_grid_comes_out_clean_through_every_offset() {
    let link = Link {
        clock_ppm: -20.0,
        cycles_per_symbol: 3e-3,
        phase: 2.1,
        delay_samples: 5.37,
        gain: 0.6,
        echo: Some((10f32.powf(-56.0 / 20.0), 1.6 * SPS as f64)),
        mer_db: Some(62.0),
    };
    assert_eq!(link_test(65_536, &link, 70_000, 10_000), 0);
}

#[test]
fn a_clock_fifty_ppm_off_is_followed() {
    let link = Link {
        clock_ppm: 50.0,
        cycles_per_symbol: 1e-3,
        delay_samples: 9.1,
        mer_db: Some(62.0),
        ..CLEAN
    };
    assert_eq!(link_test(65_536, &link, 70_000, 10_000), 0);
}

#[test]
fn a_million_point_grid_comes_out_clean() {
    let order = 1 << 20;
    let table = tables::qam_square(order).unwrap();
    let labels: Vec<u32> = draws(200_000, order as usize, 0x1e6);
    let link = Link {
        cycles_per_symbol: 1e-3,
        phase: 0.7,
        delay_samples: 6.3,
        ..CLEAN
    };
    let wave = received_with(&table, &labels, &link, rrc_spanning(32));
    let got = demodulate_with(&table, &wave, 4096, &rrc_spanning(32));
    assert_eq!(tail_errors(&table, &labels, &got, 20_000), 0);
}

#[test]
fn lower_orders_come_out_clean_on_the_same_path() {
    for order in [256, 4096] {
        let link = Link {
            cycles_per_symbol: -2e-3,
            phase: 0.4,
            delay_samples: 3.9,
            mer_db: Some(50.0),
            ..CLEAN
        };
        assert_eq!(link_test(order, &link, 30_000, 5_000), 0, "{order}-QAM");
    }
}

#[test]
fn any_block_split_gives_the_same_symbols() {
    let table = tables::qam_square(4096).unwrap();
    let labels: Vec<u32> = draws(8_000, 4096, 0xb10c);
    let wave = received(
        &table,
        &labels,
        &Link {
            delay_samples: 2.5,
            ..CLEAN
        },
    );
    let whole = demodulate(&table, &wave, 1 << 20);
    let split = demodulate(&table, &wave, 997);
    assert_eq!(whole.len(), split.len());
    let first = whole.iter().zip(&split).position(|(a, b)| a != b);
    assert_eq!(first, None, "symbols part at index {first:?}");
}

#[test]
fn tables_off_a_square_grid_are_refused() {
    for table in [tables::psk(8).unwrap(), tables::qam_cross(32).unwrap()] {
        assert!(matches!(
            DenseQamDemod::new(table, &rrc(), SPS as f64, 0),
            Err(DenseError::NotSquareGrid)
        ));
    }
    let table = tables::qam_square(16).unwrap();
    assert!(matches!(
        DenseQamDemod::new(table, &rrc(), 2.0, 0),
        Err(DenseError::TooFewSamplesPerSymbol)
    ));
}

#[test]
fn tracking_allocates_nothing() {
    let table = tables::qam_square(65_536).unwrap();
    let labels: Vec<u32> = draws(20_000, 65_536, 0xa110);
    let wave = received(&table, &labels, &CLEAN);
    let mut demod = DenseQamDemod::new(table, &rrc(), SPS as f64, DEFAULT_EQUALISER_TAPS).unwrap();
    let mut out = Vec::with_capacity(2 * labels.len());
    let chunk = 4096;
    let (opening, rest) = wave.split_at(wave.len() - 4 * chunk);
    for piece in opening.chunks(chunk) {
        demod.process(piece, &mut out);
    }
    let mut pieces = rest.chunks(chunk);
    assert_no_alloc("DenseQamDemod::process", || {
        for piece in pieces.by_ref() {
            demod.process(piece, &mut out);
        }
    });
}
