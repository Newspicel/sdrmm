use sdrmm_modem_test_support::ber::{
    impair::{Awgn, Cfo, ChannelSpec, ClockError, Impairment, Multipath, MultipathProfile},
    rng::Rng,
};

use super::*;
use crate::{
    constellation::tables,
    ofdm::modulator::OfdmMod,
    testutil::{points, symbol_errors},
};

const SYMBOLS: usize = 16;

fn table() -> Constellation {
    match tables::qam_square(16) {
        Ok(t) => t,
        Err(why) => panic!("16-QAM: {why}"),
    }
}

fn payload(params: &OfdmParams, seed: u64) -> Vec<Complex<f32>> {
    points(&table(), params.data_subcarriers() * SYMBOLS, seed)
}

fn burst(params: &OfdmParams, lead: usize, seed: u64) -> (Vec<Complex<f32>>, Vec<Complex<f32>>) {
    let points = payload(params, seed);
    let mut wave = vec![Complex::new(0.0, 0.0); lead];
    OfdmMod::new(params.clone()).frame(&points, &mut wave);
    wave.resize(wave.len() + 128, Complex::new(0.0, 0.0));
    (points, wave)
}

fn errors(sent: &[Complex<f32>], got: &[Complex<f32>]) -> usize {
    symbol_errors(&table(), sent, got)
}

fn decode(demod: &mut OfdmDemod, wave: &[Complex<f32>]) -> Vec<Complex<f32>> {
    assert!(demod.acquire(wave, 200).is_some(), "no preamble to acquire");
    let mut out = Vec::new();
    demod.demodulate(wave, SYMBOLS, &mut out);
    out
}

#[test]
fn a_bin_with_no_gain_erases_instead_of_amplifying() {
    let params = OfdmParams::wifi_like();
    let table = table();
    let demod = OfdmDemod::new(params.clone());
    let points = vec![table.points()[0]; params.data_subcarriers()];
    let mut out = vec![Llr(0.0); points.len() * table.bits_per_symbol()];
    demod.llrs(&points, &table, &mut out);
    assert!(out.iter().all(|llr| llr.0 == 0.0), "a dead channel decided");
}

#[test]
fn every_estimator_round_trips_a_clean_frame() {
    let params = OfdmParams::wifi_like();
    for estimator in [ChannelEstimator::LongTraining, ChannelEstimator::ShortComb] {
        for lead in [0usize, 5, 63, 140] {
            let (sent, wave) = burst(&params, lead, 0x0f_d1);
            let mut demod = OfdmDemod::new(params.clone()).with_estimator(estimator);
            let got = decode(&mut demod, &wave);
            assert_eq!(
                errors(&sent, &got),
                0,
                "{estimator:?} at lead {lead}: {} of {} points wrong",
                errors(&sent, &got),
                sent.len()
            );
        }
    }
}

#[test]
fn the_dmt_configuration_decodes_through_the_same_receiver() {
    let params = OfdmParams::dmt_like();
    let (sent, wave) = burst(&params, 17, 0x0d_47);
    let mut demod = OfdmDemod::new(params);
    let got = decode(&mut demod, &wave);
    assert_eq!(errors(&sent, &got), 0);
}

#[test]
fn a_carrier_offset_is_removed_across_a_whole_frame() {
    let params = OfdmParams::wifi_like();
    for cfo in [-0.02, -0.003, 0.003, 0.02] {
        let (sent, mut wave) = burst(&params, 31, 0x0f_c0);
        Cfo::from_cycles_per_sample(cfo).apply(&mut wave, &mut Rng::new(0));
        let mut demod = OfdmDemod::new(params.clone());
        let got = decode(&mut demod, &wave);
        assert_eq!(errors(&sent, &got), 0, "cfo {cfo}");
        assert!((demod.cfo() - cfo).abs() < 1e-5, "cfo {cfo}");
    }
}

#[test]
fn multipath_inside_the_prefix_is_equalised_and_past_it_is_not() {
    let params = OfdmParams::wifi_like();
    let (sent, clean) = burst(&params, 23, 0x0f_e0);
    let echo = |delay: usize| {
        let mut wave = clean.clone();
        Multipath::new(MultipathProfile::TwoRay {
            delay_samples: delay,
            relative_db: -3.0,
            phase_rad: 0.7,
        })
        .apply(&mut wave, &mut Rng::new(0x0e));
        let mut demod = OfdmDemod::new(params.clone());
        errors(&sent, &decode(&mut demod, &wave))
    };
    for delay in [1usize, 4, 8, 12] {
        assert_eq!(
            echo(delay),
            0,
            "an echo at {delay} samples is inside the window"
        );
    }
    assert!(
        echo(20) > sent.len() / 100,
        "an echo past the window's own start must break the one-tap model"
    );
}

