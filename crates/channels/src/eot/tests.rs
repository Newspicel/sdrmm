use sdrmm_wire::{EotArming, EotBattery, EotStatus, HotCommand, HotRequest};

use super::*;
use crate::{synth, testutil::settings};

pub(crate) fn status() -> EotStatus {
    EotStatus {
        message_type: 2,
        arming: EotArming::Normal,
        pressure_psig: 87,
        battery: EotBattery::Ok,
        battery_charge_pct: 80,
        valve_ok: true,
        confirmed: false,
        turbine: true,
        motion: true,
        marker_light: true,
        marker_battery_low: false,
        discretionary: false,
        chaining: 3,
    }
}

fn rear() -> synth::eot::Rear {
    synth::eot::Rear {
        unit_address: 23_456,
        status: status(),
    }
}

fn channel() -> EotChannel {
    EotChannel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::Eot(EotParams::default())),
    )
    .unwrap()
}

fn messages(iq: &[Complex<f32>]) -> Vec<EotMessage> {
    let mut channel = channel();
    let mut out = ChannelOutputs::default();
    for chunk in iq.chunks(991) {
        channel.process(chunk, &mut out);
    }
    out.events
        .into_iter()
        .filter_map(|event| match event {
            DecoderEvent::Eot(message) => Some(message),
            _ => None,
        })
        .collect()
}

fn bit_messages(bits: impl IntoIterator<Item = bool>) -> Vec<EotMessage> {
    let mut channel = channel();
    let mut out = ChannelOutputs::default();
    for bit in bits {
        channel.symbol(if bit { 1.0 } else { -1.0 }, &mut out);
    }
    out.events
        .into_iter()
        .filter_map(|event| match event {
            DecoderEvent::Eot(message) => Some(message),
            _ => None,
        })
        .collect()
}

#[test]
fn decodes_a_rear_unit_burst_once_despite_its_repeat() {
    let got = messages(&synth::eot::rear_transmission(&rear(), RATE));
    assert_eq!(
        got,
        [EotMessage {
            unit_address: 23_456,
            report: EotReport::Rear(status()),
            errors_corrected: 0,
            rejected: 0,
        }]
    );
}

#[test]
fn decodes_a_head_unit_emergency() {
    let head = synth::eot::Head {
        unit_address: 98_765,
        code: 0xAA,
    };
    let got = messages(&synth::eot::head_transmission(head, RATE));
    assert_eq!(
        got,
        [EotMessage {
            unit_address: 98_765,
            report: EotReport::Head(HotRequest {
                command: HotCommand::Emergency,
                code: 0xAA,
                copies: 3,
            }),
            errors_corrected: 0,
            rejected: 0,
        }]
    );
}

#[test]
fn decodes_through_noise_at_an_offset() {
    let mut iq = synth::eot::rear_transmission(&rear(), RATE);
    synth::shift(&mut iq, 400.0, RATE);
    synth::add_noise(&mut iq, 0xE07, 0.6);
    let got = messages(&iq);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].report, EotReport::Rear(status()));
}

#[test]
fn inverted_tones_still_decode() {
    let bits = synth::eot::rear_bits(&rear()).into_iter().map(|bit| !bit);
    let got = bit_messages(bits);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].unit_address, 23_456);
}

#[test]
fn a_broken_block_is_counted_not_emitted() {
    let mut bits = synth::eot::rear_bits(&rear());
    let half = bits.len() / 2;
    for at in [100, 110, 120] {
        bits[at] = !bits[at];
        bits[half + at] = !bits[half + at];
    }
    assert!(bit_messages(bits.iter().copied()).is_empty());
    let mut stream = bits;
    let mut next = rear();
    next.status.pressure_psig = 0;
    stream.extend(synth::eot::rear_bits(&next));
    let got = bit_messages(stream);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].rejected, 2);
}

#[test]
fn corrected_bits_are_reported() {
    let mut bits = synth::eot::rear_bits(&rear());
    let half = bits.len() / 2;
    bits.truncate(half);
    bits[90] = !bits[90];
    let got = bit_messages(bits);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].errors_corrected, 1);
}

