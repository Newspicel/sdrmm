//! Helpers decoder tests share: settings, signal shaping, channel runs and view checks.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test helpers fail loudly on what a test did not set up"
)]

use std::f64::consts::TAU;

pub mod frames;
/// The known-key voice samples of github.com/tylerwatt12/known-key-mbe-samples: encrypted vocoder
/// frames, their key and the audio they decode to. The ignored tests read a checkout named by
/// `KNOWN_KEY_SAMPLES`.
pub mod known_keys;
pub mod scene;
pub mod xng;

use num_complex::Complex;
use sdrmm_wire::{ChannelParams, ChannelSettings, DecoderEvent};

use sdrmm_decoder::{AUDIO_RATE, Channel, ChannelOutputs};
pub use sdrmm_modem_test_support::audio::{dominant_tone, tone_amplitude, tone_power};

pub fn settings(params: ChannelParams) -> ChannelSettings {
    ChannelSettings {
        frequency_hz: 0.0,
        squelch: sdrmm_wire::Squelch::Off,
        params,
        blanker: Default::default(),
        if_filter: Default::default(),
    }
}

pub fn cf32_le(bytes: &[u8]) -> Vec<Complex<f32>> {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|s| {
            Complex::new(
                f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                f32::from_le_bytes([s[4], s[5], s[6], s[7]]),
            )
        })
        .collect()
}

pub fn complex_tone(freq_norm: f64, len: usize) -> Vec<Complex<f32>> {
    (0..len)
        .map(|n| {
            let p = TAU * freq_norm * n as f64;
            Complex::new(p.cos() as f32, p.sin() as f32)
        })
        .collect()
}

pub fn fm_iq(rate: f64, f_mod: f64, deviation_hz: f64, len: usize) -> Vec<Complex<f32>> {
    let mut phase = 0.0f64;
    (0..len)
        .map(|k| {
            phase += TAU * deviation_hz * (TAU * f_mod * k as f64 / rate).cos() / rate;
            Complex::from_polar(1.0, phase as f32)
        })
        .collect()
}

pub fn am_iq(rate: f64, f_mod: f64, depth: f32, len: usize) -> Vec<Complex<f32>> {
    (0..len)
        .map(|k| {
            let audio = (TAU * f_mod * k as f64 / rate).cos() as f32;
            Complex::new(1.0 + depth * audio, 0.0)
        })
        .collect()
}

pub fn component(iq: &[Complex<f32>], freq_hz: f64, rate: f64) -> f64 {
    let step = TAU * freq_hz / rate;
    let sum: Complex<f64> = iq
        .iter()
        .enumerate()
        .map(|(k, s)| {
            Complex::new(f64::from(s.re), f64::from(s.im))
                * Complex::from_polar(1.0, -step * k as f64)
        })
        .sum();
    sum.norm() / iq.len() as f64
}

pub fn run_ragged(chan: &mut dyn Channel, iq: &[Complex<f32>]) -> Vec<f32> {
    let mut out = ChannelOutputs::default();
    let mut audio = Vec::new();
    let mut pos = 0;
    for len in [997usize, 1, 4_096, 65, 2_048, 7, 1_024].iter().cycle() {
        if pos >= iq.len() {
            break;
        }
        let end = (pos + len).min(iq.len());
        out.reset();
        chan.process(&iq[pos..end], &mut out);
        if !out.audio_pcm.is_empty() {
            assert_eq!(out.audio_rate, AUDIO_RATE);
        }
        audio.extend_from_slice(&out.audio_pcm);
        pos = end;
    }
    audio
}

pub fn run_events(chan: &mut dyn Channel, iq: &[Complex<f32>]) -> Vec<DecoderEvent> {
    let mut out = ChannelOutputs::default();
    let mut events = Vec::new();
    let mut pos = 0;
    for len in [997usize, 1, 4_096, 65, 2_048, 7, 1_024].iter().cycle() {
        if pos >= iq.len() {
            break;
        }
        let end = (pos + len).min(iq.len());
        out.reset();
        chan.process(&iq[pos..end], &mut out);
        events.append(&mut out.events);
        pos = end;
    }
    events
}

pub fn split_stereo(pcm: &[f32]) -> (Vec<f32>, Vec<f32>) {
    assert_eq!(pcm.len() % 2, 0, "interleaved stereo needs whole frames");
    (
        pcm.iter().step_by(2).copied().collect(),
        pcm.iter().skip(1).step_by(2).copied().collect(),
    )
}

pub use sdrmm_modem_test_support::noise::complex_noise;

pub fn frequency_shift(iq: &[Complex<f32>], hz: f64, rate: f64) -> Vec<Complex<f32>> {
    iq.iter()
        .enumerate()
        .map(|(index, &value)| {
            let phase = TAU * hz * index as f64 / rate;
            value * Complex::new(phase.cos() as f32, phase.sin() as f32)
        })
        .collect()
}

