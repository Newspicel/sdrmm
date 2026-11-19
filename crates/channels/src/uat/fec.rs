//! The UAT Reed-Solomon codes of DO-282B: one per ADS-B length, and the six interleaved blocks
//! of a ground uplink.

use sdrmm_dsp::{Permutation, ReedSolomon};

/// x^8 + x^7 + x^2 + x + 1, the field every UAT code is built on.
const PRIMITIVE: u16 = 0x187;
/// Roots start at alpha^120, as in DO-282B.
const FIRST_ROOT: u8 = 120;

pub(crate) const SHORT_DATA: usize = 18;
pub(crate) const SHORT_BYTES: usize = 30;
pub(crate) const LONG_DATA: usize = 34;
pub(crate) const LONG_BYTES: usize = 48;
pub(crate) const BLOCKS: usize = 6;
pub(crate) const BLOCK_DATA: usize = 72;
pub(crate) const BLOCK_BYTES: usize = 92;
pub(crate) const UPLINK_DATA: usize = BLOCKS * BLOCK_DATA;
pub(crate) const UPLINK_BYTES: usize = BLOCKS * BLOCK_BYTES;

/// A corrected ADS-B frame: its data and how many symbols the code repaired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Downlink {
    pub data: [u8; LONG_DATA],
    pub len: usize,
    pub corrected: u32,
}

impl Downlink {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

pub(crate) struct Codes {
    short: ReedSolomon,
    long: ReedSolomon,
    uplink: ReedSolomon,
    interleave: Permutation,
}

impl Codes {
    pub(crate) fn new() -> Self {
        Self {
            short: ReedSolomon::new(PRIMITIVE, FIRST_ROOT, SHORT_BYTES - SHORT_DATA),
            long: ReedSolomon::new(PRIMITIVE, FIRST_ROOT, LONG_BYTES - LONG_DATA),
            uplink: ReedSolomon::new(PRIMITIVE, FIRST_ROOT, BLOCK_BYTES - BLOCK_DATA),
            interleave: Permutation::from_fn(UPLINK_BYTES, |at| {
                (at % BLOCK_BYTES) * BLOCKS + at / BLOCK_BYTES
            }),
        }
    }

    /// A Long ADS-B frame when the payload type says long, else a Basic one. A frame the code
    /// cannot repair is `None`.
    pub(crate) fn downlink(&self, frame: &mut [u8; LONG_BYTES]) -> Option<Downlink> {
        let mut long = *frame;
        if let Some(corrected) = self.long.decode(&mut long)
            && payload_type(&long) != 0
        {
            return Some(downlink(&long, LONG_DATA, corrected));
        }
        let short = &mut frame[..SHORT_BYTES];
        let corrected = self.short.decode(short)?;
        (payload_type(short) == 0).then(|| downlink(short, SHORT_DATA, corrected))
    }

    /// The 432 data bytes of a ground uplink, every block repaired, or `None` when one is not.
    pub(crate) fn uplink(
        &self,
        received: &[u8; UPLINK_BYTES],
        blocks: &mut [u8; UPLINK_BYTES],
        data: &mut [u8; UPLINK_DATA],
    ) -> Option<u32> {
        self.interleave.gather(received, blocks);
        let mut corrected = 0;
        for (block, out) in blocks
            .as_chunks_mut::<BLOCK_BYTES>()
            .0
            .iter_mut()
            .zip(data.as_chunks_mut::<BLOCK_DATA>().0)
        {
            corrected += self.uplink.decode(block)?;
            out.copy_from_slice(&block[..BLOCK_DATA]);
        }
        Some(corrected)
    }

    #[cfg(any(test, feature = "synth"))]
    pub(crate) fn encode_downlink(&self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(LONG_BYTES);
        let code = if data.len() == LONG_DATA {
            &self.long
        } else {
            &self.short
        };
        code.encode(data, &mut out);
        out
    }

    #[cfg(any(test, feature = "synth"))]
    pub(crate) fn encode_uplink(&self, data: &[u8; UPLINK_DATA]) -> [u8; UPLINK_BYTES] {
        let mut blocks = Vec::with_capacity(UPLINK_BYTES);
        for block in data.as_chunks::<BLOCK_DATA>().0 {
            self.uplink.encode(block, &mut blocks);
        }
        let mut sent = [0u8; UPLINK_BYTES];
        self.interleave.scatter(&blocks, &mut sent);
        sent
    }
}

fn payload_type(frame: &[u8]) -> u8 {
    frame[0] >> 3
}

fn downlink(frame: &[u8], len: usize, corrected: u32) -> Downlink {
    let mut data = [0u8; LONG_DATA];
    data[..len].copy_from_slice(&frame[..len]);
    Downlink {
        data,
        len,
        corrected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    /// dump978's sample-data.txt.gz, line 1003: a Long ADS-B frame from AD7233.
    const LONG: &str = "08ad7233358c435286720a89009a36202f06430d6ded2d0bcea4c080000ae0000000";
    /// The same file, line 1: a Basic frame from A66EF1.
    const SHORT: &str = "00a66ef135445d525a0c0519119021204800";

    fn received(data: &str) -> [u8; LONG_BYTES] {
        let mut frame = [0u8; LONG_BYTES];
        let sent = Codes::new().encode_downlink(&hex(data));
        frame[..sent.len()].copy_from_slice(&sent);
        frame
    }

    #[test]
    fn a_long_frame_survives_seven_symbol_errors() {
        let codes = Codes::new();
        let mut frame = received(LONG);
        for at in [0, 5, 11, 20, 33, 40, 47] {
            frame[at] ^= 0xA5;
        }
        let decoded = codes.downlink(&mut frame).expect("repaired");
        assert_eq!(decoded.bytes(), hex(LONG));
        assert_eq!(decoded.corrected, 7);
    }

    #[test]
    fn a_basic_frame_is_read_from_its_first_thirty_bytes() {
        let codes = Codes::new();
        let mut frame = received(SHORT);
        for (at, byte) in frame.iter_mut().enumerate().skip(SHORT_BYTES) {
            *byte = at as u8;
        }
        frame[3] ^= 0x10;
        let decoded = codes.downlink(&mut frame).expect("repaired");
        assert_eq!(decoded.bytes(), hex(SHORT));
        assert_eq!(decoded.corrected, 1);
    }

    #[test]
    fn eight_long_frame_errors_are_refused() {
        let codes = Codes::new();
        let mut frame = received(LONG);
        for at in [0, 5, 11, 20, 33, 40, 44, 47] {
            frame[at] ^= 0x3C;
        }
        assert_eq!(codes.downlink(&mut frame), None);
    }

    #[test]
    fn an_uplink_round_trips_through_the_interleaver_with_ten_errors_a_block() {
        let codes = Codes::new();
        let data: [u8; UPLINK_DATA] = std::array::from_fn(|at| (at * 7 + 3) as u8);
        let mut sent = codes.encode_uplink(&data);
        for at in 0..60 {
            sent[at * BLOCKS + at % BLOCKS] ^= 0x5A;
        }
        let mut blocks = [0u8; UPLINK_BYTES];
        let mut out = [0u8; UPLINK_DATA];
        assert_eq!(codes.uplink(&sent, &mut blocks, &mut out), Some(60));
        assert_eq!(out, data);
    }

    #[test]
    fn the_interleaver_sends_block_bytes_in_turn() {
        let codes = Codes::new();
        let data: [u8; UPLINK_DATA] = std::array::from_fn(|at| (at / BLOCK_DATA) as u8);
        let sent = codes.encode_uplink(&data);
        assert_eq!(&sent[..BLOCKS * 2], &[0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5]);
    }
}
