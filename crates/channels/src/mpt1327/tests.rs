use sdrmm_decoder_test_support::frames::decode;
use sdrmm_decoder_test_support::{complex_noise, settings};
use sdrmm_wire::{ChannelParams, DvFrameKind};

use super::message::Address;
use super::*;

mod air;

fn channel() -> Mpt1327Channel {
    Mpt1327Channel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::of(&Mpt1327Params::default()).expect("mpt1327 settings")),
    )
    .expect("mpt1327 channel")
}

fn opcodes(frames: &[DvFrame]) -> Vec<&str> {
    frames
        .iter()
        .filter_map(|frame| frame.opcode.as_deref())
        .collect()
}

#[test]
fn a_control_channel_reads_back_every_slot() {
    let iq = synth::control_channel(synth::SITE_SYSTEM, &synth::site_messages(), RATE);
    let frames = decode(&mut channel(), &iq);
    assert_eq!(opcodes(&frames), ["ALH", "BCAST", "GTC"]);
    let grant = &frames[2];
    assert_eq!(grant.kind, DvFrameKind::Control);
    assert_eq!(grant.channel, Some(123));
    assert_eq!(grant.destination, Some(10 * 8192 + 4105));
    assert_eq!(grant.source, Some(10 * 8192 + 77));
    assert_eq!(grant.system_id, Some(synth::SITE_SYSTEM));
    assert_eq!(grant.control_channel, Some(true));
    assert_eq!(grant.trunk_protocol.as_deref(), Some("mpt1327"));
    assert_eq!(grant.errors_corrected, 0);
}

#[test]
fn the_site_reads_through_noise() {
    let mut iq = synth::control_channel(synth::SITE_SYSTEM, &synth::site_messages(), RATE);
    let noise = complex_noise(7, 0.25, iq.len());
    for (sample, noise) in iq.iter_mut().zip(noise) {
        *sample += noise;
    }
    let frames = decode(&mut channel(), &iq);
    assert!(opcodes(&frames).contains(&"GTC"), "{:?}", opcodes(&frames));
}

#[test]
fn a_flipped_bit_in_a_grant_is_repaired() {
    let mut bits = synth::control_bits(
        synth::SITE_SYSTEM,
        &[synth::gtc(
            Address {
                prefix: 1,
                ident: 2,
            },
            3,
            44,
        )],
    );
    bits[48 + 20] ^= true;
    let frames = decode(&mut channel(), &synth::modulate(&bits, RATE));
    assert_eq!(opcodes(&frames), ["GTC"]);
    assert_eq!(frames[0].errors_corrected, 1);
    assert_eq!(frames[0].channel, Some(44));
}

#[test]
fn a_repeated_aloha_is_shown_once() {
    let iq = synth::control_channel(1, &[synth::aloha(1); 6], RATE);
    let frames = decode(&mut channel(), &iq);
    assert_eq!(opcodes(&frames), ["ALH"]);
}

#[test]
fn noise_alone_decodes_nothing() {
    let frames = decode(&mut channel(), &complex_noise(3, 0.5, 5 * RATE as usize));
    assert!(frames.is_empty(), "{:?}", opcodes(&frames));
}
