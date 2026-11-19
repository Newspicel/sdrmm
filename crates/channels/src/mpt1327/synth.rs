//! An MPT 1327 control channel as a site transmits it: a preamble, SYNC, then slots of a control
//! channel system codeword and an address codeword, keyed as 1200 baud FFSK on FM.

use num_complex::Complex;
use sdrmm_modem::ffsk::FfskTones;
use sdrmm_modem::synth::{NARROW_HZ, Render, Signal, fm_modulate, silence};

use super::DEVIATION_HZ;
use super::codeword::{Code, PREAMBLE, SYNC};
use super::message::Address;

const LEVEL: f32 = 0.8;
const GAP_S: f64 = 0.1;
const PREAMBLE_BITS: usize = 32;
const COMPLETION_SEQUENCES: u32 = 1 << 16;

/// A group call granted channel `channel`.
#[must_use]
pub fn gtc(called: Address, calling_ident: u16, channel: u16) -> u64 {
    1 << 47
        | u64::from(called.prefix & 0x7F) << 40
        | u64::from(called.ident & 0x1FFF) << 27
        | u64::from(channel & 0x3FF) << 15
        | u64::from(calling_ident & 0x1FFF) << 2
}

/// A general Aloha inviting every unit, sent on the control channel numbered `channel`.
#[must_use]
pub fn aloha(channel: u16) -> u64 {
    1 << 47 | 1 << 26 | u64::from(channel & 0xF) << 14
}

/// The broadcast of a system's call maintenance parameters.
#[must_use]
pub fn maintenance_parameters(system: u16) -> u64 {
    1 << 47 | 0b00010 << 42 | u64::from(system & 0x7FFF) << 27 | 1 << 26 | 0b11_100 << 18
}

fn push_word(bits: &mut Vec<bool>, word: u64, len: u32) {
    bits.extend((0..len).rev().map(|bit| word >> bit & 1 == 1));
}

/// The control channel system codeword of `system`: its completion sequence is the one that
/// makes the check bits spell SYNC.
fn system_codeword(code: &Code, system: u16) -> u64 {
    let base = u64::from(system & 0x7FFF) << 32 | u64::from(PREAMBLE);
    (0..COMPLETION_SEQUENCES)
        .map(|sequence| code.encode(base | u64::from(sequence) << 16))
        .find(|word| *word as u16 == SYNC)
        .unwrap_or_default()
}

/// The bits of a control channel carrying `messages`, one per slot.
#[must_use]
pub fn control_bits(system: u16, messages: &[u64]) -> Vec<bool> {
    let code = Code::new();
    let ccsc = system_codeword(&code, system);
    let mut bits: Vec<bool> = (0..PREAMBLE_BITS).map(|k| k % 2 == 0).collect();
    push_word(&mut bits, u64::from(SYNC), 16);
    for (slot, message) in messages.iter().enumerate() {
        if slot > 0 {
            push_word(&mut bits, ccsc, 64);
        }
        push_word(&mut bits, code.encode(*message), 64);
    }
    push_word(&mut bits, ccsc, 64);
    bits.push(false);
    bits
}

/// `bits` keyed on an FM carrier at `rate`, with a little silence either side.
#[must_use]
pub fn modulate(bits: &[bool], rate: f64) -> Vec<Complex<f32>> {
    let gap = (GAP_S * rate) as usize;
    let mut iq = silence(gap);
    iq.extend(fm_modulate(
        &FfskTones::CCIR_1200.audio(bits, LEVEL, rate),
        DEVIATION_HZ,
        rate,
    ));
    iq.extend(silence(gap));
    iq
}

#[must_use]
pub fn control_channel(system: u16, messages: &[u64], rate: f64) -> Vec<Complex<f32>> {
    modulate(&control_bits(system, messages), rate)
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "mpt1327",
    label: "MPT 1327",
    rate_hz: NARROW_HZ,
    render: Render::Always(site),
}];

pub(crate) const SITE_SYSTEM: u16 = 0x1FD7;

pub(crate) fn site_messages() -> Vec<u64> {
    let fleet = Address {
        prefix: 10,
        ident: 4105,
    };
    vec![
        aloha(6),
        maintenance_parameters(SITE_SYSTEM),
        gtc(fleet, 77, 123),
        aloha(6),
    ]
}

fn site() -> Vec<Complex<f32>> {
    control_channel(SITE_SYSTEM, &site_messages(), NARROW_HZ)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpt1327::codeword::Codeword;
    use crate::mpt1327::message::Message;

    #[test]
    fn the_system_codeword_ends_in_a_preamble_and_sync() {
        let code = Code::new();
        let word = system_codeword(&code, SITE_SYSTEM);
        let heard = code.decode(word).expect("a valid codeword");
        assert_eq!(heard, Codeword { word, errors: 0 });
        let system = heard.system().expect("a system codeword");
        assert_eq!(system.system, SITE_SYSTEM);
        assert!(system.control);
    }

    #[test]
    fn the_built_messages_read_back() {
        assert_eq!(Message::read(aloha(6)).mnemonic(), "ALH");
        assert_eq!(
            Message::read(maintenance_parameters(SITE_SYSTEM)).own_system(),
            Some(SITE_SYSTEM)
        );
        let Message::Gtc { channel, .. } = Message::read(gtc(
            Address {
                prefix: 1,
                ident: 2,
            },
            3,
            1_000,
        )) else {
            panic!("a grant");
        };
        assert_eq!(channel, 1_000);
    }
}
