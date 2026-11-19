use super::*;
use crate::framesync::FixedFramer;
use sdrmm_modem_test_support::noise::complex_noise;

const BIT_RATE: f64 = 665_400.0;
const RATE: f64 = 4.0 * BIT_RATE;
const INDEX: f64 = 1.2;
const WORD: u64 = 0x1ACF_FC1D;
const FRAME_BYTES: usize = 128;

fn payload(frame: usize) -> Vec<u8> {
    (0..FRAME_BYTES - 4)
        .map(|k| (frame * 31 + k * 7) as u8)
        .collect()
}

fn frames_bits(count: usize) -> Vec<bool> {
    let mut bytes = vec![0x5A; 40];
    for frame in 0..count {
        bytes.extend_from_slice(&WORD.to_be_bytes()[4..]);
        bytes.extend(payload(frame));
    }
    bytes
        .iter()
        .flat_map(|byte| (0..8).rev().map(move |shift| byte >> shift & 1 == 1))
        .collect()
}

fn decoded(iq: &[Complex<f32>]) -> Vec<Vec<u8>> {
    let mut demod = SplitPhaseDemod::new(RATE, BIT_RATE, 2_000.0);
    let mut framer = FixedFramer::new(WORD, 32, FRAME_BYTES * 8, 0, 4, 2);
    let mut soft = Vec::new();
    let mut frames = Vec::new();
    for chunk in iq.chunks(4_096) {
        soft.clear();
        demod.process(chunk, &mut soft);
        for &value in &soft {
            if framer.push(value > 0.0).is_some() {
                frames.push(framer.frame()[4..].to_vec());
            }
        }
    }
    frames
}

/// The first frame may still carry the loops' settling; every later one must be exact.
fn assert_frames(frames: &[Vec<u8>], sent: usize, must: usize) {
    assert!(frames.len() >= must, "{} of {sent} frames", frames.len());
    let first = (0..sent)
        .find(|&frame| payload(frame) == frames[1])
        .expect("second frame is one that was sent");
    for (k, frame) in frames.iter().enumerate().skip(1) {
        assert_eq!(*frame, payload(first + k - 1), "frame {k}");
    }
}

#[test]
fn split_phase_bits_survive_a_carrier_offset() {
    let sent = 40;
    let iq = modulate(&frames_bits(sent), RATE, BIT_RATE, INDEX, 37_000.0);
    assert_frames(&decoded(&iq), sent, sent - 4);
}

#[test]
fn split_phase_bits_survive_noise_and_a_clock_offset() {
    let sent = 40;
    let bits = frames_bits(sent);
    let mut iq = modulate(&bits, RATE, BIT_RATE * (1.0 + 30e-6), INDEX, -12_000.0);
    let noise = complex_noise(7, 0.35, iq.len());
    for (sample, noise) in iq.iter_mut().zip(noise) {
        *sample += noise;
    }
    assert_frames(&decoded(&iq), sent, sent - 6);
}

#[test]
fn the_carrier_loop_reports_its_offset() {
    let iq = modulate(&frames_bits(10), RATE, BIT_RATE, INDEX, 25_000.0);
    let mut demod = SplitPhaseDemod::new(RATE, BIT_RATE, 2_000.0);
    demod.process(&iq, &mut Vec::new());
    assert!(demod.carrier().locked());
    assert!((demod.carrier().offset_hz() - 25_000.0).abs() < 50.0);
}
