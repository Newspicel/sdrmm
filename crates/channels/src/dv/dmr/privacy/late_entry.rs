use sdrmm_dsp::{CyclicCode, crc_msb_bits};

use super::FRAMES_PER_BURST;

const BURSTS: usize = 6;
const DATA_BURSTS: usize = 3;
const FRAGMENT_BITS: u32 = 4;
const WORD_BITS: u32 = 12;
const CRC_POLY: u32 = 0x3;
const CRC_INVERT: u32 = 0xF;

/// Four bits of the late-entry message indicator ride in the low end of each voice frame's
/// unprotected u3, lowest bit first.
fn fragment_of(u3: u16) -> u8 {
    (0..FRAGMENT_BITS).fold(0, |acc, bit| acc << 1 | (u3 >> bit & 1) as u8)
}

pub(crate) fn with_fragment(u3: u16, fragment: u8) -> u16 {
    (0..FRAGMENT_BITS).fold(u3 & !0xF, |acc, bit| {
        acc | u16::from(fragment >> (FRAGMENT_BITS - 1 - bit) & 1) << bit
    })
}

fn crc4(mi: u32) -> u32 {
    crc_msb_bits(CRC_POLY, 4, 0, (0..32).rev().map(|bit| mi >> bit & 1 == 1)) ^ CRC_INVERT
}

/// Reads the 36 bits three bursts carry: each frame in turn takes four bits from each burst.
fn gather(fragments: &[[u8; FRAMES_PER_BURST]]) -> u64 {
    (0..FRAMES_PER_BURST)
        .flat_map(|frame| fragments.iter().map(move |burst| burst[frame]))
        .fold(0, |acc, fragment| {
            acc << FRAGMENT_BITS | u64::from(fragment)
        })
}

fn scatter(bits: u64, fragments: &mut [[u8; FRAMES_PER_BURST]]) {
    let mut shift = 36;
    for frame in 0..FRAMES_PER_BURST {
        for burst in fragments.iter_mut() {
            shift -= FRAGMENT_BITS;
            burst[frame] = (bits >> shift & 0xF) as u8;
        }
    }
}

/// The DMRA late-entry message indicator, spread over a superframe so a receiver that missed the
/// PI header learns the next superframe's MI: 32 bits and a CRC-4 in bursts A to C, their Golay
/// (24,12) parity in D to F.
#[derive(Default)]
pub(crate) struct LateEntry {
    fragments: [[u8; FRAMES_PER_BURST]; BURSTS],
    seen: u32,
}

impl LateEntry {
    pub(crate) fn push(&mut self, burst: usize, frame: usize, u3: u16) {
        if burst < BURSTS && frame < FRAMES_PER_BURST {
            self.fragments[burst][frame] = fragment_of(u3);
            self.seen |= 1 << (burst * FRAMES_PER_BURST + frame);
        }
    }

    /// The MI once a whole superframe's fragments are in and they decode; starts over either way.
    pub(crate) fn take(&mut self) -> Option<u32> {
        let complete = self.seen == (1 << (BURSTS * FRAMES_PER_BURST)) - 1;
        self.seen = 0;
        if !complete {
            return None;
        }
        let data = gather(&self.fragments[..DATA_BURSTS]);
        let parity = gather(&self.fragments[DATA_BURSTS..]);
        let mut corrected = 0u64;
        for word in 0..3 {
            let shift = WORD_BITS * (2 - word);
            let received = (data >> shift & 0xFFF) << WORD_BITS | parity >> shift & 0xFFF;
            let (info, _) = CyclicCode::GOLAY_24_12.decode(received)?;
            corrected = corrected << WORD_BITS | u64::from(info);
        }
        let mi = (corrected >> FRAGMENT_BITS) as u32;
        (corrected as u32 & 0xF == crc4(mi)).then_some(mi)
    }
}

/// The fragments a transmitter spreads `mi` over, burst by burst, frame by frame.
pub(crate) fn fragments(mi: u32) -> [[u8; FRAMES_PER_BURST]; BURSTS] {
    let data = u64::from(mi) << FRAGMENT_BITS | u64::from(crc4(mi));
    let mut parity = 0u64;
    for word in 0..3 {
        let info = (data >> (WORD_BITS * (2 - word)) & 0xFFF) as u32;
        let coded = CyclicCode::GOLAY_24_12.encode(info);
        parity = parity << WORD_BITS | coded & 0xFFF;
    }
    let mut out = [[0; FRAMES_PER_BURST]; BURSTS];
    scatter(data, &mut out[..DATA_BURSTS]);
    scatter(parity, &mut out[DATA_BURSTS..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn received(mi: u32) -> LateEntry {
        let mut late = LateEntry::default();
        for (burst, frames) in fragments(mi).iter().enumerate() {
            for (frame, &fragment) in frames.iter().enumerate() {
                late.push(burst, frame, with_fragment(0x2AB5, fragment));
            }
        }
        late
    }

    #[test]
    fn a_whole_superframe_gives_its_message_indicator() {
        assert_eq!(received(0xDEAD_BEEF).take(), Some(0xDEAD_BEEF));
    }

    #[test]
    fn golay_repairs_a_damaged_fragment() {
        let mut late = received(0x1234_5678);
        late.fragments[1][2] ^= 0b0101;
        assert_eq!(late.take(), Some(0x1234_5678));
    }

    #[test]
    fn a_missed_frame_gives_nothing() {
        let mut late = received(0x1234_5678);
        late.seen &= !(1 << 7);
        assert_eq!(late.take(), None);
    }

    #[test]
    fn the_fragment_rides_in_the_low_four_bits() {
        assert_eq!(with_fragment(0x3FF0, 0b1000), 0x3FF1);
        assert_eq!(fragment_of(0x3FF1), 0b1000);
    }

    #[test]
    #[ignore = "needs KNOWN_KEY_SAMPLES=<known-key-mbe-samples checkout>"]
    fn known_key_samples_carry_the_next_message_indicator() {
        use crate::dv::known_keys::samples;
        use crate::dv::vocoder::{AMBE_3600_INTERLEAVE, half_frame, half_rate_code_vectors};

        let all = samples("dmr_");
        assert!(!all.is_empty(), "no dmr samples");
        for sample in &all {
            let markers: Vec<(usize, u32)> = sample
                .frames
                .iter()
                .enumerate()
                .filter_map(|(index, frame)| frame.mi.map(|mi| (index, mi as u32)))
                .collect();
            let start = markers[0].0 % 18;
            let mut late = LateEntry::default();
            let (mut matched, mut decoded) = (0, 0);
            for (index, frame) in sample.frames.iter().enumerate().skip(start) {
                let position = (index - start) % 18;
                let bits: [bool; 72] =
                    std::array::from_fn(|i| frame.bytes[i / 8] >> (7 - i % 8) & 1 == 1);
                let info = half_frame(half_rate_code_vectors(&bits, &AMBE_3600_INTERLEAVE)).info;
                late.push(position / 3, position % 3, info[3]);
                if position == 17
                    && let Some(mi) = late.take()
                {
                    decoded += 1;
                    let next = markers.iter().find(|(at, _)| *at == index + 1);
                    matched += usize::from(next.is_some_and(|(_, marked)| *marked == mi));
                }
            }
            println!(
                "{}: {decoded} decoded, {matched} match the next marker",
                sample.name
            );
            assert!(matched + 1 >= markers.len(), "{}", sample.name);
        }
    }
}
