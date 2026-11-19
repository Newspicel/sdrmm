use super::*;
use crate::weak_signal::fixture::recording;
use crate::weak_signal::{Fst4Channel, Fst4Params, Fst4wChannel, Fst4wParams, Fst4wSpot};
use sdrmm_decoder::{Channel, ChannelCtx, ChannelOutputs, NativeChannel};
use sdrmm_decoder_test_support::settings;
use sdrmm_wire::{ChannelParams, DecoderEvent};

const RATE: usize = 12_000;

struct Noise(u64);

impl Noise {
    fn uniform(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64).max(1e-12)
    }

    fn gaussian(&mut self) -> f32 {
        let (u, v) = (self.uniform(), self.uniform());
        ((-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()) as f32
    }
}

/// One period with `text` starting `start_s` in at `snr_db` in 2500 Hz of white noise.
fn period(setup: Fst4Setup, text: &str, audio_hz: f64, start_s: f32, snr_db: f32) -> Vec<f32> {
    const SIGMA: f32 = 0.05;
    let mut noise = Noise(0x5eed ^ u64::from(setup.period_s));
    let mut audio: Vec<f32> = (0..setup.period_s as usize * RATE)
        .map(|_| SIGMA * noise.gaussian())
        .collect();
    let amplitude = SIGMA * (2.0 * 10f32.powf(snr_db / 10.0) * 2_500.0 / 6_000.0).sqrt();
    let wave = waveform(setup, text, audio_hz).expect("a waveform");
    let start = (start_s * RATE as f32) as usize;
    for (sample, value) in audio[start..].iter_mut().zip(&wave) {
        *sample += amplitude * value;
    }
    audio
}

fn decode(setup: Fst4Setup, audio: &[f32], low_hz: f32, high_hz: f32) -> Vec<Fst4Found> {
    Fst4Decoder::new(setup).decode(audio, low_hz, high_hz, 200)
}

#[test]
fn a_transmission_lasts_its_160_symbols() {
    let fst4 = |period_s| Fst4Setup {
        period_s,
        wspr: false,
    };
    assert!((tx_seconds(fst4(15)) - 9.6).abs() < 1e-4);
    assert!((tx_seconds(fst4(60)) - 51.84).abs() < 1e-3);
    assert!((tx_seconds(fst4(300)) - 286.72).abs() < 1e-2);
    assert_eq!(nominal_start_s(fst4(15)), 0.5);
    assert_eq!(nominal_start_s(fst4(120)), 1.0);
    assert!(
        !Fst4Setup {
            period_s: 60,
            wspr: true
        }
        .is_supported()
    );
    assert!(!fst4(600).is_supported());
    assert!(
        Fst4Setup {
            period_s: 1_800,
            wspr: true
        }
        .is_supported()
    );
    assert_eq!(
        waveform(fst4(15), "CQ K1ABC FN42", 1_000.0).map(|wave| wave.len()),
        Some(160 * 720)
    );
}

#[test]
fn the_tones_carry_the_sync_words() {
    let code = Code::new(Payload::Standard);
    let codeword = code.encode(&[false; 77]);
    let tones = tones(&codeword);
    assert_eq!(&tones[..8], &SYNC_A);
    assert_eq!(&tones[38..46], &SYNC_B);
    assert_eq!(&tones[152..], &SYNC_A);
}

#[test]
fn fst4_15_decodes_a_weak_call_anywhere_in_the_window() {
    let setup = Fst4Setup {
        period_s: 15,
        wspr: false,
    };
    for start_s in [0.1, 0.5, 3.9] {
        let audio = period(setup, "CQ K1ABC FN42", 1_200.0, start_s, -14.0);
        let found = decode(setup, &audio, 200.0, 3_000.0);
        assert_eq!(found.len(), 1, "{start_s} s: {found:?}");
        let found = &found[0];
        assert_eq!(found.text, "CQ K1ABC FN42");
        assert!((found.frequency_hz - 1_200.0).abs() < 2.0, "{found:?}");
        assert!((found.start_s - start_s).abs() < 0.05, "{found:?}");
        assert!((found.snr_db + 14.0).abs() < 3.0, "{found:?}");
    }
}