#[test]
fn the_comb_tier_trades_delay_spread_for_estimator_noise() {
    let params = OfdmParams::wifi_like();
    let table = match tables::qam_square(4) {
        Ok(t) => t,
        Err(why) => panic!("QPSK: {why}"),
    };
    let sent = points(&table, params.data_subcarriers() * SYMBOLS, 0x0f_c8);
    let mut clean = vec![Complex::new(0.0, 0.0); 19];
    OfdmMod::new(params.clone()).frame(&sent, &mut clean);
    clean.resize(clean.len() + 128, Complex::new(0.0, 0.0));

    let with = |estimator, delay| {
        let mut wave = clean.clone();
        Multipath::new(MultipathProfile::TwoRay {
            delay_samples: delay,
            relative_db: -3.0,
            phase_rad: 2.1,
        })
        .apply(&mut wave, &mut Rng::new(0x0e));
        let mut demod = OfdmDemod::new(params.clone()).with_estimator(estimator);
        symbol_errors(&table, &sent, &decode(&mut demod, &wave))
    };
    assert_eq!(with(ChannelEstimator::LongTraining, 2), 0);
    assert_eq!(with(ChannelEstimator::ShortComb, 2), 0);
    assert_eq!(with(ChannelEstimator::LongTraining, 12), 0);
    assert!(
        with(ChannelEstimator::ShortComb, 12) > sent.len() / 10,
        "the comb estimate resolved a delay its anchors cannot represent"
    );
}

#[test]
fn pilot_tracking_is_what_survives_a_sampling_clock_error() {
    let params = OfdmParams::wifi_like();
    let (sent, clean) = burst(&params, 11, 0x0f_c1);
    let run = |tracking: bool| {
        let mut wave = clean.clone();
        ClockError::new(500.0).apply(&mut wave, &mut Rng::new(0));
        let mut demod = OfdmDemod::new(params.clone()).with_pilot_tracking(tracking);
        errors(&sent, &decode(&mut demod, &wave))
    };
    assert_eq!(run(true), 0, "500 ppm is well inside the prefix's budget");
    assert!(
        run(false) > sent.len() / 20,
        "without the pilot fit the same clock error must be visible"
    );
}

#[test]
fn llr_confidence_follows_the_per_bin_channel() {
    let params = OfdmParams::wifi_like();
    let table = table();
    let mut demod = OfdmDemod::new(params.clone());
    let mut truth = vec![Complex::new(1.0f32, 0.0); params.map().occupied().len()];
    let faded = params
        .map()
        .occupied()
        .iter()
        .position(|c| c.bin == params.map().data()[0].bin)
        .unwrap();
    truth[faded] = Complex::new(0.25, 0.0);
    demod.genie(params.data_offset(), &truth, 0.01);

    let points = vec![table.points()[6]; params.data_subcarriers()];
    let bits = table.bits_per_symbol();
    let mut llrs = vec![Llr(0.0); points.len() * bits];
    demod.llrs(&points, &table, &mut llrs);
    let confidence = |index: usize| {
        llrs[index * bits..(index + 1) * bits]
            .iter()
            .map(|l| f64::from(l.0.abs()))
            .sum::<f64>()
    };
    assert!(
        confidence(0) < confidence(1),
        "the faded bin's bits are as believable as the strong bin's: {} vs {}",
        confidence(0),
        confidence(1)
    );
    let label = table.hard_slice(points[1]);
    for bit in 0..bits {
        let llr = llrs[bits + bit].0;
        assert_eq!(
            (label >> bit) & 1 == 1,
            llr > 0.0,
            "bit {bit}: label {label:b}, llr {llr}"
        );
    }
}

#[test]
fn the_genie_receiver_agrees_with_the_acquiring_one_on_a_clean_frame() {
    let params = OfdmParams::wifi_like();
    let (sent, mut wave) = burst(&params, 0, 0x0f_9e);
    ChannelSpec::default()
        .awgn(Awgn::with_sigma(0.02))
        .build()
        .apply(&mut wave, &mut Rng::new(0x9e));
    let mut acquiring = OfdmDemod::new(params.clone());
    let a = decode(&mut acquiring, &wave);

    let mut genie = OfdmDemod::new(params.clone())
        .with_pilot_tracking(false)
        .with_window_backoff(0);
    genie.genie(
        params.data_offset(),
        &vec![Complex::new(1.0, 0.0); params.map().occupied().len()],
        1.0,
    );
    let mut b = Vec::new();
    genie.demodulate(&wave, SYMBOLS, &mut b);

    assert_eq!(errors(&sent, &a), 0);
    assert_eq!(errors(&sent, &b), 0);
    let worst = a
        .iter()
        .zip(&b)
        .map(|(x, y)| f64::from((x - y).norm()))
        .fold(0.0f64, f64::max);
    assert!(worst < 0.1, "worst point disagreement {worst}");
}
