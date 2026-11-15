/// Ones in a row after which the sender stuffs a zero.
const STUFF_ONES: u8 = 5;
const FLAG_ONES: u8 = 6;
const ABORT_ONES: u8 = 7;
/// Bits of a closing flag already shifted into the byte being built when the frame ended on
/// a byte boundary.
const FLAG_TAIL_BITS: [u8; 2] = [5, 6];

#[derive(Clone, Debug)]
pub struct HdlcDeframer {
    min_bytes: usize,
    max_bytes: usize,
    ones: u8,
    byte: u8,
    nbits: u8,
    frame: Vec<u8>,
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
            in_frame: false,
            overflow: false,
        }
    }

    pub fn push(&mut self, bit: bool) -> Option<Vec<u8>> {
        match self.ones {
            0..STUFF_ONES => {
                self.ones = if bit { self.ones + 1 } else { 0 };
                self.append(bit);
                None
            }
            STUFF_ONES => {
                self.ones = if bit { FLAG_ONES } else { 0 };
                None
            }
            FLAG_ONES if !bit => self.close(),
            _ => {
                self.ones = if bit { ABORT_ONES } else { 0 };
                self.in_frame = false;
                self.discard();
                None
            }
        }
    }

    pub fn reset(&mut self) {
        self.ones = 0;
        self.in_frame = false;
        self.discard();
    }

    fn append(&mut self, bit: bool) {
        if !self.in_frame {
            return;
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
        self.byte = 0;
        self.nbits = 0;
        self.overflow = false;
    }

    fn close(&mut self) -> Option<Vec<u8>> {
        let aligned = FLAG_TAIL_BITS.contains(&self.nbits);
        let keep = self.in_frame
            && aligned
            && !self.overflow
            && (self.min_bytes..=self.max_bytes).contains(&self.frame.len());
        let out =
            keep.then(|| std::mem::replace(&mut self.frame, Vec::with_capacity(self.max_bytes)));
        self.ones = 0;
        self.in_frame = true;
        self.discard();
        out
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

    fn deframe(deframer: &mut HdlcDeframer, bits: &[bool]) -> Vec<Vec<u8>> {
        bits.iter().filter_map(|&b| deframer.push(b)).collect()
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
    fn hdlc_rejects_a_misaligned_frame() {
        let mut bits = FLAG.to_vec();
        bits.extend(stuff(&[0x11, 0x22, 0x33]));
        bits.push(false);
        bits.extend(FLAG);
        assert!(deframe(&mut HdlcDeframer::new(2, 64), &bits).is_empty());
    }
}
