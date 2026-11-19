use num_complex::Complex;
use sdrmm_decoder::{Channel, ChannelCtx, ChannelOutputs, NativeChannel};
use sdrmm_decoder_test_support::settings;
use sdrmm_wire::ChannelParams;

use super::*;

fn decode(iq: &[Complex<f32>]) -> Vec<Js8Message> {
    let mut channel = Js8Channel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::new("js8", &WsjtParams::default()).expect("js8 settings")),
    )
    .unwrap();
    let mut out = ChannelOutputs::default();
    for block in iq.chunks(4_096) {
        channel.process(block, &mut out);
        channel.settle(&mut out);
    }
    out.events
        .iter()
        .filter_map(DecoderEvent::read::<Js8Message>)
        .collect()
}

#[test]
fn a_synthetic_heartbeat_decodes_to_its_station() {
    let payload = pack::heartbeat("KN4CRD", "EM73", false, 3);
    let heard = decode(&normal_slot(payload, 1_234.0));
    assert_eq!(heard.len(), 1, "{heard:?}");
    let message = &heard[0];
    assert_eq!(message.text, "KN4CRD: @HB HEARTBEAT EM73");
    assert_eq!(message.speed, "Normal");
    assert!((message.audio_hz - 1_234.0).abs() < 2.0);
    assert!(message.time_offset_s.abs() < 0.2);
}

const RATE: usize = 12_000;

fn fixture(bytes: &[u8], lead_s: usize) -> Vec<Complex<f32>> {
    let mut iq = vec![Complex::default(); lead_s * RATE];
    iq.extend(bytes.as_chunks::<8>().0.iter().map(|sample| {
        Complex::new(
            f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]),
            f32::from_le_bytes([sample[4], sample[5], sample[6], sample[7]]),
        )
    }));
    iq.resize(iq.len() + 8 * RATE, Complex::default());
    iq
}

fn texts(messages: &[Js8Message]) -> Vec<&str> {
    let mut texts: Vec<&str> = messages
        .iter()
        .map(|message| message.text.as_str())
        .collect();
    texts.sort_unstable();
    texts
}

fn well_formed(messages: &[Js8Message], speed: &str) {
    for message in messages {
        assert_eq!(message.frame.len(), 12, "{message:?}");
        assert!(
            message
                .frame
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'+'),
            "{message:?}"
        );
        assert_eq!(message.speed, speed);
        assert!(message.time_offset_s.abs() < 2.0, "{message:?}");
    }
}

/// `JS8Call`'s `A_2_5.wav`: its CLI reads five frames at depth 2.
#[test]
fn a_recorded_normal_slot_reads_heartbeat_replies() {
    let heard = decode(&fixture(
        include_bytes!("../../../../../fixtures/js8_normal_heartbeats_12k.sigmf-data"),
        0,
    ));
    well_formed(&heard, "Normal");
    assert_eq!(
        texts(&heard),
        [
            "K0OG: KN4CRD SNR +02",
            "KD8SKZ: KN4CRD HEARTBEAT SNR -14",
            "KG9B: KN4CRD HEARTBEAT SNR -14",
            "KN4ZXG: @HB HEARTBEAT FM16",
            "VA3QR: KN4CRD HEARTBEAT SNR -17",
        ]
    );
    let heartbeat = heard
        .iter()
        .find(|message| message.frame_type == "Heartbeat")
        .expect("a heartbeat");
    assert_eq!(heartbeat.callsign.as_deref(), Some("KN4ZXG"));
    assert_eq!(heartbeat.grid.as_deref(), Some("FM16"));
}

/// `JS8Call`'s `A_2_6.wav`: its CLI reads six frames at depth 2; this reads one more, every
/// reply addressed to the same station.
#[test]
fn a_recorded_normal_slot_reads_a_data_frame() {
    let heard = decode(&fixture(
        include_bytes!("../../../../../fixtures/js8_normal_data_12k.sigmf-data"),
        0,
    ));
    well_formed(&heard, "Normal");
    assert_eq!(
        texts(&heard),
        [
            "K3CHB: AC9BA HEARTBEAT SNR -03",
            "KB1CTC: AC9BA HEARTBEAT SNR -17",
            "KE2KQ: AC9BA HEARTBEAT SNR -06",
            "KX4XT: AC9BA HEARTBEAT SNR -04",
            "N0PHT: AC9BA HEARTBEAT SNR -11",
            "N3RTB: AC9BA HEARTBEAT SNR -13",
            "u3ipItc4eML+",
        ]
    );
    let data = heard
        .iter()
        .find(|message| message.frame_type == "Data")
        .expect("a data frame");
    assert_eq!(data.text, data.frame);
}

/// `JS8Call`'s `A_2_9.wav`: its CLI reads nine frames at depth 2; this reads ten, every one a
/// heartbeat reply to K0EIA.
#[test]
fn a_busy_recorded_normal_slot_reads_every_reply() {
    let heard = decode(&fixture(
        include_bytes!("../../../../../fixtures/js8_normal_busy_12k.sigmf-data"),
        0,
    ));
    well_formed(&heard, "Normal");
    assert_eq!(
        texts(&heard),
        [
            "AC9BA: K0EIA HEARTBEAT SNR -11",
            "K4BYN: K0EIA HEARTBEAT SNR +05",
            "K8KDS: K0EIA HEARTBEAT SNR -11",
            "KB1CTC: K0EIA HEARTBEAT SNR +06",
            "KE2KQ: K0EIA HEARTBEAT SNR +03",
            "KG9PL: K0EIA HEARTBEAT SNR -01",
            "KX4XT: K0EIA HEARTBEAT SNR -10",
            "N5RML: K0EIA HEARTBEAT SNR -13",
            "W9DRR: K0EIA HEARTBEAT SNR -05",
            "WP4OH: K0EIA HEARTBEAT SNR -03",
        ]
    );
}

/// `JS8Call`'s `E_2_1.wav`, one Slow data frame. Slow windows are searched every other hop, so
/// the recording is fed after four seconds of silence, as audio before it would sit in a stream.
#[test]
fn a_recorded_slow_period_reads_its_frame_on_the_utc_grid() {
    let iq = fixture(
        include_bytes!("../../../../../fixtures/js8_slow_12k.sigmf-data"),
        4,
    );
    let period: jiff::Timestamp = "2026-11-07T15:00:00Z".parse().unwrap();
    let captured = std::time::SystemTime::from(period) - std::time::Duration::from_secs(4);
    let mut channel = Js8Channel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::new("js8", &WsjtParams::default()).expect("js8 settings")),
    )
    .unwrap();
    channel.captured_at(captured);
    let mut out = ChannelOutputs::default();
    for block in iq.chunks(4_096) {
        channel.process(block, &mut out);
        channel.settle(&mut out);
    }
    let heard: Vec<Js8Message> = out
        .events
        .iter()
        .filter_map(DecoderEvent::read::<Js8Message>)
        .collect();
    well_formed(&heard, "Slow");
    assert_eq!(texts(&heard), ["TrMcT8++++++"]);
    assert_eq!(heard[0].slot.as_deref(), Some("2026-11-07T15:00:00Z"));
}
