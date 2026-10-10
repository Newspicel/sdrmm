use num_complex::Complex;
use sdrmm_wire::{LoraCodingRate, LoraImplicitHeader, LoraIntegrity, LoraIq};

use super::{Decoder, Frame, Layout};
use crate::synth::lora::LoraWaveform;

const OVERSAMPLING: f64 = 2.0;

struct Gaussian(u64);

impl Gaussian {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn sample(&mut self, sigma: f64) -> Complex<f32> {
        let radius = (-2.0 * self.uniform().ln()).sqrt() * sigma;
        let angle = std::f64::consts::TAU * self.uniform();
        Complex::new((radius * angle.cos()) as f32, (radius * angle.sin()) as f32)
    }
}

fn layout(bandwidth_hz: f64, factors: std::ops::RangeInclusive<u8>) -> Layout {
    Layout {
        bandwidth_hz,
        spreading_factors: factors,
        iq: LoraIq::Normal,
        implicit_header: None,
    }
}

struct Scene {
    iq: Vec<Complex<f32>>,
    rate: f64,
}

impl Scene {
    fn new(bandwidth_hz: f64, lead_s: f64) -> Self {
        let rate = bandwidth_hz * OVERSAMPLING;
        Self {
            iq: vec![Complex::new(0.0, 0.0); (lead_s * rate) as usize],
            rate,
        }
    }

    fn add(&mut self, waveform: &LoraWaveform, payload: &[u8], delay_s: f64, cfo_hz: f64) {
        let frame = waveform.frame_at(payload, self.rate, delay_s);
        let offset = self.iq.len();
        self.iq.resize(offset + frame.len(), Complex::new(0.0, 0.0));
        for (k, sample) in frame.into_iter().enumerate() {
            let turns = cfo_hz * (offset + k) as f64 / self.rate;
            self.iq[offset + k] +=
                sample * Complex::from_polar(1.0, (std::f64::consts::TAU * turns.fract()) as f32);
        }
    }

    fn pause(&mut self, seconds: f64) {
        let len = self.iq.len() + (seconds * self.rate) as usize;
        self.iq.resize(len, Complex::new(0.0, 0.0));
    }

    fn noise(mut self, snr_db: f64, seed: u64) -> Self {
        let sigma = (OVERSAMPLING * 10f64.powf(-snr_db / 10.0) / 2.0).sqrt();
        let mut gaussian = Gaussian(seed | 1);
        for sample in &mut self.iq {
            *sample += gaussian.sample(sigma);
        }
        self
    }
}

fn decode(layout: &Layout, iq: &[Complex<f32>]) -> Vec<Frame> {
    decode_at(layout, 0.0, iq)
}

fn decode_at(layout: &Layout, carrier_hz: f64, iq: &[Complex<f32>]) -> Vec<Frame> {
    let mut decoder = Decoder::new(layout, carrier_hz);
    let mut frames = Vec::new();
    let mut at = 0;
    let mut size = 777;
    while at < iq.len() {
        let end = (at + size).min(iq.len());
        decoder.process(&iq[at..end], &mut frames);
        at = end;
        size = size * 7 % 5_003 + 1;
    }
    frames
}

fn waveform(spreading_factor: u8, bandwidth_hz: f64) -> LoraWaveform {
    LoraWaveform {
        spreading_factor,
        bandwidth_hz,
        ..LoraWaveform::default()
    }
}

#[test]
fn every_spreading_factor_decodes_a_clean_frame() {
    for sf in 7..=12 {
        let waveform = waveform(sf, 125_000.0);
        let mut scene = Scene::new(125_000.0, 0.013);
        scene.add(&waveform, b"hello lora", 0.0, 0.0);
        scene.pause(0.05);
        let frames = decode(&layout(125_000.0, sf..=sf), &scene.iq);
        assert_eq!(frames.len(), 1, "SF{sf}: {frames:?}");
        let frame = &frames[0];
        assert_eq!(frame.payload, b"hello lora", "SF{sf}");
        assert_eq!(frame.integrity, LoraIntegrity::CrcOk);
        assert_eq!(frame.spreading_factor, sf);
        assert_eq!(frame.sync_word, 0x12);
        assert_eq!(frame.low_data_rate, sf >= 11);
        assert_eq!(frame.coding_rate, LoraCodingRate::Cr45);
    }
}

