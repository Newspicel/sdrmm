use sdrmm_modem_test_support::ber::{
    impair::{ClockError, Impairment, MovingEcho},
    perf::assert_no_alloc,
    rng::Rng,
};

use sdrmm_dsp::Decibels;

use super::*;
use crate::testutil::draws;
use crate::{
    constellation::tables,
    linear::{
        LinearBurstDemod, LinearDemod, LinearMod, LinearParams, LinearTiming, PhaseDetector,
        testutil::{SPS, rrc},
    },
};

fn echoes() -> Vec<(usize, Complex<f32>)> {
    vec![
        (0, Complex::new(1.0, 0.0)),
        (SPS, Complex::from_polar(0.35, 1.0)),
        (SPS + SPS / 2, Complex::from_polar(0.2, -2.0)),
        (3 * SPS, Complex::from_polar(0.1, 0.5)),
    ]
}

fn smear(wave: &[Complex<f32>], taps: &[(usize, Complex<f32>)]) -> Vec<Complex<f32>> {
    let power: f32 = taps.iter().map(|(_, h)| h.norm_sqr()).sum();
    let scale = power.sqrt().recip();
    (0..wave.len())
        .map(|n| {
            taps.iter()
                .filter_map(|&(d, h)| n.checked_sub(d).map(|i| wave[i] * h))
                .sum::<Complex<f32>>()
                * scale
        })
        .collect()
}

fn add_noise(wave: &mut [Complex<f32>], es_n0_db: f64, seed: u64) {
    let mut rng = Rng::new(seed);
    let sigma = (0.5 / es_n0_db.db_to_power()).sqrt();
    for s in wave {
        let (a, b) = rng.normal_pair();
        *s += Complex::new((a * sigma) as f32, (b * sigma) as f32);
    }
}

const GAIN_BLOCK: usize = 256;

fn symbol_errors(got: &[Complex<f32>], sent: &[u32], table: &Constellation, from: usize) -> usize {
    let points: Vec<Complex<f32>> = sent
        .iter()
        .map(|&l| table.points()[table.index_of(l).unwrap()])
        .collect();
    (0..=64usize)
        .map(|lag| {
            (from..sent.len())
                .step_by(GAIN_BLOCK)
                .map(|start| {
                    let block = start..(start + GAIN_BLOCK).min(sent.len());
                    block_errors(got, &points, sent, table, block, lag)
                })
                .sum::<usize>()
        })
        .min()
        .unwrap_or(usize::MAX)
}

fn block_errors(
    got: &[Complex<f32>],
    points: &[Complex<f32>],
    sent: &[u32],
    table: &Constellation,
    block: std::ops::Range<usize>,
    lag: usize,
) -> usize {
    let pairs = || {
        block
            .clone()
            .filter_map(|k| got.get(k + lag).map(|&y| (y, points[k])))
    };
    let cross: Complex<f32> = pairs().map(|(y, x)| y * x.conj()).sum();
    let power: f32 = pairs().map(|(_, x)| x.norm_sqr()).sum();
    let gain = Complex::new(power, 0.0) / cross;
    block
        .filter(|&k| {
            got.get(k + lag)
                .is_none_or(|&y| table.hard_slice(y * gain) != sent[k])
        })
        .count()
}

fn params(table: &Constellation) -> LinearParams {
    LinearParams::new(table.clone(), rrc(), SPS).unwrap()
}

fn carrier() -> CarrierLoop {
    CarrierLoop::new(PhaseDetector::DecisionDirected, 0.003)
}

fn burst(
    table: &Constellation,
    wave: &[Complex<f32>],
    eq: Option<EqualiserConfig>,
) -> Vec<Complex<f32>> {
    let p = params(table);
    let mut demod = LinearBurstDemod::new(&p, &rrc(), f64::INFINITY, Some(carrier()));
    if let Some(config) = eq {
        demod = demod.with_equaliser(config).unwrap();
    }
    let mut out = Vec::new();
    demod.process(wave, &mut out);
    out
}

fn streamed(
    table: &Constellation,
    wave: &[Complex<f32>],
    eq: Option<EqualiserConfig>,
) -> Vec<Complex<f32>> {
    let p = params(table);
    let mut demod = LinearDemod::new(&p, &rrc(), LinearTiming::CONTINUOUS, Some(carrier()));
    if let Some(config) = eq {
        demod = demod.with_equaliser(config).unwrap();
    }
    let mut out = Vec::new();
    demod.process(wave, &mut out);
    out
}

fn wave_through(
    table: &Constellation,
    sent: &[u32],
    channel: &[(usize, Complex<f32>)],
    es_n0_db: f64,
) -> Vec<Complex<f32>> {
    let mut wave = smear(&LinearMod::transmission(&params(table), sent), channel);
    add_noise(&mut wave, es_n0_db, 0x5eed);
    wave
}

