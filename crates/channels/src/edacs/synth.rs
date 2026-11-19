//! An EDACS site keyed from scratch: control channel frames and a working channel's call.

use num_complex::Complex;
use sdrmm_dsp::coding::fec::Bch;
use sdrmm_modem::synth::{bits, fm_modulate, fsk, tone_audio};

use super::{COPIES, DATA_BITS, EdacsBaud, MESSAGE_BITS, SYNC, SYNC_BITS, air};

const DOTTING_BITS: usize = 64;
const VOICE_LEVEL: f32 = 0.6;

/// A message's 28 bits and their BCH check, as sent.
#[must_use]
pub fn codeword(data: u32) -> Vec<bool> {
    let message = bits(u64::from(data), DATA_BITS);
    let mut word = Vec::with_capacity(MESSAGE_BITS);
    Bch::new(6, 0x43, 2).encode(&message, &mut word);
    word
}

/// One frame: the sync, then each message three times with the middle copy inverted.
#[must_use]
pub fn frame(first: &[bool], second: &[bool]) -> Vec<bool> {
    let mut out = bits(SYNC, SYNC_BITS as usize);
    for word in [first, second] {
        for copy in 0..COPIES {
            out.extend(word.iter().map(|&bit| bit != (copy == 1)));
        }
    }
    out
}

/// A control channel sending `frames`, two messages each, after a second of its idle frame.
#[must_use]
pub fn control_channel(frames: &[(u32, u32)], baud: EdacsBaud, rate: f64) -> Vec<Complex<f32>> {
    let idle = frame(&codeword(site(1, 1)), &codeword(site(1, 1)));
    let lead = (baud.hz() / idle.len() as f64) as usize;
    let mut keyed: Vec<bool> = (0..lead).flat_map(|_| idle.clone()).collect();
    for &(first, second) in frames {
        keyed.extend(frame(&codeword(first), &codeword(second)));
    }
    keyed.extend((0..lead / 4).flat_map(|_| idle.clone()));
    fsk(&keyed, baud.hz(), air(baud).data_deviation_hz, rate)
}

fn dotting() -> Vec<bool> {
    (0..DOTTING_BITS).map(|index| index % 2 == 0).collect()
}

/// A working channel's call: the site's data, `voice_s` of a voice tone, the disconnect dotting.
#[must_use]
pub fn working_channel(baud: EdacsBaud, voice_s: f64, rate: f64) -> Vec<Complex<f32>> {
    let deviation = air(baud).data_deviation_hz;
    let confirm = frame(&codeword(site(1, 1)), &codeword(site(1, 1)));
    let mut iq = fsk(
        &[dotting(), confirm.clone(), confirm].concat(),
        baud.hz(),
        deviation,
        rate,
    );
    let voice = tone_audio(1_000.0, VOICE_LEVEL, rate, (voice_s * rate) as usize);
    iq.extend(fm_modulate(&voice, air(baud).voice_deviation_hz, rate));
    iq.extend(fsk(&dotting(), baud.hz(), deviation, rate));
    iq
}

/// A standard site's identity: site `site`, its control channel on LCN `control_lcn`.
#[must_use]
pub const fn site(site: u32, control_lcn: u32) -> u32 {
    0x7 << 25 | 0x7 << 22 | 0x08 << 17 | control_lcn << 12 | site
}

/// A standard analog voice group channel update: `group` is on LCN `lcn`.
#[must_use]
pub const fn group_update(group: u32, lcn: u32) -> u32 {
    0x7 << 25 | 0x3 << 22 | 0x2 << 20 | lcn << 15 | group
}

/// A standard analog voice group assignment and its second word, which carries the rest of the
/// caller's logical ID.
#[must_use]
pub const fn group_assignment(group: u32, unit: u32, lcn: u32) -> (u32, u32) {
    (
        (unit >> 7) << 18 | lcn << 12 | 1 << 11 | group,
        (unit & 0x7F) << 17,
    )
}

/// An extended addressing analog group grant: `group` from `unit` on LCN `lcn`.
#[must_use]
pub const fn extended_group(group: u32, unit: u32, lcn: u32) -> (u32, u32) {
    (0x06 << 23 | lcn << 17 | group, 1 << 21 | unit)
}