#[test]
fn offsets_in_frequency_and_time_are_recovered() {
    let waveform = LoraWaveform {
        coding_rate: LoraCodingRate::Cr48,
        sync_word: 0x34,
        ..waveform(8, 125_000.0)
    };
    for (cfo_hz, delay_s) in [
        (0.0, 0.0),
        (11_300.0, 1.3e-6),
        (-23_700.0, 7.77e-6),
        (4_100.0, 3.1e-3),
    ] {
        let mut scene = Scene::new(125_000.0, 0.02);
        scene.add(&waveform, b"offsets", delay_s, cfo_hz);
        scene.pause(0.03);
        let frames = decode(&layout(125_000.0, 8..=8), &scene.iq);
        assert_eq!(frames.len(), 1, "cfo {cfo_hz} delay {delay_s}");
        assert_eq!(frames[0].payload, b"offsets");
        assert_eq!(frames[0].sync_word, 0x34);
        assert!(
            (f64::from(frames[0].frequency_error_hz) - cfo_hz).abs() < 50.0,
            "cfo {cfo_hz} read {}",
            frames[0].frequency_error_hz
        );
    }
}

#[test]
fn every_coding_rate_survives_noise() {
    for rate in [
        LoraCodingRate::Cr45,
        LoraCodingRate::Cr46,
        LoraCodingRate::Cr47,
        LoraCodingRate::Cr48,
    ] {
        let waveform = LoraWaveform {
            coding_rate: rate,
            ..waveform(9, 250_000.0)
        };
        let mut scene = Scene::new(250_000.0, 0.01);
        scene.add(&waveform, b"coding rates", 0.0, 2_500.0);
        scene.pause(0.02);
        let frames = decode(&layout(250_000.0, 9..=9), &scene.noise(-5.0, 42).iq);
        assert_eq!(frames.len(), 1, "{rate:?}");
        assert_eq!(frames[0].payload, b"coding rates", "{rate:?}");
        assert_eq!(frames[0].coding_rate, rate);
    }
}

#[test]
fn frames_near_the_sensitivity_limit_still_decode() {
    for (sf, snr_db) in [(7u8, -6.0), (9, -11.0), (12, -18.0)] {
        let waveform = waveform(sf, 125_000.0);
        let mut decoded = 0;
        let trials = 4u32;
        for trial in 0..trials {
            let mut scene = Scene::new(125_000.0, 0.02);
            scene.add(&waveform, b"weak", 0.37e-6 * f64::from(trial), 1_000.0);
            scene.pause(0.03);
            let frames = decode(
                &layout(125_000.0, sf..=sf),
                &scene.noise(snr_db, 7 + u64::from(trial)).iq,
            );
            decoded += frames
                .iter()
                .filter(|f| f.integrity == LoraIntegrity::CrcOk && f.payload == b"weak")
                .count();
        }
        assert!(
            decoded + 1 >= trials as usize,
            "SF{sf} at {snr_db} dB: {decoded}/{trials}"
        );
    }
}

#[test]
fn noise_alone_yields_no_frames() {
    let scene = Scene::new(125_000.0, 1.0).noise(0.0, 99);
    let frames = decode(&layout(125_000.0, 7..=12), &scene.iq);
    assert!(frames.is_empty(), "{frames:?}");
}

#[test]
fn all_spreading_factors_are_heard_at_once() {
    let mut scene = Scene::new(125_000.0, 0.01);
    scene.add(&waveform(7, 125_000.0), b"seven", 0.0, 0.0);
    scene.pause(0.02);
    scene.add(&waveform(10, 125_000.0), b"ten", 0.0, 0.0);
    scene.pause(0.02);
    scene.add(&waveform(12, 125_000.0), b"twelve", 0.0, 0.0);
    scene.pause(0.05);
    let frames = decode(&layout(125_000.0, 7..=12), &scene.iq);
    let heard: Vec<(u8, &[u8])> = frames
        .iter()
        .map(|f| (f.spreading_factor, f.payload.as_slice()))
        .collect();
    assert_eq!(
        heard,
        [(7, &b"seven"[..]), (10, &b"ten"[..]), (12, &b"twelve"[..])]
    );
}

