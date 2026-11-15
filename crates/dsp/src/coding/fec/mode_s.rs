const MODE_S_POLY: u32 = 0x00FF_F409;
const MODE_S_PARITY_BITS: u32 = 24;
const MODE_S_MASK: u32 = (1 << MODE_S_PARITY_BITS) - 1;
const MODE_S_TOP_BIT: u32 = 1 << (MODE_S_PARITY_BITS - 1);
const MODE_S_SHORT_LEN: usize = 7;
const MODE_S_LONG_LEN: usize = 14;
const MODE_S_PARITY_LEN: usize = 3;

#[must_use]
pub fn mode_s_syndrome(frame: &[u8]) -> u32 {
    if !is_transmission(frame.len()) {
        return u32::MAX;
    }
    mode_s_crc(frame)
}

#[must_use]
pub fn mode_s_overlay(frame: &[u8]) -> Option<u32> {
    if !is_transmission(frame.len()) {
        return None;
    }
    let (body, field) = frame.split_at(frame.len() - MODE_S_PARITY_LEN);
    let &[hi, mid, lo] = field else { return None };
    let parity = u32::from(hi) << 16 | u32::from(mid) << 8 | u32::from(lo);
    Some(mode_s_crc(body) ^ parity)
}

#[must_use]
pub fn mode_s_bit_overlays(frame_len: usize) -> Vec<u32> {
    let mut frame = vec![0u8; frame_len];
    (0..frame_len * 8)
        .map(|bit| {
            frame.fill(0);
            frame[bit / 8] = 0x80 >> (bit % 8);
            mode_s_overlay(&frame).unwrap_or(u32::MAX)
        })
        .collect()
}

pub fn mode_s_append_parity(body: &mut Vec<u8>) {
    mode_s_append_overlaid_parity(body, 0);
}

pub fn mode_s_append_overlaid_parity(body: &mut Vec<u8>, overlay: u32) {
    assert!(
        is_transmission(body.len() + MODE_S_PARITY_LEN),
        "mode s message body must be 4 or 11 bytes, got {}",
        body.len()
    );
    let parity = mode_s_crc(body) ^ (overlay & MODE_S_MASK);
    body.extend_from_slice(&[(parity >> 16) as u8, (parity >> 8) as u8, parity as u8]);
}

pub fn mode_s_fix_single_bit(frame: &mut [u8]) -> Option<usize> {
    let syndrome = mode_s_syndrome(frame);
    if syndrome == 0 {
        return None;
    }
    let bits = frame.len() * 8;
    let mut trial = MODE_S_POLY;
    for i in (0..bits).rev() {
        if trial == syndrome {
            frame[i / 8] ^= 0x80 >> (i % 8);
            return Some(i);
        }
        trial = mode_s_step(trial);
    }
    None
}

const fn is_transmission(len: usize) -> bool {
    len == MODE_S_SHORT_LEN || len == MODE_S_LONG_LEN
}

fn mode_s_step(reg: u32) -> u32 {
    if reg & MODE_S_TOP_BIT != 0 {
        ((reg << 1) ^ MODE_S_POLY) & MODE_S_MASK
    } else {
        (reg << 1) & MODE_S_MASK
    }
}

fn mode_s_crc(data: &[u8]) -> u32 {
    let mut reg = 0;
    for &byte in data {
        reg ^= u32::from(byte) << (MODE_S_PARITY_BITS - 8);
        for _ in 0..8 {
            reg = mode_s_step(reg);
        }
    }
    reg
}

#[cfg(test)]
mod tests {
    use super::{super::test_data::flip_msb_first as flip, *};

    fn squitter_body() -> Vec<u8> {
        vec![
            0x8D, 0x40, 0x62, 0x1D, 0x58, 0xC3, 0x82, 0xD6, 0x90, 0xC8, 0xAC,
        ]
    }

    #[test]
    fn mode_s_parity_matches_the_published_squitter() {
        let mut frame = squitter_body();
        mode_s_append_parity(&mut frame);
        assert_eq!(frame[11..], [0x28, 0x63, 0xA7]);
        assert_eq!(mode_s_syndrome(&frame), 0);
    }