#[test]
fn noise_alone_emits_nothing() {
    let mut iq = synth::silence(RATE as usize * 20);
    synth::add_noise(&mut iq, 0xBAD, 1.0);
    assert!(messages(&iq).is_empty());
}

fn weak_copies(sync_lost: bool) -> Vec<EotMessage> {
    let bits = synth::eot::rear_bits(&rear());
    let half = bits.len() / 2;
    let mut symbols = bits
        .iter()
        .map(|&bit| if bit { 1.0 } else { -1.0 })
        .collect::<Vec<f32>>();
    for (copy, positions) in [(0, [90, 100, 110]), (half, [95, 105, 115])] {
        for at in positions {
            symbols[copy + at] *= -0.2;
        }
    }
    if sync_lost {
        for at in [half + 70, half + 72, half + 74] {
            symbols[at] = -symbols[at];
        }
    }
    symbols.extend([-1.0; 200]);
    let mut channel = channel();
    let mut out = ChannelOutputs::default();
    for symbol in symbols {
        channel.symbol(symbol, &mut out);
    }
    out.events
        .into_iter()
        .filter_map(|event| match event {
            DecoderEvent::Eot(message) => Some(message),
            _ => None,
        })
        .collect()
}

#[test]
fn two_weak_copies_add_up_to_one_good_frame() {
    let got = weak_copies(false);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].report, EotReport::Rear(status()));
    assert_eq!(got[0].rejected, 0);
}

#[test]
fn a_repeat_that_never_syncs_still_joins_the_first_copy() {
    let got = weak_copies(true);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].report, EotReport::Rear(status()));
}

fn wav_audio(wav: &[u8]) -> Vec<f32> {
    let data = wav
        .windows(4)
        .position(|tag| tag == b"data")
        .map_or(wav.len(), |at| at + 8);
    wav[data..]
        .iter()
        .map(|&byte| (f32::from(byte) - 128.0) / 128.0)
        .collect()
}

struct Heard {
    pressure_psig: u8,
    motion: bool,
    marker_light: bool,
    turbine: bool,
    battery_charge_pct: u8,
}

fn off_air(heard: Heard) -> EotStatus {
    EotStatus {
        message_type: 0,
        arming: EotArming::Normal,
        pressure_psig: heard.pressure_psig,
        battery: EotBattery::Ok,
        battery_charge_pct: heard.battery_charge_pct,
        valve_ok: true,
        confirmed: false,
        turbine: heard.turbine,
        motion: heard.motion,
        marker_light: heard.marker_light,
        marker_battery_low: true,
        discretionary: false,
        chaining: 3,
    }
}

#[test]
fn decodes_three_rear_units_from_an_off_air_recording() {
    let audio = wav_audio(include_bytes!(
        "../../../../fixtures/eot/pyeot_demo3_48k.wav"
    ));
    let peak = audio
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    let audio = audio.iter().map(|sample| sample / peak).collect::<Vec<_>>();
    let got = messages(&synth::fm_modulate(&audio, DEVIATION_HZ, RATE));
    let reports = got
        .iter()
        .map(|message| {
            (
                message.unit_address,
                message.report.clone(),
                message.errors_corrected,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reports,
        [
            (
                39_572,
                EotReport::Rear(off_air(Heard {
                    pressure_psig: 89,
                    motion: false,
                    marker_light: true,
                    turbine: true,
                    battery_charge_pct: 83,
                })),
                0
            ),
            (
                10_690,
                EotReport::Rear(off_air(Heard {
                    pressure_psig: 66,
                    motion: false,
                    marker_light: false,
                    turbine: false,
                    battery_charge_pct: 91,
                })),
                0
            ),
            (
                19_472,
                EotReport::Rear(off_air(Heard {
                    pressure_psig: 89,
                    motion: true,
                    marker_light: false,
                    turbine: true,
                    battery_charge_pct: 71,
                })),
                0
            ),
        ]
    );
}
