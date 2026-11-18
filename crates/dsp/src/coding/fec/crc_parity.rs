/// A CRC check field closing a byte frame, optionally added to an overlay such as an
/// address, so the receiver reads the overlay back out of the parity.
#[derive(Clone, Copy, Debug)]
pub struct CrcParity {
    poly: u32,
    bits: u32,
}

impl CrcParity {
    #[must_use]
    pub const fn new(poly: u32, bits: u32) -> Self {
        assert!(
            bits >= 8 && bits <= 32 && bits.is_multiple_of(8),
            "parity is one to four whole bytes"
        );
        Self { poly, bits }
    }

    #[must_use]
    pub const fn bytes(&self) -> usize {
        (self.bits / 8) as usize
    }

    /// The CRC over the whole frame: zero for a clean frame with no overlay.
    #[must_use]
    pub fn remainder(&self, data: &[u8]) -> u32 {
        let mut reg = 0;
        for &byte in data {
            reg ^= u32::from(byte) << (self.bits - 8);
            for _ in 0..8 {
                reg = self.step(reg);
            }
        }
        reg
    }

    #[must_use]
    pub fn overlay(&self, frame: &[u8]) -> Option<u32> {
        let split = frame.len().checked_sub(self.bytes())?;
        let (body, field) = frame.split_at(split);
        let parity = field
            .iter()
            .fold(0u32, |acc, &byte| acc << 8 | u32::from(byte));
        Some(self.remainder(body) ^ parity)
    }

    /// How flipping each bit of a `frame_len` byte frame, MSB first, moves its overlay.
    #[must_use]
    pub fn bit_overlays(&self, frame_len: usize) -> Vec<u32> {
        let mut frame = vec![0u8; frame_len];
        (0..frame_len * 8)
            .map(|bit| {
                frame.fill(0);
                frame[bit / 8] = 0x80 >> (bit % 8);
                self.overlay(&frame).unwrap_or(u32::MAX)
            })
            .collect()
    }

    pub fn append(&self, body: &mut Vec<u8>, overlay: u32) {
        let parity = self.remainder(body) ^ (overlay & self.mask());
        for shift in (0..self.bits).step_by(8).rev() {
            body.push((parity >> shift) as u8);
        }
    }

    /// Flips the one bit the frame's syndrome points at; returns its MSB first index.
    pub fn fix_single_bit(&self, frame: &mut [u8]) -> Option<usize> {
        let syndrome = self.remainder(frame);
        if syndrome == 0 {
            return None;
        }
        let bits = frame.len() * 8;
        let mut trial = self.poly & self.mask();
        for i in (0..bits).rev() {
            if trial == syndrome {
                frame[i / 8] ^= 0x80 >> (i % 8);
                return Some(i);
            }
            trial = self.step(trial);
        }
        None
    }

    const fn mask(self) -> u32 {
        u32::MAX >> (32 - self.bits)
    }

    const fn step(self, reg: u32) -> u32 {
        let top = 1 << (self.bits - 1);
        if reg & top != 0 {
            ((reg << 1) ^ self.poly) & self.mask()
        } else {
            (reg << 1) & self.mask()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{super::test_data::flip_msb_first as flip, *};

    const CRC24: CrcParity = CrcParity::new(0x00FF_F409, 24);
    const SHORT: usize = 7;
    const LONG: usize = 14;

    fn body() -> Vec<u8> {
        vec![
            0x8D, 0x40, 0x62, 0x1D, 0x58, 0xC3, 0x82, 0xD6, 0x90, 0xC8, 0xAC,
        ]
    }

    fn closed() -> Vec<u8> {
        let mut frame = body();
        CRC24.append(&mut frame, 0);
        frame
    }

    #[test]
    fn parity_matches_a_published_frame() {
        let frame = closed();
        assert_eq!(frame[11..], [0x28, 0x63, 0xA7]);
        assert_eq!(CRC24.remainder(&frame), 0);
    }

    #[test]
    fn parity_closes_short_frames_too() {
        let mut frame = vec![0x5D, 0x48, 0x40, 0xD6];
        CRC24.append(&mut frame, 0);
        assert_eq!(frame.len(), SHORT);
        assert_eq!(CRC24.remainder(&frame), 0);
    }

    #[test]
    fn any_single_bit_error_is_repaired() {
        let clean = closed();
        for bit in 0..clean.len() * 8 {
            let mut frame = clean.clone();
            flip(&mut frame, bit);
            assert_ne!(CRC24.remainder(&frame), 0, "bit {bit} left no syndrome");
            assert_eq!(CRC24.fix_single_bit(&mut frame), Some(bit));
            assert_eq!(frame, clean, "bit {bit} repaired to the wrong frame");
        }
    }

    #[test]
    fn a_two_bit_error_is_not_repaired() {
        let clean = closed();
        let bits = clean.len() * 8;
        for a in 0..bits {
            for b in [(a + 1) % bits, (a + 7) % bits, (a + 53) % bits] {
                if a == b {
                    continue;
                }
                let mut frame = clean.clone();
                flip(&mut frame, a);
                flip(&mut frame, b);
                assert_eq!(CRC24.fix_single_bit(&mut frame), None, "bits {a},{b}");
                assert_ne!(CRC24.remainder(&frame), 0, "bits {a},{b}");
            }
        }
    }

    #[test]
    fn flipping_a_bit_moves_the_overlay_by_its_table_entry() {
        for len in [SHORT, LONG] {
            let table = CRC24.bit_overlays(len);
            let frame: Vec<u8> = (0..len as u8).map(|b| b.wrapping_mul(37) ^ 0x5A).collect();
            let base = CRC24.overlay(&frame).unwrap();
            for (bit, &delta) in table.iter().enumerate() {
                let mut flipped = frame.clone();
                flip(&mut flipped, bit);
                assert_eq!(
                    CRC24.overlay(&flipped).unwrap(),
                    base ^ delta,
                    "{len} {bit}"
                );
            }
        }
    }

    #[test]
    fn a_clean_frame_is_left_alone() {
        let mut frame = closed();
        let clean = frame.clone();
        assert_eq!(CRC24.fix_single_bit(&mut frame), None);
        assert_eq!(frame, clean);
    }

    #[test]
    fn a_frame_shorter_than_its_parity_has_no_overlay() {
        assert_eq!(CRC24.overlay(&[0; 2]), None);
        assert_eq!(CRC24.overlay(&[]), None);
    }

    #[test]
    fn an_overlaid_address_comes_back_out_of_the_parity() {
        for overlay in [0x00_0001, 0x3C_6444, 0x48_40D6, 0xFF_FFFF] {
            for body in [vec![0x20, 0x00, 0x19, 0x10], body()] {
                let mut frame = body;
                CRC24.append(&mut frame, overlay);
                assert_eq!(CRC24.overlay(&frame), Some(overlay));
                assert_ne!(CRC24.remainder(&frame), overlay);
            }
        }
    }

    #[test]
    fn a_bare_parity_frame_reads_as_a_zero_overlay() {
        let mut frame = closed();
        assert_eq!(CRC24.overlay(&frame), Some(0));
        flip(&mut frame, 42);
        assert_ne!(CRC24.overlay(&frame), Some(0));
    }

    #[test]
    fn a_sixteen_bit_parity_round_trips() {
        let crc16 = CrcParity::new(0x1021, 16);
        let mut frame = vec![0x12, 0x34, 0x56];
        crc16.append(&mut frame, 0xBEEF);
        assert_eq!(frame.len(), 5);
        assert_eq!(crc16.overlay(&frame), Some(0xBEEF));
    }
}
