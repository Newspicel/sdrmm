use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_wire::EotStatus;

use super::{fm_modulate, silence};
use crate::eot::{
    BAUD, DEVIATION_HZ, MARK_HZ, SPACE_HZ,
    frame::{self, HEAD, HEAD_COPIES, REAR},
};

const REAR_DOTTING: usize = 69;
const REAR_FRAME_SYNC_BITS: u32 = 11;
const REAR_REPEATS: usize = 2;
const HEAD_DOTTING: usize = 456;
const HEAD_FRAME_SYNC_BITS: u32 = 24;
const LEVEL: f32 = 0.8;
const GAP_S: f64 = 0.1;

#[derive(Clone, Debug)]
pub struct Rear {
    pub unit_address: u32,
    pub status: EotStatus,
}

#[derive(Clone, Copy, Debug)]
pub struct Head {
    pub unit_address: u32,
    pub code: u8,
}

fn dotting(bits: &mut Vec<bool>, len: usize, last: bool) {
    bits.extend((0..len).map(|k| (len - 1 - k).is_multiple_of(2) == last));
}

fn sync(bits: &mut Vec<bool>, word: u64, len: u32) {
    bits.extend((0..len).rev().map(|bit| word >> bit & 1 == 1));
}

#[must_use]
pub fn rear_bits(rear: &Rear) -> Vec<bool> {
    let word = frame::rear_codeword(rear.unit_address, &rear.status);
    let mut bits = Vec::new();
    for _ in 0..REAR_REPEATS {
        dotting(&mut bits, REAR_DOTTING, false);
        sync(&mut bits, frame::REAR_SYNC, REAR_FRAME_SYNC_BITS);
        bits.extend(REAR.sent(word));
        bits.push(false);
    }
    bits
}

#[must_use]
pub fn head_bits(head: Head) -> Vec<bool> {
    let word = frame::head_codeword(head.unit_address, head.code);
    let mut bits = Vec::new();
    dotting(&mut bits, HEAD_DOTTING, true);
    sync(&mut bits, frame::HEAD_SYNC, HEAD_FRAME_SYNC_BITS);
    for _ in 0..HEAD_COPIES {
        bits.extend(HEAD.sent(word));
        bits.push(frame::odd_parity(word));
    }
    bits
}

#[must_use]
pub fn ffsk(bits: &[bool], rate: f64) -> Vec<f32> {
    let len = (bits.len() as f64 * rate / BAUD) as usize;
    let mut phase = 0.0f64;
    (0..len)
        .map(|k| {
            let bit = bits[((k as f64 * BAUD / rate) as usize).min(bits.len() - 1)];
            phase = (phase + TAU * if bit { MARK_HZ } else { SPACE_HZ } / rate).rem_euclid(TAU);
            LEVEL * phase.sin() as f32
        })
        .collect()
}

#[must_use]
pub fn modulate(bits: &[bool], rate: f64) -> Vec<Complex<f32>> {
    let gap = (GAP_S * rate) as usize;
    let mut iq = silence(gap);
    iq.extend(fm_modulate(&ffsk(bits, rate), DEVIATION_HZ, rate));
    iq.extend(silence(gap));
    iq
}

#[must_use]
pub fn rear_transmission(rear: &Rear, rate: f64) -> Vec<Complex<f32>> {
    modulate(&rear_bits(rear), rate)
}

#[must_use]
pub fn head_transmission(head: Head, rate: f64) -> Vec<Complex<f32>> {
    modulate(&head_bits(head), rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_burst_lengths_match_the_published_airtime() {
        let rear = Rear {
            unit_address: 1,
            status: crate::eot::tests::status(),
        };
        assert_eq!(rear_bits(&rear).len(), 288);
        let head = head_bits(Head {
            unit_address: 1,
            code: 0x55,
        });
        assert_eq!(head.len(), 672);
    }
}
