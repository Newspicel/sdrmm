use sdrmm_wire::{SarsatBeacon, SarsatMessage, SarsatProtocol};

use super::*;
use crate::{
    synth::{self, sarsat::EpirbFix},
    testutil::settings,
};

const SPEC_SHORT: &str = "56E6804002202009655250";

fn fix() -> EpirbFix {
    EpirbFix {
        country: 366,
        mmsi_tail: 123_456,
        lat: 37.8199,
        lon: -122.4783,
    }
}

fn channel() -> SarsatChannel {
    SarsatChannel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::Sarsat(SarsatParams::default())),
    )
    .unwrap()
}

fn run(iq: &[Complex<f32>]) -> (Vec<SarsatMessage>, u32) {
    let mut channel = channel();
    let mut out = ChannelOutputs::default();
    for chunk in iq.chunks(997) {
        channel.process(chunk, &mut out);
    }
    let messages = out
        .events
        .into_iter()
        .filter_map(|event| match event {
            DecoderEvent::Sarsat(message) => Some(message),
            _ => None,
        })
        .collect();
    (messages, channel.rejected())
}

fn close(got: Option<f64>, want: f64) {
    let got = got.expect("position");
    assert!((got - want).abs() <= 1.0 / 900.0, "{got} vs {want}");
}

#[test]
fn decodes_a_located_epirb_off_centre_in_noise() {
    let mut iq = synth::sarsat::transmission(&synth::sarsat::epirb_location(fix()), false, RATE);
    synth::shift(&mut iq, 3_170.0, RATE);
    synth::add_noise(&mut iq, 0x406, 1.0);
    let (got, rejected) = run(&iq);
    assert_eq!(got.len(), 1, "{got:?}");
    let message = &got[0];
    assert_eq!(message.protocol, SarsatProtocol::StandardLocation);
    assert_eq!(message.beacon, SarsatBeacon::Epirb);
    assert_eq!(message.identity.as_deref(), Some("MMSI 366123456"));
    assert_eq!(message.pdf2_ok, Some(true));
    assert!(!message.self_test);
    close(message.lat, 37.8199);
    close(message.lon, -122.4783);
    assert_eq!(rejected, 0);
}

#[test]
fn decodes_the_published_short_message_as_a_self_test() {
    let message = synth::sarsat::from_hex(SPEC_SHORT).expect("hex");
    let mut iq = synth::sarsat::transmission(&message, true, RATE);
    synth::shift(&mut iq, -6_250.0, RATE);
    let (got, _) = run(&iq);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].hex_id, "ADCD00800440401");
    assert_eq!(got[0].message, SPEC_SHORT);
    assert!(got[0].self_test);
    assert!(!got[0].long);
}

#[test]
fn an_inverted_spectrum_still_decodes() {
    let mut iq = synth::sarsat::transmission(&synth::sarsat::epirb_location(fix()), false, RATE);
    iq.iter_mut().for_each(|sample| *sample = sample.conj());
    synth::shift(&mut iq, 1_000.0, RATE);
    let (got, _) = run(&iq);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].identity.as_deref(), Some("MMSI 366123456"));
}

#[test]
fn consecutive_bursts_each_decode() {
    let message = synth::sarsat::epirb_location(fix());
    let mut iq = synth::sarsat::transmission(&message, false, RATE);
    iq.extend(synth::silence(RATE as usize));
    iq.extend(synth::sarsat::transmission(&message, false, RATE));
    synth::add_noise(&mut iq, 0xB0A7, 0.05);
    assert_eq!(run(&iq).0.len(), 2);
}

#[test]
fn a_broken_first_field_is_counted_not_emitted() {
    let mut message = synth::sarsat::epirb_location(fix());
    for at in [3, 17, 29, 41, 55] {
        message[at] = !message[at];
    }
    let iq = synth::sarsat::transmission(&message, false, RATE);
    let (got, rejected) = run(&iq);
    assert!(got.is_empty(), "{got:?}");
    assert_eq!(rejected, 1);
}

#[test]
fn a_broken_second_field_is_flagged_with_the_coarse_fix() {
    let mut message = synth::sarsat::epirb_location(fix());
    for at in [90, 97, 104] {
        message[at] = !message[at];
    }
    let iq = synth::sarsat::transmission(&message, false, RATE);
    let (got, rejected) = run(&iq);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].pdf2_ok, Some(false));
    close(got[0].lat, 37.75);
    close(got[0].lon, -122.5);
    assert_eq!(rejected, 0);
}

#[test]
fn noise_and_a_steady_carrier_emit_and_reject_nothing() {
    let mut iq = synth::silence(RATE as usize * 5);
    for (k, sample) in iq.iter_mut().enumerate() {
        *sample = Complex::from_polar(0.3, (k as f32 * 0.37).rem_euclid(std::f32::consts::TAU));
    }
    synth::add_noise(&mut iq, 0xBAD, 1.0);
    assert_eq!(run(&iq), (Vec::new(), 0));
}

#[test]
fn the_bandwidth_is_checked() {
    let params = SarsatParams {
        bandwidth_hz: 30_000.0,
    };
    let built = SarsatChannel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::Sarsat(params)),
    );
    assert!(built.is_err());
}
