use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{Decibels, math::complex::widen};
use sdrmm_modem_test_support::ber::analog::{analyse_tone, tone};

use super::*;

const RATE: f64 = 48_000.0;
const BANDWIDTH: f64 = 3_000.0 / RATE;
const TONE: f64 = 1_000.0 / RATE;
const WINDOW: std::ops::Range<usize> = 8_192..40_928;

fn params(sideband: Sideband, method: SsbMethod) -> SsbParams {
    SsbParams::new(sideband, method, BANDWIDTH)
}

fn excite(params: &SsbParams, samples: usize) -> Vec<Complex<f32>> {
    let mut out = Vec::new();
    SsbMod::new(params).process(&tone(TONE, 0.5, samples), &mut out);
    out
}

fn detect(params: &SsbParams, detector: SsbDetector, iq: &[Complex<f32>]) -> Vec<f32> {
    let mut out = Vec::new();
    SsbDemod::new(params, detector, true).process(iq, &mut out);
    out
}

fn sideband_rejection_db(iq: &[Complex<f32>], sideband: Sideband) -> f64 {
    let component = |f: f64| {
        iq.iter()
            .enumerate()
            .map(|(n, &z)| widen(z) * Complex::from_polar(1.0, -TAU * f * n as f64))
            .sum::<Complex<f64>>()
            .norm_sqr()
    };
    let (up, down) = (component(TONE), component(-TONE));
    let (wanted, unwanted) = match sideband {
        Sideband::Upper => (up, down),
        Sideband::Lower => (down, up),
    };
    (wanted / unwanted).power_to_db()
}

#[test]
fn both_methods_radiate_one_sideband() {
    for method in [SsbMethod::Hilbert, SsbMethod::Weaver] {
        for sideband in [Sideband::Upper, Sideband::Lower] {
            let iq = excite(&params(sideband, method), 16_384);
            let rejection = sideband_rejection_db(&iq[1_024..9_184], sideband);
            assert!(rejection > 40.0, "{method:?} {sideband:?}: {rejection} dB");
        }
    }
}

#[test]
fn every_exciter_detector_pair_round_trips() {
    for method in [SsbMethod::Hilbert, SsbMethod::Weaver] {
        for detector in [SsbDetector::Filter, SsbDetector::Weaver] {
            for sideband in [Sideband::Upper, Sideband::Lower] {
                let params = params(sideband, method);
                let audio = detect(&params, detector, &excite(&params, 49_152));
                let analysis = analyse_tone(&audio[WINDOW], TONE);
                assert!(
                    (analysis.amplitude - 0.5).abs() < 0.02,
                    "{method:?}/{detector:?}/{sideband:?}: amplitude {}",
                    analysis.amplitude
                );
                assert!(
                    analysis.thd() < 0.01,
                    "{method:?}/{detector:?}/{sideband:?}: thd {}",
                    analysis.thd()
                );
            }
        }
    }
}

#[test]
fn a_receiver_on_the_wrong_sideband_hears_nothing() {
    let sent = params(Sideband::Upper, SsbMethod::Hilbert);
    let mut listening = sent;
    listening.sideband = Sideband::Lower;
    let iq = excite(&sent, 49_152);
    let right = analyse_tone(&detect(&sent, SsbDetector::Filter, &iq)[WINDOW], TONE);
    let wrong = analyse_tone(&detect(&listening, SsbDetector::Filter, &iq)[WINDOW], TONE);
    let rejection = (right.amplitude / wrong.amplitude).amplitude_to_db();
    assert!(
        rejection > 40.0,
        "opposite-sideband rejection {rejection} dB"
    );
}
