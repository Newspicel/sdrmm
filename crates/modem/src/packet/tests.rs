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
        rx.process(block, |heard| frames.push(heard.to_vec()));
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
        plain.process(&iq, |frame| heard.push(frame.to_vec()));
        watched.process_watched(&iq, &mut levels, |frame| seen.push(frame.to_vec()));
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
