use std::time::{Duration, SystemTime};

use sdrmm_decoder::{Channel, ChannelCtx, ChannelOutputs, NativeChannel};
use sdrmm_decoder_test_support::settings;
use sdrmm_wire::ChannelParams;

use super::*;
use crate::weak_signal::fixture::recording;
use crate::weak_signal::ftx::message::pack;
use crate::weak_signal::{INPUT_RATE_HZ, Q65Channel, Q65Params, WsjtMessage};

const RATE: usize = 12_000;
const Q65_60A: Q65Setup = Q65Setup {
    period_s: 60,
    submode: 0,
};

/// White noise of unit power and `signal` added at `snr_db` in 2500 Hz.
fn in_noise(signal: &[f32], len: usize, offset: usize, snr_db: f32, mut state: u64) -> Vec<f32> {
    let mut uniform = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 11) as f64 / (1u64 << 53) as f64).max(1e-12)
    };
    let mut audio: Vec<f32> = (0..len)
        .map(|_| {
            let (u, v) = (uniform(), uniform());
            ((-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()) as f32
        })
        .collect();
    let amplitude = (2.0 * 10f32.powf(snr_db / 10.0) * 2_500.0 / 6_000.0).sqrt();
    for (sample, value) in audio[offset..].iter_mut().zip(signal) {
        *sample += amplitude * value;
    }
    audio
}

#[test]
fn payload_symbols_round_trip() {
    let payload = pack("CQ K1ABC FN42").expect("packs");
    assert_eq!(payload_of(&message_of(payload)), payload);
    assert_eq!(message_of(payload)[12] & 1, 0);
}

#[test]
fn a_waveform_has_85_symbols_with_sync_on_the_lowest_tone() {
    let payload = pack("CQ K1ABC FN42").expect("packs");
    let wave = waveform(Q65_60A, payload, 1_000.0);
    assert_eq!(wave.len(), 85 * 7_200);
    let tones = channel_tones(&qra::encode(&message_of(payload)));
    assert!(SYNC.iter().all(|&symbol| tones[symbol] == 0));
    assert!(
        !tones
            .iter()
            .enumerate()
            .any(|(symbol, &tone)| tone == 0 && !SYNC.contains(&symbol))
    );
}

#[test]
fn a_weak_signal_late_in_the_window_is_found_where_it_was_sent() {
    let text = "K1ABC W9XYZ EN37";
    let wave = waveform(Q65_60A, pack(text).expect("packs"), 1_234.0);
    let audio = in_noise(&wave, 60 * RATE, 7 * RATE + RATE / 4, -22.0, 7);
    let mut decoder = Q65Decoder::new(Q65_60A);
    let found = decoder.decode(&audio, 200.0, 3_000.0, 20);
    let heard = found
        .iter()
        .find(|found| found.text == text)
        .unwrap_or_else(|| {
            panic!(
                "{text} not heard in {:?}",
                found.iter().map(|f| &f.text).collect::<Vec<_>>()
            )
        });
    assert!(
        (heard.frequency_hz - 1_234.0).abs() < 1.0,
        "{}",
        heard.frequency_hz
    );
    assert!((heard.start_s - 7.25).abs() < 0.08, "{}", heard.start_s);
    assert!((heard.snr_db + 22.0).abs() < 3.0, "{}", heard.snr_db);
    assert_eq!(found.len(), 1);
}

