use crate::coding::fec::crc16_x25;

pub const HDLC_FLAG: u8 = 0x7E;
/// Ones in a row after which the sender stuffs a zero.
const STUFF_ONES: u8 = 5;
const FLAG_ONES: u8 = 6;
const ABORT_ONES: u8 = 7;
/// Bits of a closing flag already shifted into the byte being built when the frame ended on
/// a byte boundary.
const FLAG_TAIL_BITS: [u8; 2] = [5, 6];

/// A closed frame and how sure each of its bits was: bit `i` is bit `i % 8` of byte `i / 8`,
/// the order HDLC sends them.
#[derive(Clone, Copy, Debug)]
pub struct SoftFrame<'a> {
    pub bytes: &'a [u8],
    pub reliability: &'a [f32],
}

/// Bits in, HDLC frames out. A closed frame is lent from a buffer the next frame reuses, so a
/// frame costs no allocation.
#[derive(Clone, Debug)]
pub struct HdlcDeframer {
    min_bytes: usize,
    max_bytes: usize,
    ones: u8,
    byte: u8,
    nbits: u8,
    frame: Vec<u8>,
    closed: Vec<u8>,
    reliability: Vec<f32>,
    closed_reliability: Vec<f32>,
    in_frame: bool,
    overflow: bool,
}

impl HdlcDeframer {
    #[must_use]
    pub fn new(min_bytes: usize, max_bytes: usize) -> Self {
        assert!(
            min_bytes > 0 && max_bytes >= min_bytes,
            "hdlc frame bounds must be non-empty and ordered"
        );
        Self {
            min_bytes,
            max_bytes,
            ones: 0,
            byte: 0,
            nbits: 0,
            frame: Vec::with_capacity(max_bytes),
            closed: Vec::with_capacity(max_bytes),
            reliability: Vec::with_capacity(max_bytes * 8),
            closed_reliability: Vec::with_capacity(max_bytes * 8),
            in_frame: false,
            overflow: false,
        }
    }

    pub fn push(&mut self, bit: bool) -> Option<&[u8]> {
        self.step(bit, 1.0).then_some(self.closed.as_slice())
    }

    /// Takes one soft bit, positive is one; a closed frame comes with each bit's magnitude.
    pub fn push_soft(&mut self, value: f32) -> Option<SoftFrame<'_>> {
        self.step(value > 0.0, value.abs()).then_some(SoftFrame {
            bytes: &self.closed,
            reliability: &self.closed_reliability,
        })
    }

    /// `true` when the bit closed a frame, now in `closed`.
    fn step(&mut self, bit: bool, reliability: f32) -> bool {
        match self.ones {
            0..STUFF_ONES => {
                self.ones = if bit { self.ones + 1 } else { 0 };
                self.append(bit, reliability);
                false
            }
            STUFF_ONES => {
                self.ones = if bit { FLAG_ONES } else { 0 };
                false
            }
            FLAG_ONES if !bit => self.close(),
            _ => {
                self.ones = if bit { ABORT_ONES } else { 0 };
                self.in_frame = false;
                self.discard();
                false
            }
        }
    }

    pub fn reset(&mut self) {
        self.ones = 0;
        self.in_frame = false;
        self.discard();
    }

    fn append(&mut self, bit: bool, reliability: f32) {
        if !self.in_frame {
            return;
        }
        if self.reliability.len() < self.max_bytes * 8 {
            self.reliability.push(reliability);
        }
        self.byte |= u8::from(bit) << self.nbits;
        self.nbits += 1;
        if self.nbits == 8 {
            if self.frame.len() == self.max_bytes {
                self.overflow = true;
            } else {
                self.frame.push(self.byte);
            }
            self.byte = 0;
            self.nbits = 0;
        }
    }

    fn discard(&mut self) {
        self.frame.clear();
        self.reliability.clear();
        self.byte = 0;
        self.nbits = 0;
        self.overflow = false;
    }

    fn close(&mut self) -> bool {
        let aligned = FLAG_TAIL_BITS.contains(&self.nbits);
        let keep = self.in_frame
            && aligned
            && !self.overflow
            && (self.min_bytes..=self.max_bytes).contains(&self.frame.len());
        if keep {
            self.reliability.truncate(self.frame.len() * 8);
            std::mem::swap(&mut self.frame, &mut self.closed);
            std::mem::swap(&mut self.reliability, &mut self.closed_reliability);
        }
        self.ones = 0;
        self.in_frame = true;
        self.discard();
        keep
    }
}