#[test]
fn an_implicit_header_frame_decodes_with_the_configured_shape() {
    let waveform = LoraWaveform {
        implicit_header: true,
        coding_rate: LoraCodingRate::Cr47,
        ..waveform(8, 125_000.0)
    };
    let mut scene = Scene::new(125_000.0, 0.01);
    scene.add(&waveform, b"implicit", 0.0, -3_000.0);
    scene.pause(0.03);
    let layout = Layout {
        implicit_header: Some(LoraImplicitHeader {
            length: 8,
            coding_rate: LoraCodingRate::Cr47,
            crc: true,
        }),
        ..layout(125_000.0, 8..=8)
    };
    let frames = decode(&layout, &scene.iq);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].payload, b"implicit");
    assert!(frames[0].implicit_header);
    assert_eq!(frames[0].integrity, LoraIntegrity::CrcOk);
}

#[test]
fn inverted_iq_needs_the_inverted_receiver() {
    let waveform = LoraWaveform {
        inverted: true,
        crc: false,
        sync_word: 0x34,
        ..waveform(9, 125_000.0)
    };
    let mut scene = Scene::new(125_000.0, 0.01);
    scene.add(&waveform, b"downlink", 0.0, 1_500.0);
    scene.pause(0.03);
    assert!(decode(&layout(125_000.0, 9..=9), &scene.iq).is_empty());
    for iq in [LoraIq::Inverted, LoraIq::Both] {
        let layout = Layout {
            iq,
            ..layout(125_000.0, 9..=9)
        };
        let frames = decode(&layout, &scene.iq);
        assert_eq!(frames.len(), 1, "{iq:?}");
        assert_eq!(frames[0].payload, b"downlink");
        assert!(frames[0].inverted_iq);
        assert_eq!(frames[0].integrity, LoraIntegrity::NoCrc);
        assert!((frames[0].frequency_error_hz - 1_500.0).abs() < 50.0);
    }
}

#[test]
fn a_long_slow_frame_rides_out_clock_drift() {
    let waveform = LoraWaveform {
        coding_rate: LoraCodingRate::Cr48,
        ..waveform(12, 125_000.0)
    };
    let payload: Vec<u8> = (0..120u8).collect();
    let carrier_hz = 868_100_000.0;
    let crystal = 20e-6;
    let mut scene = Scene::new(125_000.0, 0.05);
    let drifted = LoraWaveform {
        bandwidth_hz: 125_000.0 * (1.0 + crystal),
        ..waveform
    };
    scene.add(&drifted, &payload, 0.0, carrier_hz * crystal);
    scene.pause(0.1);
    let noisy = scene.noise(-10.0, 5);
    assert!(
        decode(&layout(125_000.0, 12..=12), &noisy.iq)
            .iter()
            .all(|f| f.payload != payload)
    );
    let frames = decode_at(&layout(125_000.0, 12..=12), carrier_hz, &noisy.iq);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].payload, payload);
    assert_eq!(frames[0].integrity, LoraIntegrity::CrcOk);
}

#[test]
fn a_damaged_payload_reports_its_crc() {
    let waveform = waveform(7, 125_000.0);
    let mut scene = Scene::new(125_000.0, 0.01);
    scene.add(&waveform, b"crc check", 0.0, 0.0);
    scene.pause(0.02);
    let n = 128.0 * OVERSAMPLING;
    let payload_start = 0.01 * scene.rate + (8.0 + 2.0 + 2.25 + 9.0) * n;
    for k in 0..(4.0 * n) as usize {
        scene.iq[payload_start as usize + k] = Complex::new(0.0, 0.0);
    }
    let frames = decode(&layout(125_000.0, 7..=7), &scene.iq);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].integrity, LoraIntegrity::CrcFailed);
}
