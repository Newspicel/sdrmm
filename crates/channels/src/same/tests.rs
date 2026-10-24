use sdrmm_wire::{ChannelParams, DecoderEvent, SameBody, SameLocation, SameMessage, SameParams};

use super::*;
use crate::{synth, testutil::settings};

const TORNADO: &str = "ZCZC-WXR-TOR-048201-048157+0030-1232200-KEC55/NW-";

fn channel() -> SameChannel {
    let params = ChannelParams::Same(SameParams::default());
    SameChannel::new(ChannelCtx { input_rate: RATE }, settings(params)).expect("opens")
}

fn messages(iq: &[Complex<f32>]) -> (Vec<SameMessage>, u32) {
    let mut channel = channel();
    let mut out = ChannelOutputs::default();
    for block in iq.chunks(4_096) {
        channel.process(block, &mut out);
    }
    let decoded = out
        .events
        .iter()
        .filter_map(|event| match event {
            DecoderEvent::Same(message) => Some(message.clone()),
            _ => None,
        })
        .collect();
    (decoded, channel.rejected())
}

#[test]
fn a_bit_is_exactly_twenty_four_samples_of_the_audio_rate() {
    assert_eq!(SAMPLES_PER_BIT, 24);
    assert_eq!(AUDIO_RATE, 12_500.0);
}

#[test]
fn three_matching_bursts_decode_to_one_alert() {
    let (decoded, rejected) = messages(&synth::same::alert(TORNADO, RATE));
    assert_eq!(decoded.len(), 1);
    assert_eq!(rejected, 0);
    let message = &decoded[0];
    assert_eq!(message.raw, TORNADO);
    assert_eq!(message.copies, 2);
    let SameBody::Alert(alert) = &message.body else {
        panic!("expected an alert");
    };
    assert_eq!(alert.event, "TOR");
    assert_eq!(alert.station, "KEC55/NW");
    assert_eq!(
        alert.locations,
        vec![
            SameLocation {
                part: 0,
                state: 48,
                county: 201,
                name: Some("Harris, TX".to_owned()),
            },
            SameLocation {
                part: 0,
                state: 48,
                county: 157,
                name: Some("Fort Bend, TX".to_owned()),
            },
        ]
    );
}

#[test]
fn two_bursts_are_enough_to_settle_a_header() {
    let (decoded, _) = messages(&synth::same::transmission(TORNADO, 2, RATE));
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].copies, 2);
}

#[test]
fn the_end_of_message_burst_decodes_on_its_own() {
    let (decoded, rejected) = messages(&synth::same::alert("NNNN", RATE));
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].body, SameBody::EndOfMessage);
    assert_eq!(rejected, 0);
}

#[test]
fn an_alert_survives_noise() {
    let mut iq = synth::same::alert(TORNADO, RATE);
    synth::add_noise(&mut iq, 7, 0.08);
    let (decoded, _) = messages(&iq);
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].raw, TORNADO);
}

#[test]
fn a_lone_burst_never_settles_and_is_counted_as_rejected() {
    let mut iq = synth::same::transmission(TORNADO, 1, RATE);
    iq.extend(synth::silence(21 * RATE as usize));
    let (decoded, rejected) = messages(&iq);
    assert!(decoded.is_empty());
    assert_eq!(rejected, 1);
}

#[test]
fn two_alerts_in_a_row_both_reach_the_log() {
    let other = "ZCZC-WXR-SVR-048201+0045-1232300-KEC55/NW-";
    let mut iq = synth::same::alert(TORNADO, RATE);
    iq.extend(synth::same::alert(other, RATE));
    let (decoded, rejected) = messages(&iq);
    assert_eq!(
        decoded.iter().map(|m| m.raw.clone()).collect::<Vec<_>>(),
        vec![TORNADO.to_owned(), other.to_owned()]
    );
    assert_eq!(rejected, 0);
}

#[test]
fn silence_decodes_to_nothing() {
    let (decoded, rejected) = messages(&synth::silence(RATE as usize));
    assert!(decoded.is_empty());
    assert_eq!(rejected, 0);
}

#[test]
fn a_bandwidth_outside_the_channel_is_refused() {
    let params = SameParams {
        bandwidth_hz: 1_000.0,
    };
    assert!(channel_filter(&params).is_err());
    assert!(
        SameChannel::new(
            ChannelCtx { input_rate: RATE },
            settings(ChannelParams::Same(params))
        )
        .is_err()
    );
}

#[test]
fn the_occupied_band_is_the_bandwidth_around_the_carrier() {
    let params = SameParams {
        bandwidth_hz: 25_000.0,
    };
    assert_eq!(occupied_band(&params), (-12_500.0, 12_500.0));
}

#[test]
fn other_params_are_refused() {
    let settings = settings(ChannelParams::Nfm(sdrmm_wire::NfmParams::default()));
    assert!(SameChannel::new(ChannelCtx { input_rate: RATE }, settings).is_err());
}