fn cases() -> [(&'static str, Constellation, f64); 3] {
    [
        ("qpsk", tables::qam_square(4).unwrap(), 17.0),
        ("16-qam", tables::qam_square(16).unwrap(), 25.0),
        ("64-qam", tables::qam_square(64).unwrap(), 31.0),
    ]
}

#[test]
fn a_static_echo_is_equalised_on_a_burst() {
    for (name, table, snr) in cases() {
        let sent = draws(8_000, table.len(), 0xe0);
        let wave = wave_through(&table, &sent, &echoes(), snr);
        let tail = &sent[..7_000];
        let plain = symbol_errors(&burst(&table, &wave, None), tail, &table, 1_000);
        let equalised = burst(&table, &wave, Some(EqualiserConfig::DEFAULT));
        let errors = symbol_errors(&equalised, tail, &table, 1_000);
        println!("burst {name}: {errors} equalised, {plain} plain");
        assert_eq!(errors, 0, "{name}: {errors} errors, {plain} without");
    }
}

#[test]
fn the_equalised_burst_keeps_the_plain_alignment() {
    let table = tables::qam_square(16).unwrap();
    let sent = draws(4_000, 16, 0xa1);
    let wave = wave_through(&table, &sent, &[(0, ONE)], 30.0);
    let lag = |out: &[Complex<f32>]| {
        (0..64usize).find(|&l| {
            (100..200).all(|k| {
                out.get(k + l)
                    .is_some_and(|&y| table.hard_slice(y) == sent[k])
            })
        })
    };
    let plain = burst(&table, &wave, None);
    let equalised = burst(&table, &wave, Some(EqualiserConfig::DEFAULT));
    assert!(lag(&plain).is_some());
    assert_eq!(lag(&equalised), lag(&plain));
    assert_eq!(equalised.len(), plain.len());
}

#[test]
fn a_static_echo_is_equalised_while_streaming() {
    for (name, table, snr) in cases() {
        let sent = draws(20_000, table.len(), 0xe1);
        let wave = wave_through(&table, &sent, &echoes(), snr);
        let tail = &sent[..19_000];
        let plain = symbol_errors(&streamed(&table, &wave, None), tail, &table, 10_000);
        let equalised = streamed(&table, &wave, Some(EqualiserConfig::DEFAULT));
        let errors = symbol_errors(&equalised, tail, &table, 10_000);
        println!("stream {name}: {errors} equalised, {plain} plain");
        assert_eq!(errors, 0, "{name}: {errors} errors, {plain} without");
    }
}

#[test]
fn a_moving_echo_is_tracked() {
    let table = tables::qam_square(16).unwrap();
    let sent = draws(40_000, 16, 0xe2);
    let mut wave = LinearMod::transmission(&params(&table), &sent);
    MovingEcho::new(SPS, -8.0, 1e-5).apply(&mut wave, &mut Rng::new(1));
    add_noise(&mut wave, 22.0, 0x6060);
    let tail = &sent[..39_000];
    let plain = symbol_errors(&streamed(&table, &wave, None), tail, &table, 10_000);
    let equalised = streamed(&table, &wave, Some(EqualiserConfig::DEFAULT));
    let errors = symbol_errors(&equalised, tail, &table, 10_000);
    println!("moving echo: {errors} equalised, {plain} plain");
    assert!(errors <= 30, "{errors} errors, {plain} without");
    assert!(plain > 1_000, "the moving echo only cost {plain} errors");
}

#[test]
fn a_clock_offset_is_followed_while_streaming() {
    let table = tables::qam_square(16).unwrap();
    let sent = draws(30_000, 16, 0xe3);
    let mut wave = smear(&LinearMod::transmission(&params(&table), &sent), &echoes());
    ClockError::new(150.0).apply(&mut wave, &mut Rng::new(2));
    add_noise(&mut wave, 22.0, 0x7070);
    let p = params(&table);
    let mut demod = LinearDemod::new(&p, &rrc(), LinearTiming::CONTINUOUS, Some(carrier()))
        .with_equaliser(EqualiserConfig::DEFAULT)
        .unwrap();
    let mut out = Vec::new();
    demod.process(&wave, &mut out);
    let errors = symbol_errors(&out, &sent[..29_000], &table, 15_000);
    assert_eq!(errors, 0);
    assert_eq!(demod.equaliser().map(Equaliser::slips), Some(0));
}

#[test]
fn awgn_alone_costs_next_to_nothing() {
    for (name, table, snr, burst_mode) in [
        ("qpsk burst", tables::qam_square(4).unwrap(), 8.0, true),
        ("16-qam burst", tables::qam_square(16).unwrap(), 15.0, true),
        ("qpsk stream", tables::qam_square(4).unwrap(), 8.0, false),
        (
            "16-qam stream",
            tables::qam_square(16).unwrap(),
            15.0,
            false,
        ),
    ] {
        let sent = draws(20_000, table.len(), 0xa2);
        let wave = wave_through(&table, &sent, &[(0, ONE)], snr);
        let run = |eq| {
            let out = if burst_mode {
                burst(&table, &wave, eq)
            } else {
                streamed(&table, &wave, eq)
            };
            symbol_errors(&out, &sent[..19_000], &table, 4_000)
        };
        let plain = run(None);
        let equalised = run(Some(EqualiserConfig::DEFAULT));
        println!("awgn {name}: {equalised} equalised, {plain} plain");
        assert!(plain > 50, "{name}: too few errors ({plain}) to compare");
        assert!(
            equalised as f64 <= 1.2 * plain as f64,
            "{name}: {equalised} errors equalised, {plain} plain"
        );
    }
}

#[test]
fn a_blown_up_equaliser_resets_and_recovers() {
    let table = tables::qam_square(4).unwrap();
    let sent = draws(6_000, 4, 0xa3);
    let wave = wave_through(&table, &sent, &echoes(), 14.0);
    let mut eq = Equaliser::new(EqualiserConfig::DEFAULT, &table).unwrap();
    eq.forward.fill(Complex::new(40.0, 0.0));
    let p = params(&table);
    let mut timing = crate::linear::FeedforwardTiming::new(&p, &rrc());
    let mut halves = Vec::new();
    timing.process_spaced(&wave, 2, &mut halves);
    let mut out = Vec::new();
    eq.equalise_burst(&halves, Some(&carrier()), &table, &mut out);
    assert!(eq.resets() >= 1);
    assert!(out.iter().all(|z| z.re.is_finite() && z.im.is_finite()));
    assert_eq!(symbol_errors(&out, &sent[..5_000], &table, 3_000), 0);
}

#[test]
fn decision_feedback_cancels_a_strong_postcursor() {
    let table = tables::qam_square(16).unwrap();
    let sent = draws(8_000, 16, 0xa4);
    let channel = [(0, ONE), (SPS, Complex::from_polar(0.7, 2.0))];
    let wave = wave_through(&table, &sent, &channel, 20.0);
    let tail = &sent[..7_000];
    let run = |config| symbol_errors(&burst(&table, &wave, Some(config)), tail, &table, 1_000);
    let linear = run(EqualiserConfig::DEFAULT);
    let feedback = run(EqualiserConfig::DEFAULT.with_feedback(4));
    println!("postcursor: linear {linear}, feedback {feedback}");
    assert!(feedback <= linear, "feedback {feedback}, linear {linear}");
}

#[test]
fn the_blind_target_follows_the_table() {
    let target = |t: Constellation| BlindTarget::for_table(&t);
    assert!(matches!(
        target(tables::qam_square(16).unwrap()),
        BlindTarget::Axes { .. }
    ));
    assert!(matches!(
        target(tables::pam(2).unwrap()),
        BlindTarget::Axes { im, .. } if im == 0.0
    ));
    assert!(matches!(
        target(tables::psk(8).unwrap()),
        BlindTarget::Modulus(r) if (r - 1.0).abs() < 1e-5
    ));
    assert!(matches!(
        target(tables::apsk16_dvbs2(2.57).unwrap()),
        BlindTarget::Modulus(_)
    ));
}

#[test]
fn a_bad_configuration_is_refused() {
    let table = tables::qam_square(4).unwrap();
    let bad = |config| Equaliser::new(config, &table).err();
    assert_eq!(
        bad(EqualiserConfig::DEFAULT.with_span(0)),
        Some(EqualiserError::NoSpan)
    );
    assert_eq!(
        bad(EqualiserConfig {
            burst_passes: 0,
            ..EqualiserConfig::DEFAULT
        }),
        Some(EqualiserError::NoPasses)
    );
    assert_eq!(
        bad(EqualiserConfig {
            decision_step: 1.5,
            ..EqualiserConfig::DEFAULT
        }),
        Some(EqualiserError::StepOutOfRange(1.5))
    );
}

#[test]
fn any_block_split_gives_the_same_symbols() {
    let table = tables::qam_square(16).unwrap();
    let sent = draws(3_000, 16, 0xa5);
    let wave = wave_through(&table, &sent, &echoes(), 25.0);
    let p = params(&table);
    let demod = || {
        LinearDemod::new(&p, &rrc(), LinearTiming::CONTINUOUS, Some(carrier()))
            .with_equaliser(EqualiserConfig::DEFAULT.with_feedback(2))
            .unwrap()
    };
    let mut whole = demod();
    let mut a = Vec::new();
    whole.process(&wave, &mut a);
    let mut split = demod();
    let mut b = Vec::new();
    for chunk in wave.chunks(313) {
        split.process(chunk, &mut b);
    }
    assert_eq!(a, b);
}

#[test]
fn steady_state_allocates_nothing() {
    let table = tables::qam_square(16).unwrap();
    let p = params(&table);
    let wave = LinearMod::transmission(&p, &draws(2_048, 16, 0xa6));
    let mut demod = LinearDemod::new(&p, &rrc(), LinearTiming::CONTINUOUS, Some(carrier()))
        .with_equaliser(EqualiserConfig::DEFAULT.with_feedback(3))
        .unwrap();
    let mut out = Vec::with_capacity(wave.len());
    demod.process(&wave, &mut out);
    out.clear();
    demod.process(&wave, &mut out);
    out.clear();
    assert_no_alloc("LinearDemod::process (equalised)", || {
        demod.process(&wave, &mut out);
    });
    assert!(!out.is_empty());
}
