use std::f32::consts::PI;

use num_complex::Complex;

use super::code::{self, N};
use crate::weak_signal::ftx::message::{CallBook, unpack};

pub(super) const FRAME: usize = 864;
pub(super) const BITS: usize = 144;
pub(super) const SYNC_SAMPLES: usize = 42;
/// Where the second sync word starts, 56 bits into the frame.
pub(super) const SECOND_SYNC: usize = 336;
const HALF_SYMBOL: usize = 6;
const SYMBOL: usize = 12;
const SYNC: [bool; 8] = [false, true, true, true, false, false, true, false];
const SYNC_BITS: usize = SYNC.len();
const SECOND_SYNC_BIT: usize = 56;
const MAX_BAD_SYNC: u32 = 4;
const MAX_HARD_ERRORS: u32 = 18;
const SIGMA: f32 = 0.6;

pub(super) fn pulse() -> [f32; SYMBOL] {
    std::array::from_fn(|index| (index as f32 * PI / SYMBOL as f32).sin())
}

fn polar(bit: bool) -> f32 {
    if bit { 1.0 } else { -1.0 }
}

/// The complex baseband of the eight sync bits as they open a frame: odd bits on I, even bits
/// on Q half a symbol early, so the first half of bit 0 lies before the frame.
pub(super) fn sync_template() -> [Complex<f32>; SYNC_SAMPLES] {
    let pulse = pulse();
    let mut template = [Complex::default(); SYNC_SAMPLES];
    for (bit, &value) in SYNC.iter().enumerate() {
        let sign = polar(value);
        let start = (bit / 2) * SYMBOL;
        for (offset, &shape) in pulse.iter().enumerate() {
            if bit % 2 == 1 {
                if let Some(sample) = template.get_mut(start + offset) {
                    sample.re = sign * shape;
                }
            } else if let Some(sample) = (start + offset)
                .checked_sub(HALF_SYMBOL)
                .and_then(|at| template.get_mut(at))
            {
                sample.im = sign * shape;
            }
        }
    }
    template
}

/// The complex baseband of one frame for its 144 channel bits.
#[cfg(any(test, feature = "synth"))]
pub(super) fn baseband(bits: &[bool; BITS]) -> [Complex<f32>; FRAME] {
    let pulse = pulse();
    let mut frame = [Complex::default(); FRAME];
    for (bit, &value) in bits.iter().enumerate() {
        let sign = polar(value);
        if bit % 2 == 1 {
            let start = (bit / 2) * SYMBOL;
            for (offset, &shape) in pulse.iter().enumerate() {
                frame[start + offset].re = sign * shape;
            }
        } else {
            let start = (bit / 2) * SYMBOL + FRAME - HALF_SYMBOL;
            for (offset, &shape) in pulse.iter().enumerate() {
                frame[(start + offset) % FRAME].im = sign * shape;
            }
        }
    }
    frame
}

#[cfg(any(test, feature = "synth"))]
/// The 144 channel bits of a frame: a sync word, 48 codeword bits, the sync word again and
/// the last 80 codeword bits.
pub(super) fn channel_bits(codeword: &[bool; N]) -> [bool; BITS] {
    let mut bits = [false; BITS];
    bits[..SYNC_BITS].copy_from_slice(&SYNC);
    bits[SYNC_BITS..SECOND_SYNC_BIT].copy_from_slice(&codeword[..SECOND_SYNC_BIT - SYNC_BITS]);
    bits[SECOND_SYNC_BIT..SECOND_SYNC_BIT + SYNC_BITS].copy_from_slice(&SYNC);
    bits[SECOND_SYNC_BIT + SYNC_BITS..].copy_from_slice(&codeword[SECOND_SYNC_BIT - SYNC_BITS..]);
    bits
}

pub(super) struct FrameDecoder {
    template: [Complex<f32>; SYNC_SAMPLES],
    pulse: [f32; SYMBOL],
    code: code::Decoder,
    book: CallBook,
}

pub(super) struct Message {
    pub(super) payload: u128,
    pub(super) text: String,
    pub(super) hard_errors: u32,
}

impl FrameDecoder {
    pub(super) fn new() -> Self {
        Self {
            template: sync_template(),
            pulse: pulse(),
            code: code::Decoder::new(),
            book: CallBook::default(),
        }
    }

