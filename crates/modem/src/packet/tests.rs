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