    #[test]
    fn mode_s_parity_closes_short_frames_too() {
        let mut frame = vec![0x5D, 0x48, 0x40, 0xD6];
        mode_s_append_parity(&mut frame);
        assert_eq!(frame.len(), MODE_S_SHORT_LEN);
        assert_eq!(mode_s_syndrome(&frame), 0);
    }

    #[test]
    fn mode_s_repairs_any_single_bit_error() {
        let mut clean = squitter_body();
        mode_s_append_parity(&mut clean);

        for bit in 0..clean.len() * 8 {
            let mut frame = clean.clone();
            flip(&mut frame, bit);
            assert_ne!(mode_s_syndrome(&frame), 0, "bit {bit} left no syndrome");
            assert_eq!(mode_s_fix_single_bit(&mut frame), Some(bit));
            assert_eq!(mode_s_syndrome(&frame), 0);
            assert_eq!(frame, clean, "bit {bit} repaired to the wrong frame");
        }
    }

    #[test]
    fn mode_s_refuses_to_repair_a_two_bit_error() {
        let mut clean = squitter_body();
        mode_s_append_parity(&mut clean);
        let bits = clean.len() * 8;

        for a in 0..bits {
            for b in [(a + 1) % bits, (a + 7) % bits, (a + 53) % bits] {
                if a == b {
                    continue;
                }
                let mut frame = clean.clone();
                flip(&mut frame, a);
                flip(&mut frame, b);
                assert_eq!(mode_s_fix_single_bit(&mut frame), None, "bits {a},{b}");
                assert_ne!(mode_s_syndrome(&frame), 0, "bits {a},{b}");
            }
        }
    }

    #[test]
    fn flipping_a_bit_moves_the_overlay_by_its_table_entry() {
        for len in [MODE_S_SHORT_LEN, MODE_S_LONG_LEN] {
            let table = mode_s_bit_overlays(len);
            let frame: Vec<u8> = (0..len as u8).map(|b| b.wrapping_mul(37) ^ 0x5A).collect();
            let base = mode_s_overlay(&frame).unwrap();
            for (bit, &delta) in table.iter().enumerate() {
                let mut flipped = frame.clone();
                flip(&mut flipped, bit);
                assert_eq!(
                    mode_s_overlay(&flipped).unwrap(),
                    base ^ delta,
                    "len {len} bit {bit}"
                );
            }
        }
    }

    #[test]
    fn mode_s_leaves_a_clean_frame_alone() {
        let mut frame = squitter_body();
        mode_s_append_parity(&mut frame);
        let clean = frame.clone();
        assert_eq!(mode_s_fix_single_bit(&mut frame), None);
        assert_eq!(frame, clean);
    }

    #[test]
    fn mode_s_rejects_lengths_that_are_not_transmissions() {
        assert_ne!(mode_s_syndrome(&[0; 8]), 0);
        assert_ne!(mode_s_syndrome(&[]), 0);
        assert_eq!(mode_s_fix_single_bit(&mut [0; 8]), None);
        assert_eq!(mode_s_overlay(&[0; 8]), None);
        assert_eq!(mode_s_overlay(&[]), None);
    }

    #[test]
    fn an_overlaid_address_comes_back_out_of_the_parity() {
        for overlay in [0x00_0001, 0x3C_6444, 0x48_40D6, 0xFF_FFFF] {
            for body in [vec![0x20, 0x00, 0x19, 0x10], squitter_body()] {
                let mut frame = body;
                mode_s_append_overlaid_parity(&mut frame, overlay);
                assert_eq!(mode_s_overlay(&frame), Some(overlay));
                assert_ne!(mode_s_syndrome(&frame), overlay);
            }
        }
    }

    #[test]
    fn a_bare_parity_frame_reads_as_a_zero_overlay() {
        let mut frame = squitter_body();
        mode_s_append_parity(&mut frame);
        assert_eq!(mode_s_overlay(&frame), Some(0));
        assert_eq!(mode_s_syndrome(&frame), 0);

        flip(&mut frame, 42);
        assert_ne!(mode_s_overlay(&frame), Some(0));
    }
}