#[test]
fn fst4_60_decodes_a_call_below_the_noise() {
    let setup = Fst4Setup {
        period_s: 60,
        wspr: false,
    };
    let audio = period(setup, "K1ABC W9XYZ -17", 1_500.0, 4.0, -24.0);
    let found = decode(setup, &audio, 1_400.0, 1_600.0);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].text, "K1ABC W9XYZ -17");
    assert!((found[0].start_s - 4.0).abs() < 0.1, "{found:?}");
}

#[test]
fn fst4w_120_decodes_a_beacon() {
    let setup = Fst4Setup {
        period_s: 120,
        wspr: true,
    };
    let audio = period(setup, "DL0HOT JO60 30", 1_500.0, 1.0, -28.0);
    let found = decode(setup, &audio, 1_400.0, 1_600.0);
    assert_eq!(found.len(), 1, "{found:?}");
    let beacon = found[0].beacon.as_ref().expect("a beacon");
    assert_eq!(found[0].text, "DL0HOT JO60 30");
    assert_eq!(beacon.callsign, "DL0HOT");
    assert_eq!(beacon.grid.as_deref(), Some("JO60"));
    assert_eq!(beacon.power_dbm, Some(30));
    assert!((found[0].frequency_hz - 1_500.0).abs() < 1.0, "{found:?}");
}

#[test]
fn noise_alone_decodes_to_nothing() {
    for (period_s, wspr, seeds) in [(15, false, 1..9), (120, true, 1..3)] {
        let setup = Fst4Setup { period_s, wspr };
        for seed in seeds {
            let mut noise = Noise(seed);
            let audio: Vec<f32> = (0..period_s as usize * RATE)
                .map(|_| 0.05 * noise.gaussian())
                .collect();
            let found = decode(setup, &audio, 200.0, 3_000.0);
            assert!(found.is_empty(), "{period_s} s seed {seed}: {found:?}");
        }
    }
}

#[test]
fn an_off_air_fst4_60_period_reads_its_three_calls() {
    let setup = Fst4Setup {
        period_s: 60,
        wspr: false,
    };
    let audio = recording("fst4_60_630m_12k");
    let found = decode(setup, &audio, 200.0, 3_000.0);
    let heard: Vec<(&str, f32)> = found
        .iter()
        .map(|found| (found.text.as_str(), found.frequency_hz))
        .collect();
    for (text, hz) in [
        ("CQ K9KFR EN71", 1_331.0),
        ("CQ N5TM EL29", 1_101.0),
        ("CQ N3HSK FM28", 1_135.0),
    ] {
        assert!(
            heard
                .iter()
                .any(|&(heard, at)| heard == text && (at - hz).abs() < 2.0),
            "{text} missing from {found:?}"
        );
    }
    assert_eq!(found.len(), 3, "{found:?}");
}

/// The complex samples of a committed fixture.
fn baseband(stem: &str) -> Vec<Complex<f32>> {
    let bytes = std::fs::read(format!(
        "{}/../../fixtures/{stem}.sigmf-data",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture");
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|sample| {
            Complex::new(
                f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]),
                f32::from_le_bytes([sample[4], sample[5], sample[6], sample[7]]),
            )
        })
        .collect()
}

#[test]
fn an_off_air_fst4w_1800_period_reads_its_beacon_from_narrowed_audio() {
    const CENTER_HZ: f32 = 1_450.0;
    let setup = Fst4Setup {
        period_s: 1_800,
        wspr: true,
    };
    let rate = setup.stream_rate();
    let mut stream = Vec::new();
    Narrow::new(rate, 1, 0.0).process(&baseband("fst4w_1800_750"), &mut stream);
    let shift = narrow::shift_hz(rate as f32, CENTER_HZ);
    let found = decode(setup, &stream, 1_400.0 - shift, 1_500.0 - shift);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].text, "DL0HOT JO60 30");
    assert!(
        (found[0].frequency_hz + shift - 1_433.0).abs() < 0.1,
        "{found:?}"
    );
    assert!((found[0].snr_db + 44.0).abs() < 1.5, "{found:?}");
}

