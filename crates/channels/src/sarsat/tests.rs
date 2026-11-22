use crate::sarsat::SarsatBeacon;
use crate::sarsat::SarsatMessage;
use crate::sarsat::SarsatProtocol;
use sdrmm_wire::ChannelParams;

use super::*;
use crate::synth::{self, sarsat::EpirbFix};
use sdrmm_decoder_test_support::settings;

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
        settings(ChannelParams::new("sarsat", &SarsatParams::default()).expect("sarsat settings")),
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
        .filter_map(|event| event.read::<SarsatMessage>())
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
    sdrmm_modem::synth::shift(&mut iq, 3_170.0, RATE);
    sdrmm_modem::synth::add_noise(&mut iq, 0x406, 1.0);
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
    sdrmm_modem::synth::shift(&mut iq, -6_250.0, RATE);
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
    for sample in &mut iq {
        *sample = sample.conj();
    }
    sdrmm_modem::synth::shift(&mut iq, 1_000.0, RATE);
    let (got, _) = run(&iq);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].identity.as_deref(), Some("MMSI 366123456"));
}

#[test]
fn consecutive_bursts_each_decode() {
    let message = synth::sarsat::epirb_location(fix());
    let mut iq = synth::sarsat::transmission(&message, false, RATE);
    iq.extend(sdrmm_modem::synth::silence(RATE as usize));
    iq.extend(synth::sarsat::transmission(&message, false, RATE));
    sdrmm_modem::synth::add_noise(&mut iq, 0xB0A7, 0.05);
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
    let mut iq = sdrmm_modem::synth::silence(RATE as usize * 5);
    for (k, sample) in iq.iter_mut().enumerate() {
        *sample = Complex::from_polar(0.3, (k as f32 * 0.37).rem_euclid(std::f32::consts::TAU));
    }
    sdrmm_modem::synth::add_noise(&mut iq, 0xBAD, 1.0);
    assert_eq!(run(&iq), (Vec::new(), 0));
}

#[test]
fn the_bandwidth_is_checked() {
    let params = SarsatParams {
        bandwidth_hz: 30_000.0,
    };
    let built = SarsatChannel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::of(&params).expect("settings")),
    );
    assert!(built.is_err());
}

#[test]
fn the_scope_sees_the_two_chip_phases() {
    let mut burst = sdrmm_modem::synth::silence(RATE as usize / 2);
    burst.extend(synth::sarsat::burst(
        &synth::sarsat::epirb_location(fix()),
        false,
        RATE,
    ));
    sdrmm_modem::synth::shift(&mut burst, 2_000.0, RATE);
    let iq = burst;
    let mut chan = channel();
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(true);
    let mut tap = sdrmm_decoder::SymbolTap::default();
    for block in iq.as_chunks::<997>().0 {
        out.reset();
        chan.process(block, &mut out);
        if out.symbols.plane.is_some() {
            tap = std::mem::take(&mut out.symbols);
            out.symbols.set_wanted(true);
        }
    }
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Level));
    assert_eq!(tap.reference.len(), 2);
    assert!(tap.symbols.len() >= 20, "{} values", tap.symbols.len());
    assert_eq!(tap.symbol_rate, CHIP_RATE);
    assert!(
        (tap.freq_error_hz - 2_000.0).abs() < 20.0,
        "{} Hz",
        tap.freq_error_hz
    );
    assert!(tap.evm < 0.15, "EVM {}", tap.evm);
}

fn locks(iq: &[Complex<f32>]) -> (Vec<f64>, usize) {
    let mut chan = channel();
    let mut out = ChannelOutputs::default();
    let mut carriers = Vec::new();
    let mut tracking = false;
    for chunk in iq.chunks(997) {
        chan.process(chunk, &mut out);
        let now = matches!(chan.state, State::Tracking { .. });
        if now && !tracking {
            carriers.push(chan.carrier_hz);
        }
        tracking = now;
    }
    (carriers, out.events.len())
}

fn shifted(hz: f64) -> Vec<Complex<f32>> {
    let mut iq = synth::sarsat::transmission(&synth::sarsat::epirb_location(fix()), false, RATE);
    sdrmm_modem::synth::shift(&mut iq, hz, RATE);
    iq
}

#[test]
fn the_end_of_a_burst_is_not_a_new_carrier() {
    for noise in [0.0, 0.01] {
        let mut iq = shifted(2_000.0);
        iq.extend(sdrmm_modem::synth::silence(RATE as usize * 2));
        if noise > 0.0 {
            sdrmm_modem::synth::add_noise(&mut iq, 0x5A75, noise);
        }
        let (carriers, decoded) = locks(&iq);
        assert_eq!(decoded, 1, "noise {noise}");
        assert_eq!(carriers.len(), 1, "noise {noise}: {carriers:?}");
    }
}

#[test]
fn back_to_back_bursts_each_lock_once() {
    for second_hz in [2_000.0, -1_500.0] {
        let mut iq = shifted(2_000.0);
        iq.extend(shifted(second_hz));
        sdrmm_modem::synth::add_noise(&mut iq, 0xB2B, 0.01);
        let (carriers, decoded) = locks(&iq);
        assert_eq!(decoded, 2, "second at {second_hz} Hz");
        assert_eq!(carriers.len(), 2, "{carriers:?}");
        assert!((carriers[1] - second_hz).abs() < 20.0, "{carriers:?}");
    }
}
