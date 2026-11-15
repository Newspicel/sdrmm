use num_complex::Complex;
use sdrmm_modem_test_support::ber::analog::{analyse_tone, tone};

use super::*;

const RATE: f64 = 48_000.0;
const BANDWIDTH: f64 = 4_000.0 / RATE;
const TONE: f64 = 1_000.0 / RATE;

fn tone_amplitude(audio: &[f32], freq: f64) -> f64 {
    analyse_tone(audio, freq).amplitude
}

fn harmonic_ratio(audio: &[f32], freq: f64) -> f64 {
    analyse_tone(audio, freq).thd()
}

fn modulated(params: &AmParams, amplitude: f32, samples: usize) -> Vec<Complex<f32>> {
    let mut out = Vec::new();
    AmMod::new(params).process(&tone(TONE, amplitude, samples), &mut out);
    out
}

fn demodulated(params: &AmParams, rx: &AmRx, iq: &[Complex<f32>]) -> Vec<f32> {
    let mut out = Vec::new();
    AmDemod::new(params, rx).process(iq, &mut out);
    out
}

#[test]
fn the_full_carrier_envelope_carries_its_stated_depth() {
    let params = AmParams::new(AmMode::FullCarrier { depth: 0.8 }, BANDWIDTH);
    let iq = modulated(&params, 1.0, 16_384);
    let steady = &iq[512..];
    let peak = steady.iter().map(|z| z.norm()).fold(f32::MIN, f32::max);
    let trough = steady.iter().map(|z| z.norm()).fold(f32::MAX, f32::min);
    assert!((peak - 1.8).abs() < 0.02, "peak {peak}");
    assert!((trough - 0.2).abs() < 0.02, "trough {trough}");
    let depth = f64::from(peak - trough) / f64::from(peak + trough);
    assert!((depth - 0.8).abs() < 0.02, "measured depth {depth}");
}

#[test]
fn the_envelope_tier_recovers_a_tone() {
    let params = AmParams::new(AmMode::FullCarrier { depth: 0.8 }, BANDWIDTH);
    let iq = modulated(&params, 0.5, 32_768);
    let audio = demodulated(&params, &AmRx::new(AmDetector::Envelope), &iq);
    let window = &audio[4_096..28_672];
    let amplitude = tone_amplitude(window, TONE);
    assert!((amplitude - 0.4).abs() < 0.01, "amplitude {amplitude}");
    assert!(
        harmonic_ratio(window, TONE) < 0.01,
        "distortion {}",
        harmonic_ratio(window, TONE)
    );
}

#[test]
fn the_synchronous_tier_recovers_the_same_tone() {
    let params = AmParams::new(AmMode::FullCarrier { depth: 0.8 }, BANDWIDTH);
    let iq = modulated(&params, 0.5, 32_768);
    let rx = AmRx::new(AmDetector::Synchronous { loop_bw: 1e-3 });
    let audio = demodulated(&params, &rx, &iq);
    let window = &audio[8_192..28_736];
    let amplitude = tone_amplitude(window, TONE);
    assert!((amplitude - 0.4).abs() < 0.02, "amplitude {amplitude}");
    assert!(
        harmonic_ratio(window, TONE) < 0.02,
        "distortion {}",
        harmonic_ratio(window, TONE)
    );
}

#[test]
fn the_carrier_loop_pulls_in_an_offset() {
    let params = AmParams::new(AmMode::FullCarrier { depth: 0.8 }, BANDWIDTH);
    let mut iq = modulated(&params, 0.5, 65_536);
    let offset = 2e-4;
    for (n, s) in iq.iter_mut().enumerate() {
        *s *= Complex::from_polar(1.0, (2.0 * std::f64::consts::PI * offset * n as f64) as f32);
    }
    let rx = AmRx::new(AmDetector::Synchronous { loop_bw: 1e-3 });
    let mut demod = AmDemod::new(&params, &rx);
    let mut audio = Vec::new();
    demod.process(&iq, &mut audio);
    assert!(demod.lock() > 0.5, "lock {}", demod.lock());
    let amplitude = tone_amplitude(&audio[32_768..65_504], TONE);
    assert!((amplitude - 0.4).abs() < 0.03, "amplitude {amplitude}");
}