#[test]
fn every_period_and_tone_spacing_decodes_its_own_signal() {
    for (period_s, submode) in [(15, 0), (30, 1), (120, 3)] {
        let setup = Q65Setup { period_s, submode };
        let text = "CQ DL1ABC JO62";
        let wave = waveform(setup, pack(text).expect("packs"), 900.0);
        let start = (setup.nominal_start_s() * RATE as f32) as usize;
        let audio = in_noise(
            &wave,
            period_s as usize * RATE,
            start,
            -15.0,
            u64::from(period_s),
        );
        let found = Q65Decoder::new(setup).decode(&audio, 200.0, 3_000.0, 20);
        assert!(
            found
                .iter()
                .any(|f| f.text == text && (f.frequency_hz - 900.0).abs() < 2.0),
            "Q65-{period_s}{}: {:?}",
            char::from(b'A' + submode),
            found
                .iter()
                .map(|f| (&f.text, f.frequency_hz))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn noise_alone_decodes_to_nothing() {
    let audio = in_noise(&[], 60 * RATE, 0, 0.0, 11);
    let mut decoder = Q65Decoder::new(Q65_60A);
    assert!(decoder.decode(&audio, 200.0, 3_000.0, 20).is_empty());
}

/// The 12 kHz samples of the off-air Q65-60A recording, scaled to full scale.
fn off_air() -> Vec<f32> {
    recording("q65_60a_eme_12k")
}

#[test]
fn an_off_air_eme_period_reads_what_wsjt_x_published() {
    let audio = off_air();
    let mut decoder = Q65Decoder::new(Q65_60A);
    let found = decoder.decode(&audio, 200.0, 3_000.0, 20);
    for (text, hz) in [
        ("W7GJ W1VD FN31", 1_420.0),
        ("W7GJ VE1JF RRR", 1_620.0),
        ("W7GJ N8JX EN73", 697.0),
        ("W7GJ N0TB -15", 943.0),
    ] {
        let heard = found.iter().find(|f| f.text == text);
        assert!(
            heard.is_some_and(|f| (f.frequency_hz - hz).abs() < 3.0),
            "{text} at {hz} Hz missing"
        );
    }
}

#[test]
fn the_channel_puts_an_eme_period_on_its_utc_slot() {
    let start: jiff::Timestamp = "2021-01-06T16:21:00Z".parse().expect("a time");
    let mut channel = Q65Channel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::of(&Q65Params::default()).expect("q65 settings")),
    )
    .expect("builds");
    channel.captured_at(SystemTime::from(start) + Duration::ZERO);
    let mut out = ChannelOutputs::default();
    let mut iq: Vec<Complex<f32>> = off_air()
        .into_iter()
        .map(|sample| Complex::new(sample, 0.0))
        .collect();
    iq.resize(iq.len() + 4 * RATE, Complex::default());
    for hop in iq.chunks(Q65_60A.hop_samples()) {
        channel.process(hop, &mut out);
        channel.settle(&mut out);
    }
    let mut read: Vec<(String, String, i32)> = out
        .events
        .iter()
        .filter(|event| event.kind == "q65")
        .filter_map(sdrmm_wire::DecoderEvent::payload::<WsjtMessage>)
        .map(|message| {
            (
                message.text,
                message.slot.unwrap_or_default(),
                (message.time_offset_s * 10.0).round() as i32,
            )
        })
        .collect();
    read.sort();
    let texts: Vec<&str> = read.iter().map(|(text, _, _)| text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "W7GJ N0TB -15",
            "W7GJ N8JX EN73",
            "W7GJ VE1JF RRR",
            "W7GJ W1VD FN31"
        ],
        "{read:?}"
    );
    assert!(
        read.iter()
            .all(|(_, slot, offset)| slot == "2021-01-06T16:21:00Z" && (25..=31).contains(offset)),
        "{read:?}"
    );
}

#[test]
fn every_hop_fits_inside_the_start_search() {
    for period_s in [15, 30, 60, 120, 300] {
        for submode in 0..=4 {
            let setup = Q65Setup { period_s, submode };
            let slack_s = period_s as f32 - setup.transmission_s() - setup.nominal_start_s();
            let hop_s = setup.hop_samples() as f32 / RATE as f32;
            assert!(
                hop_s <= slack_s + 0.5,
                "{period_s} s: hop {hop_s} s, slack {slack_s} s"
            );
        }
    }
}