pub fn sample_clock_offset(iq: &[Complex<f32>], ppm: f64) -> Vec<Complex<f32>> {
    const HALF: isize = 16;
    let step = 1.0 / (1.0 + ppm * 1e-6);
    let count = ((iq.len() as f64 - HALF as f64) / step) as usize;
    (0..count)
        .map(|index| {
            let position = index as f64 * step;
            let center = position.floor() as isize;
            (center - HALF + 1..=center + HALF)
                .filter_map(|tap| {
                    let sample = iq.get(usize::try_from(tap).ok()?)?;
                    let x = position - tap as f64;
                    let sinc = if x.abs() < 1e-12 {
                        1.0
                    } else {
                        (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                    };
                    let window = 0.42
                        + 0.5 * (std::f64::consts::PI * x / HALF as f64).cos()
                        + 0.08 * (TAU * x / HALF as f64).cos();
                    Some(sample * (sinc * window) as f32)
                })
                .sum()
        })
        .collect()
}

pub fn multipath(iq: &[Complex<f32>], paths: &[(usize, f32, f64)], rate: f64) -> Vec<Complex<f32>> {
    (0..iq.len())
        .map(|index| {
            paths
                .iter()
                .filter(|&&(delay, _, _)| delay <= index)
                .map(|&(delay, gain, doppler_hz)| {
                    let phase = TAU * doppler_hz * index as f64 / rate;
                    iq[index - delay] * gain * Complex::new(phase.cos() as f32, phase.sin() as f32)
                })
                .sum()
        })
        .collect()
}

pub fn at_snr(signal: &[Complex<f32>], snr_db: f32, seed: u64) -> Vec<Complex<f32>> {
    let power = signal
        .iter()
        .map(num_complex::Complex::norm_sqr)
        .sum::<f32>()
        / signal.len() as f32;
    let sigma = (power / 10f32.powf(snr_db / 10.0) / 2.0).sqrt();
    let mut noisy = signal.to_vec();
    add_awgn(&mut noisy, sigma, seed);
    noisy
}

pub fn add_awgn(iq: &mut [Complex<f32>], sigma: f32, seed: u64) {
    let mut state = seed | 1;
    let mut gaussian = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let u = f64::from((state >> 40) as u32) / f64::from(1u32 << 24);
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let v = f64::from((state >> 40) as u32) / f64::from(1u32 << 24);
        ((-2.0 * u.max(1e-12).ln()).sqrt() * (TAU * v).cos()) as f32
    };
    for sample in iq {
        *sample += Complex::new(gaussian() * sigma, gaussian() * sigma);
    }
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len() as f64).sqrt() as f32
}

/// Under `cargo xtask sanitize` the decoders run several times slower than the signal they
/// decode, so the real-time gates would measure the instrumentation instead of the code.
pub fn realtime_budget(seconds: f64) -> f64 {
    if cfg!(sanitized) {
        f64::INFINITY
    } else {
        seconds
    }
}

pub const THROUGHPUT_ATTEMPTS: usize = 5;

pub fn fastest_seconds(attempts: usize, mut timed: impl FnMut() -> f64) -> f64 {
    (0..attempts).map(|_| timed()).fold(f64::INFINITY, f64::min)
}

/// Every field a view of `spec` names is one some sample presents, and no sample presents
/// one key twice.
pub fn assert_views_hold(spec: &sdrmm_wire::KindSpec, samples: &[DecoderEvent]) {
    let kind = &spec.entry.kind;
    assert!(!spec.entry.views.is_empty(), "{kind} declares no view");
    let mut shown = std::collections::BTreeSet::new();
    for sample in samples {
        assert_eq!(sample.kind, *kind, "a sample of another kind");
        let view = spec
            .codec
            .present(kind, &sample.data())
            .unwrap_or_else(|error| panic!("{kind} sample reads: {error}"));
        let keys = view.keys();
        let unique: std::collections::BTreeSet<&str> = keys.iter().copied().collect();
        assert_eq!(unique.len(), keys.len(), "{kind} repeats a key: {keys:?}");
        shown.extend(keys.into_iter().map(str::to_owned));
    }
    for view in &spec.entry.views {
        let missing: Vec<&str> = view
            .fields()
            .into_iter()
            .filter(|field| !shown.contains(*field))
            .collect();
        assert!(
            missing.is_empty(),
            "{kind} view names {missing:?}, shown {shown:?}"
        );
    }
}

/// The filter `spec` asks for, built at `rate` for a test that feeds a decoder by hand.
pub fn built_filter(spec: sdrmm_wire::FilterSpec, rate: f64) -> sdrmm_decoder::ChannelFilter {
    sdrmm_decoder::ChannelFilter::new(&spec, rate).expect("a valid channel filter")
}