#[test]
fn a_suppressed_carrier_defeats_the_envelope_tier() {
    let params = AmParams::new(AmMode::Suppressed, BANDWIDTH);
    let iq = modulated(&params, 0.8, 32_768);
    let coherent = demodulated(
        &params,
        &AmRx::new(AmDetector::Synchronous { loop_bw: 1e-3 }),
        &iq,
    );
    let window = &coherent[8_192..28_672];
    let amplitude = tone_amplitude(window, TONE);
    assert!(
        (amplitude - 0.8).abs() < 0.03,
        "coherent amplitude {amplitude}"
    );
    assert!(harmonic_ratio(window, TONE) < 0.02);

    let folded = demodulated(&params, &AmRx::new(AmDetector::Envelope), &iq);
    let window = &folded[8_192..28_672];
    let distortion = harmonic_ratio(window, TONE);
    assert!(distortion > 0.4, "rectified distortion {distortion}");
}

#[test]
fn a_vestigial_sideband_detects_undistorted() {
    let mut params = AmParams::new(AmMode::FullCarrier { depth: 0.8 }, BANDWIDTH);
    params.vestige = Some(500.0 / RATE);
    params.band_taps = 257;
    let iq = modulated(&params, 0.5, 65_536);
    let rx = AmRx::new(AmDetector::Synchronous { loop_bw: 1e-3 });
    let audio = demodulated(&params, &rx, &iq);
    let window = &audio[8_192..60_032];
    let amplitude = tone_amplitude(window, TONE);
    assert!((amplitude - 0.2).abs() < 0.02, "amplitude {amplitude}");
    assert!(
        harmonic_ratio(window, TONE) < 0.02,
        "distortion {}",
        harmonic_ratio(window, TONE)
    );
}

#[test]
fn an_envelope_detector_pays_quadrature_distortion_on_a_vestigial_sideband() {
    let mut params = AmParams::new(AmMode::FullCarrier { depth: 0.8 }, BANDWIDTH);
    params.vestige = Some(500.0 / RATE);
    params.band_taps = 257;
    let deep = modulated(&params, 0.5, 65_536);
    let audio = demodulated(&params, &AmRx::new(AmDetector::Envelope), &deep);
    let distortion = harmonic_ratio(&audio[8_192..60_032], TONE);
    assert!(
        (0.05..0.2).contains(&distortion),
        "distortion at depth 0.8: {distortion}"
    );
    params.mode = AmMode::FullCarrier { depth: 0.2 };
    let shallow = modulated(&params, 0.5, 65_536);
    let audio = demodulated(&params, &AmRx::new(AmDetector::Envelope), &shallow);
    let shallow = harmonic_ratio(&audio[8_192..60_032], TONE);
    let ratio = shallow / distortion;
    assert!((ratio - 0.25).abs() < 0.03, "distortion ratio {ratio}");
}

#[test]
fn a_bare_receiver_passes_the_detector_output_through() {
    let params = AmParams::new(AmMode::FullCarrier { depth: 0.8 }, BANDWIDTH);
    let iq = modulated(&params, 0.5, 4_096);
    let rx = AmRx {
        detector: AmDetector::Envelope,
        predetection: false,
        audio_filter: false,
        dc_block: false,
    };
    let audio = demodulated(&params, &rx, &iq);
    assert_eq!(audio.len(), iq.len());
    for (k, (&a, x)) in audio.iter().zip(&iq).enumerate() {
        assert!((a - x.norm()).abs() < 1e-6, "sample {k}");
    }
}