    /// Decodes a frame whose first sample is the start of its first sync word.
    pub(super) fn decode(&mut self, frame: &[Complex<f32>; FRAME]) -> Option<Message> {
        let soft = self.soft_bits(frame);
        if bad_sync(&soft) > MAX_BAD_SYNC {
            return None;
        }
        let decoded = self.code.decode(&llr(&soft))?;
        if decoded.hard_errors >= MAX_HARD_ERRORS || !sent_in_msk144(decoded.payload) {
            return None;
        }
        Some(Message {
            payload: decoded.payload,
            text: unpack(decoded.payload, &mut self.book)?,
            hard_errors: decoded.hard_errors,
        })
    }

    fn soft_bits(&self, frame: &[Complex<f32>; FRAME]) -> [f32; BITS] {
        let correlate = |start: usize| -> Complex<f32> {
            frame[start..start + SYNC_SAMPLES]
                .iter()
                .zip(&self.template)
                .map(|(sample, sync)| sample * sync.conj())
                .sum()
        };
        let both = correlate(0) + correlate(SECOND_SYNC);
        let derotate = Complex::from_polar(1.0, -both.arg());
        let at = |index: usize| frame[index % FRAME] * derotate;
        std::array::from_fn(|bit| {
            let pair = bit / 2;
            if bit % 2 == 1 {
                (0..SYMBOL)
                    .map(|offset| at(pair * SYMBOL + offset).re * self.pulse[offset])
                    .sum()
            } else {
                (0..SYMBOL)
                    .map(|offset| {
                        at(pair * SYMBOL + offset + FRAME - HALF_SYMBOL).im * self.pulse[offset]
                    })
                    .sum()
            }
        })
    }
}

fn bad_sync(soft: &[f32; BITS]) -> u32 {
    [0, SECOND_SYNC_BIT]
        .iter()
        .flat_map(|&start| {
            soft[start..start + SYNC_BITS]
                .iter()
                .zip(&SYNC)
                .filter(|&(&value, &sync)| (value >= 0.0) != sync)
        })
        .count() as u32
}

fn llr(soft: &[f32; BITS]) -> [f32; N] {
    let count = BITS as f32;
    let mean = soft.iter().sum::<f32>() / count;
    let power = soft.iter().map(|value| value * value).sum::<f32>() / count;
    let spread = (power - mean * mean).max(f32::MIN_POSITIVE).sqrt();
    let scale = 2.0 / (spread * SIGMA * SIGMA);
    let mut llr = [0.0; N];
    let data = soft[SYNC_BITS..SECOND_SYNC_BIT]
        .iter()
        .chain(&soft[SECOND_SYNC_BIT + SYNC_BITS..]);
    for (slot, &value) in llr.iter_mut().zip(data) {
        *slot = scale * value;
    }
    llr
}

/// MSK144 carries standard, nonstandard, EU VHF and free-text messages, never the `DXpedition`,
/// field day or RTTY contest types.
fn sent_in_msk144(payload: u128) -> bool {
    let i3 = payload & 7;
    let n3 = (payload >> 3) & 7;
    !(i3 == 0 && matches!(n3, 1 | 3 | 4 | 6 | 7) || i3 == 3 || i3 > 5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weak_signal::ftx::message::pack;

    #[test]
    fn the_sync_template_matches_the_frame_it_opens() {
        let codeword = code::encode(0x1234).expect("encodes");
        let frame = baseband(&channel_bits(&codeword));
        let template = sync_template();
        for (index, sync) in template.iter().enumerate() {
            assert!((frame[index] - sync).norm() < 1e-6, "sample {index}");
            assert!(
                (frame[SECOND_SYNC + index] - sync).norm() < 1e-6,
                "sample {index}"
            );
        }
    }

    #[test]
    fn a_clean_frame_decodes_at_any_carrier_phase() {
        let payload = pack("CQ K1JT FN20").expect("packs");
        let codeword = code::encode(payload).expect("encodes");
        let clean = baseband(&channel_bits(&codeword));
        let mut decoder = FrameDecoder::new();
        for phase in [0.0f32, 1.0, 2.5, -2.0] {
            let rotated = clean.map(|sample| sample * Complex::from_polar(1.0, phase));
            let message = decoder.decode(&rotated).expect("decodes");
            assert_eq!(message.text, "CQ K1JT FN20");
            assert_eq!(message.hard_errors, 0);
        }
    }

    #[test]
    fn contest_messages_are_not_msk144() {
        assert!(sent_in_msk144(1));
        assert!(sent_in_msk144(0));
        assert!(!sent_in_msk144(1 << 3));
        assert!(!sent_in_msk144(3));
        assert!(!sent_in_msk144(6 | (2 << 3)));
    }
}