/// Bytes in, HDLC bits out: flags, then each byte LSB first with a zero stuffed after five ones.
#[derive(Clone, Debug, Default)]
pub struct HdlcFramer {
    ones: u8,
}

impl HdlcFramer {
    #[must_use]
    pub const fn new() -> Self {
        Self { ones: 0 }
    }

    pub fn push_flags(&mut self, bits: &mut Vec<bool>, count: usize) {
        for _ in 0..count {
            bits.extend((0..8).map(|i| HDLC_FLAG >> i & 1 == 1));
        }
        self.ones = 0;
    }

    pub fn push_bit(&mut self, bits: &mut Vec<bool>, bit: bool) {
        bits.push(bit);
        self.ones = if bit { self.ones + 1 } else { 0 };
        if self.ones == STUFF_ONES {
            bits.push(false);
            self.ones = 0;
        }
    }

    pub fn push_bytes(&mut self, bits: &mut Vec<bool>, bytes: &[u8]) {
        for &byte in bytes {
            for i in 0..8 {
                self.push_bit(bits, byte >> i & 1 == 1);
            }
        }
    }

    /// `frame`, its CRC-16/X.25 FCS, then the closing flag.
    pub fn push_frame(&mut self, bits: &mut Vec<bool>, frame: &[u8]) {
        self.push_bytes(bits, frame);
        self.push_bytes(bits, &crc16_x25(frame).to_le_bytes());
        self.push_flags(bits, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLAG: [bool; 8] = [false, true, true, true, true, true, true, false];

    fn stuff(payload: &[u8]) -> Vec<bool> {
        let mut out = Vec::new();
        let mut ones = 0;
        for &byte in payload {
            for i in 0..8 {
                let bit = byte >> i & 1 == 1;
                out.push(bit);
                ones = if bit { ones + 1 } else { 0 };
                if ones == 5 {
                    out.push(false);
                    ones = 0;
                }
            }
        }
        out
    }

    fn hdlc_frame(payload: &[u8]) -> Vec<bool> {
        let mut bits = FLAG.to_vec();
        bits.extend(stuff(payload));
        bits.extend(FLAG);
        bits
    }

    #[test]
    fn framed_bits_deframe_to_the_frame_and_its_fcs() {
        let payload = [0xFFu8, 0x7E, 0x3F, 0x00, 0xF8];
        let mut framer = HdlcFramer::new();
        let mut bits = Vec::new();
        framer.push_flags(&mut bits, 2);
        framer.push_frame(&mut bits, &payload);
        let frames = deframe(&mut HdlcDeframer::new(2, 64), &bits);
        let mut expected = payload.to_vec();
        expected.extend(crc16_x25(&payload).to_le_bytes());
        assert_eq!(frames, vec![expected]);
    }

    #[test]
    fn the_framer_breaks_every_run_of_five_ones() {
        let mut framer = HdlcFramer::new();
        let mut bits = Vec::new();
        framer.push_bytes(&mut bits, &[0xFF; 4]);
        let longest = bits
            .chunk_by(|a, b| a == b)
            .filter(|run| run[0])
            .map(<[bool]>::len)
            .max();
        assert_eq!(longest, Some(5));
        assert_eq!(bits.len(), 32 + 6);
    }

    fn deframe(deframer: &mut HdlcDeframer, bits: &[bool]) -> Vec<Vec<u8>> {
        bits.iter()
            .filter_map(|&b| deframer.push(b).map(<[u8]>::to_vec))
            .collect()
    }

    #[test]
    fn hdlc_returns_exactly_the_payload() {
        let payload = [0x82u8, 0xa0, 0x01, 0x5c];
        let mut bits = vec![false; 5];
        bits.extend(hdlc_frame(&payload));
        let frames = deframe(&mut HdlcDeframer::new(2, 64), &bits);
        assert_eq!(frames, vec![payload.to_vec()]);
    }

    #[test]
    fn hdlc_unstuffs_long_runs_of_ones() {
        let payload = [0xffu8, 0x03, 0x7f];
        let bits = hdlc_frame(&payload);
        assert!(
            bits.len() > 8 + payload.len() * 8 + 8,
            "fixture must actually contain stuffed bits"
        );
        let frames = deframe(&mut HdlcDeframer::new(2, 64), &bits);
        assert_eq!(frames, vec![payload.to_vec()]);
    }

    #[test]
    fn hdlc_drops_frames_outside_the_length_bounds() {
        let mut deframer = HdlcDeframer::new(4, 8);
        assert!(deframe(&mut deframer, &hdlc_frame(&[0x11, 0x22])).is_empty());
        assert!(deframe(&mut deframer, &hdlc_frame(&[0xaa; 9])).is_empty());
        let ok = [0x11u8, 0x22, 0x33, 0x44];
        assert_eq!(deframe(&mut deframer, &hdlc_frame(&ok)), vec![ok.to_vec()]);
    }

    #[test]
    fn hdlc_handles_frames_sharing_one_flag() {
        let (first, second) = ([0x01u8, 0x02, 0x03], [0xf8u8, 0xfc, 0xfe]);
        let mut bits = FLAG.to_vec();
        bits.extend(stuff(&first));
        bits.extend(FLAG);
        bits.extend(stuff(&second));
        bits.extend(FLAG);
        let frames = deframe(&mut HdlcDeframer::new(2, 64), &bits);
        assert_eq!(frames, vec![first.to_vec(), second.to_vec()]);
    }

    #[test]
    fn hdlc_abort_drops_the_frame_in_progress() {
        let payload = [0x11u8, 0x22, 0x33];
        let mut bits = FLAG.to_vec();
        bits.extend(stuff(&payload));
        bits.extend([true; 7]);
        bits.push(false);
        bits.extend(hdlc_frame(&payload));
        let frames = deframe(&mut HdlcDeframer::new(2, 64), &bits);
        assert_eq!(
            frames,
            vec![payload.to_vec()],
            "only the clean frame survives"
        );
    }

    #[test]
    fn hdlc_reset_discards_a_partial_frame() {
        let payload = [0x11u8, 0x22, 0x33];
        let bits = hdlc_frame(&payload);
        let mut deframer = HdlcDeframer::new(2, 64);
        let split = bits.len() - 4;
        assert!(deframe(&mut deframer, &bits[..split]).is_empty());
        deframer.reset();
        assert!(deframe(&mut deframer, &bits[split..]).is_empty());
        assert_eq!(deframe(&mut deframer, &bits), vec![payload.to_vec()]);
    }

    #[test]
    fn a_soft_frame_carries_each_data_bit_its_magnitude() {
        let payload = [0xFFu8, 0x81, 0x3C];
        let mut bits = Vec::new();
        let mut framer = HdlcFramer::new();
        framer.push_flags(&mut bits, 1);
        framer.push_bytes(&mut bits, &payload);
        framer.push_flags(&mut bits, 1);
        let mut deframer = HdlcDeframer::new(2, 64);
        let mut heard = None;
        for (index, &bit) in bits.iter().enumerate() {
            let size = 1.0 + index as f32;
            if let Some(frame) = deframer.push_soft(if bit { size } else { -size }) {
                heard = Some((frame.bytes.to_vec(), frame.reliability.to_vec()));
            }
        }
        let (bytes, reliability) = heard.expect("the frame closes");
        assert_eq!(bytes, payload);
        assert_eq!(reliability.len(), payload.len() * 8);
        assert_eq!(&reliability[..6], &[9.0, 10.0, 11.0, 12.0, 13.0, 15.0]);
    }

    #[test]
    fn hdlc_rejects_a_misaligned_frame() {
        let mut bits = FLAG.to_vec();
        bits.extend(stuff(&[0x11, 0x22, 0x33]));
        bits.push(false);
        bits.extend(FLAG);
        assert!(deframe(&mut HdlcDeframer::new(2, 64), &bits).is_empty());
    }
}
