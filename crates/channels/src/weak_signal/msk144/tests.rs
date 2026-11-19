use std::time::{Duration, SystemTime};

use sdrmm_decoder::{Channel, ChannelCtx, ChannelOutputs, NativeChannel};
use sdrmm_decoder_test_support::settings;
use sdrmm_wire::ChannelParams;

use super::*;
use crate::builtin::fixtures::Heard;
use crate::weak_signal::fixture::recording;
use crate::weak_signal::ftx::message::pack;

fn noise(len: usize, sigma: f32, mut state: u64) -> Vec<f32> {
    let mut uniform = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 11) as f64 / (1u64 << 53) as f64).max(1e-12)
    };
    (0..len)
        .map(|_| {
            let (u, v) = (uniform(), uniform());
            sigma * ((-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()) as f32
        })
        .collect()
}

fn ping(text: &str, center_hz: f64, frames: usize, at: usize, amplitude: f32) -> Vec<f32> {
    let mut block = noise(BLOCK, 0.1, 7);
    let wave = waveform(pack(text).expect("packs"), center_hz, frames).expect("encodes");
    for (sample, value) in block[at..].iter_mut().zip(&wave) {
        *sample += amplitude * value;
    }
    block
}

#[test]
fn a_short_ping_decodes_with_its_carrier() {
    let block = ping("CQ K1JT FN20", 1_520.0, 4, 2_000, 0.3);
    let found = Msk144Decoder::new()
        .decode(&block, 1_500.0, 100.0)
        .expect("decodes");
    assert_eq!(found.text, "CQ K1JT FN20");
    assert!(
        (found.frequency_hz - 1_520.0).abs() < 3.0,
        "{}",
        found.frequency_hz
    );
    assert!(
        found.start_s > 0.1 && found.start_s < 0.4,
        "{}",
        found.start_s
    );
}

#[test]
fn a_weak_steady_signal_decodes_from_averaged_frames() {
    let block = ping("W1AW K1JT -05", 1_480.0, 8, 0, 0.07);
    let found = Msk144Decoder::new()
        .decode(&block, 1_500.0, 100.0)
        .expect("decodes");
    assert_eq!(found.text, "W1AW K1JT -05");
}

#[test]
fn noise_alone_reads_nothing() {
    let mut decoder = Msk144Decoder::new();
    for seed in 0..20 {
        assert!(
            decoder
                .decode(&noise(BLOCK, 0.1, seed), 1_500.0, 200.0)
                .is_none()
        );
    }
}

#[test]
fn a_signal_outside_the_tolerance_is_not_read() {
    let block = ping("CQ K1JT FN20", 1_700.0, 4, 2_000, 0.3);
    assert!(Msk144Decoder::new().decode(&block, 1_500.0, 50.0).is_none());
}

fn channel() -> Msk144Channel {
    let params = ChannelParams::of(&Msk144Params::default()).expect("msk144 settings");
    Msk144Channel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(params),
    )
    .expect("builds")
}

/// Feeds `audio` a hop at a time, as a receiver would, and returns what the channel emitted.
fn hear(audio: &[f32], captured: Option<SystemTime>) -> Vec<WsjtMessage> {
    let mut channel = channel();
    if let Some(time) = captured {
        channel.captured_at(time);
    }
    let mut out = ChannelOutputs::default();
    let iq: Vec<Complex<f32>> = audio
        .iter()
        .map(|&sample| Complex::new(sample, 0.0))
        .collect();
    for hop in iq.chunks(BLOCK_STEP) {
        channel.process(hop, &mut out);
        channel.settle(&mut out);
    }
    assert_eq!(channel.skipped(), 0);
    out.events
        .iter()
        .filter(|event| event.kind == "msk144")
        .filter_map(sdrmm_wire::DecoderEvent::payload)
        .collect()
}

pub(crate) fn heard() -> Heard {
    let mut audio = noise(3 * BLOCK, 0.1, 3);
    let wave = waveform(pack("CQ K1JT FN20").expect("packs"), 1_500.0, 4).expect("encodes");
    for (sample, value) in audio[BLOCK..].iter_mut().zip(&wave) {
        *sample += 0.3 * value;
    }
    let mut channel = channel();
    let mut out = ChannelOutputs::default();
    let iq: Vec<Complex<f32>> = audio
        .iter()
        .map(|&sample| Complex::new(sample, 0.0))
        .collect();
    for hop in iq.chunks(BLOCK_STEP) {
        channel.process(hop, &mut out);
        channel.settle(&mut out);
    }
    Heard::from_events(&out.events)
}

fn at(utc: &str, seconds: f64) -> SystemTime {
    let start: jiff::Timestamp = utc.parse().expect("a time");
    SystemTime::from(start) + Duration::from_secs_f64(seconds)
}

#[test]
fn recorded_pings_read_as_wsjtx_reads_them() {
    let first = hear(
        &recording("msk144_wa4cqg_12k"),
        Some(at("2018-12-11T12:05:00Z", FIRST_START_S)),
    );
    let second = hear(
        &recording("msk144_w4imd_kd9vv_12k"),
        Some(at("2018-12-11T12:08:00Z", SECOND_START_S)),
    );
    let read: Vec<(&str, i32, i32, &str)> = first
        .iter()
        .chain(&second)
        .map(|message| {
            (
                message.text.as_str(),
                (message.time_offset_s * 10.0).round() as i32,
                message.audio_hz.round() as i32,
                message.slot.as_deref().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            ("K1JT WA4CQG EM72", 87, 1488, "2018-12-11T12:05:00Z"),
            ("CQ W4IMD EM84", 46, 1455, "2018-12-11T12:08:00Z"),
            ("CQ KD9VV EN71", 122, 1496, "2018-12-11T12:08:00Z"),
        ],
        "{first:?} {second:?}"
    );
    assert!((first[0].snr_db - 8.0).abs() <= 2.0, "{}", first[0].snr_db);
}

const FIRST_START_S: f64 = 6.0;
const SECOND_START_S: f64 = 2.0;
