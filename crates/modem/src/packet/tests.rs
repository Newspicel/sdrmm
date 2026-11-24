use num_complex::Complex;

use super::*;

const RATE: f64 = 48_000.0;
const BYTES: std::ops::RangeInclusive<usize> = 4..=400;

fn afsk(frame: &[u8]) -> Vec<Complex<f32>> {
    let mut iq = afsk_burst(frame, RATE);
    iq.extend(crate::synth::silence(4_800));
    iq
}

#[test]
fn afsk_frames_come_back_with_their_fcs() {
    let frame = b"\x01\x07\x20N0CALL\x06\x24hello";
    let mut rx = PacketReceiver::new(PacketMode::Afsk1200, RATE, &BYTES);
    let mut frames = Vec::new();
    for block in afsk(frame).chunks(1_000) {
        rx.process(block, |heard| frames.push(heard.bytes.to_vec()));
    }
    assert!(!frames.is_empty());
    for heard in &frames {
        assert_eq!(&heard[..frame.len()], frame);
        assert!(sdrmm_dsp::hdlc_fcs_ok(heard));
    }
}

#[test]
fn a_watched_receiver_hears_the_same_frames_and_shows_every_bit() {
    for mode in [PacketMode::Afsk1200, PacketMode::G3ruh9600] {
        let iq = match mode {
            PacketMode::Afsk1200 => afsk(b"\x01\x07\x20N0CALL\x06\x24hello"),
            PacketMode::G3ruh9600 => crate::synth::silence(9_600),
        };
        let mut plain = PacketReceiver::new(mode, RATE, &BYTES);
        let mut watched = PacketReceiver::new(mode, RATE, &BYTES);
        let (mut heard, mut seen, mut levels) = (Vec::new(), Vec::new(), Vec::new());
        plain.process(&iq, |frame| heard.push(frame.bytes.to_vec()));
        watched.process_watched(&iq, &mut levels, |frame| seen.push(frame.bytes.to_vec()));
        assert_eq!(heard, seen, "{mode:?}");
        let bits = iq.len() as f64 * mode.baud() / RATE;
        assert!(
            (levels.len() as f64 - bits).abs() < 0.02 * bits,
            "{mode:?}: {} levels for {bits} bits",
            levels.len()
        );
    }
}

#[test]
fn a_repeat_inside_the_window_is_dropped_and_after_it_passes() {
    let mut recent = RecentFrames::new(100);
    assert!(recent.first(b"abc"));
    assert!(!recent.first(b"abc"));
    assert!(recent.first(b"abd"));
    recent.advance(99);
    assert!(!recent.first(b"abc"));
    recent.advance(1);
    assert!(recent.first(b"abc"));
    recent.clear();
    assert!(recent.first(b"abc"));
}

#[test]
fn a_heard_frame_costs_no_allocation() {
    let iq = afsk(b"\x01\x07\x20N0CALL\x06\x24hello");
    let mut rx = PacketReceiver::new(PacketMode::Afsk1200, RATE, &BYTES);
    rx.process(&iq, |_| {});
    let mut heard = 0;
    sdrmm_test_support::assert_no_alloc("packet receiver", || {
        rx.process(&iq, |_| heard += 1);
    });
    assert!(heard > 0);
}

/// The repair the packet decoders ran before soft bits reached them: every run of one or two
/// adjacent bits, blind to which bits were weak.
fn blind_repair(frame: &mut [u8]) -> bool {
    let flip = |frame: &mut [u8], at: usize| frame[at / 8] ^= 1 << (at % 8);
    if sdrmm_dsp::hdlc_fcs_ok(frame) {
        return true;
    }
    for span in 1..=2 {
        for at in 0..(frame.len() * 8).saturating_sub(span - 1) {
            (at..at + span).for_each(|bit| flip(frame, bit));
            if sdrmm_dsp::hdlc_fcs_ok(frame) {
                return true;
            }
            (at..at + span).for_each(|bit| flip(frame, bit));
        }
    }
    false
}

/// What noise at `noise_var` let through over `runs` transmissions of `frame`.
#[derive(Default)]
struct Heard {
    /// Transmissions heard whole through blind repair.
    blind: u32,
    /// Transmissions heard whole through soft repair.
    soft: u32,
    /// Lane frames that failed their FCS.
    failed: u32,
    /// Lane frames soft repair passed that were not `frame`.
    wrong: u32,
}

fn heard_through_noise(frame: &[u8], noise_var: f64, runs: u64) -> Heard {
    let mut tally = Heard::default();
    for seed in 0..runs {
        let mut iq = afsk(frame);
        crate::testutil::add_noise(&mut iq, 0x9e37 + seed, noise_var);
        let mut rx = PacketReceiver::new(PacketMode::Afsk1200, RATE, &BYTES);
        let (mut blind, mut soft) = (false, false);
        rx.process(&iq, |heard| {
            let mut bytes = heard.bytes.to_vec();
            tally.failed += u32::from(!sdrmm_dsp::hdlc_fcs_ok(&bytes));
            blind |= blind_repair(&mut bytes.clone()) && bytes.starts_with(frame);
            if sdrmm_dsp::hdlc_repair(&mut bytes, heard.reliability) {
                soft |= bytes.starts_with(frame);
                tally.wrong += u32::from(!bytes.starts_with(frame));
            }
        });
        tally.blind += u32::from(blind);
        tally.soft += u32::from(soft);
    }
    tally
}

#[test]
fn soft_repair_hears_afsk_where_blind_repair_fails() {
    let frame = b"\x82\xa0\xa4\xa6@@`\x9c\x94n\xa0@@\xe1\x03\xf0hello, soft bits";
    let heard = heard_through_noise(frame, 0.45, 40);
    assert!(
        heard.soft >= 2 * heard.blind && heard.soft >= 15,
        "blind {}, soft {} of 40",
        heard.blind,
        heard.soft
    );
    assert!(
        heard.wrong * 50 <= heard.failed,
        "{} wrong of {}",
        heard.wrong,
        heard.failed
    );
    let heard = heard_through_noise(frame, 0.5, 40);
    assert_eq!(heard.blind, 0);
    assert!(heard.soft >= 5, "soft {} of 40", heard.soft);
}