const CTX: ChannelCtx = ChannelCtx {
    input_rate: 12_000.0,
};

#[test]
fn the_fst4_channel_reads_the_recorded_period_and_says_which_call_needed_cq_assumed() {
    let params = ChannelParams::of(&Fst4Params::default()).expect("fst4 settings");
    let mut channel = Fst4Channel::new(CTX, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    for block in recording("fst4_60_630m_12k").chunks(4_096) {
        let iq: Vec<Complex<f32>> = block.iter().map(|&s| Complex::new(s, 0.0)).collect();
        channel.process(&iq, &mut out);
    }
    channel.settle(&mut out);
    let mut heard: Vec<Fst4Message> = out
        .events
        .iter()
        .filter_map(DecoderEvent::read::<Fst4Message>)
        .collect();
    heard.sort_by(|a, b| a.text.cmp(&b.text));
    let read: Vec<(&str, bool)> = heard
        .iter()
        .map(|message| (message.text.as_str(), message.cq_assumed))
        .collect();
    assert_eq!(
        read,
        [
            ("CQ K9KFR EN71", false),
            ("CQ N3HSK FM28", true),
            ("CQ N5TM EL29", false),
        ]
    );
    for (message, hz) in heard.iter().zip([1_331.0, 1_135.0, 1_101.0]) {
        assert!((message.audio_hz - hz).abs() < 2.0, "{message:?}");
    }
}

#[test]
fn a_long_period_is_narrowed_and_still_reads_where_the_beacon_was() {
    let setup = Fst4wParams {
        period: Fst4wPeriod::S300,
        ..Fst4wParams::default()
    };
    let audio = period(setup.setup(), "K1ABC FN42 37", 1_520.0, 1.0, -26.0);
    let params = ChannelParams::of(&setup).expect("fst4w settings");
    let mut channel = Fst4wChannel::new(CTX, settings(params)).expect("builds");
    assert_eq!(channel.0.audio.capacity(), 2 * 898_560);
    let mut out = ChannelOutputs::default();
    let iq: Vec<Complex<f32>> = audio.iter().map(|&s| Complex::new(s, 0.0)).collect();
    for block in iq.chunks(4_096) {
        channel.process(block, &mut out);
    }
    channel.settle(&mut out);
    let spots: Vec<Fst4wSpot> = out
        .events
        .iter()
        .filter_map(DecoderEvent::read::<Fst4wSpot>)
        .collect();
    assert_eq!(spots.len(), 1, "{spots:?}");
    assert_eq!(spots[0].callsign, "K1ABC");
    assert_eq!(spots[0].power_dbm, Some(37));
    assert!((spots[0].audio_hz - 1_520.0).abs() < 0.5, "{spots:?}");
    assert!((spots[0].time_offset_s).abs() < 0.2, "{spots:?}");
}

#[test]
fn a_search_too_wide_for_a_narrowed_period_is_refused() {
    let params = Fst4Params {
        period: Fst4Period::S1800,
        ..Fst4Params::default()
    };
    let params = ChannelParams::of(&params).expect("fst4 settings");
    assert!(Fst4Channel::new(CTX, settings(params)).is_err());
}

#[test]
fn every_hop_is_shorter_than_the_slack_a_window_leaves() {
    for period_s in [15, 30, 60, 120, 300, 900, 1_800] {
        let setup = Fst4Setup {
            period_s,
            wspr: false,
        };
        let rate = setup.stream_rate() as f32;
        let window_s = setup.slot_samples() as f32 / rate;
        let hop_s = setup.hop_samples() as f32 / rate;
        assert!(
            hop_s < window_s - tx_seconds(setup),
            "{period_s} s: {hop_s} s hops, {window_s} s windows"
        );
    }
}
