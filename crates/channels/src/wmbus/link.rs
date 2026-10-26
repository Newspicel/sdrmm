use sdrmm_wire::WmbusFormat;

use super::coding::crc_holds;

pub(super) const FIRST_BLOCK: usize = 12;
pub(super) const MAX_RAW: usize = 290;
pub(super) const MAX_PAYLOAD: usize = 256;
const HEADER: usize = 10;
const BLOCK: usize = 16;
const CRC: usize = 2;
const FORMAT_A_MIN_L: u8 = 9;
const FORMAT_B_MIN_L: u8 = 12;
const FORMAT_B_SECOND: usize = 126;
const FORMAT_B_THIRD: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Payload {
    pub(super) bytes: [u8; MAX_PAYLOAD],
    pub(super) len: usize,
}

impl Payload {
    pub(super) fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

pub(super) fn raw_len(format: WmbusFormat, length: u8) -> Option<usize> {
    match format {
        WmbusFormat::A if length >= FORMAT_A_MIN_L => {
            let data = usize::from(length - FORMAT_A_MIN_L);
            Some(FIRST_BLOCK + data + CRC * data.div_ceil(BLOCK))
        }
        WmbusFormat::B if length >= FORMAT_B_MIN_L => {
            let total = usize::from(length) + 1;
            (total <= FORMAT_B_THIRD || total > FORMAT_B_THIRD + CRC).then_some(total)
        }
        _ => None,
    }
}

pub(super) fn first_block_holds(raw: &[u8]) -> bool {
    crc_holds(raw, HEADER)
}

pub(super) fn unpack(format: WmbusFormat, raw: &[u8]) -> Option<Payload> {
    let total = raw_len(format, *raw.first()?)?;
    let raw = raw.get(..total)?;
    let mut payload = Payload {
        bytes: [0; MAX_PAYLOAD],
        len: 0,
    };
    let blocks = match format {
        WmbusFormat::A => format_a(raw, &mut payload),
        WmbusFormat::B => format_b(raw, &mut payload),
    };
    blocks.then_some(payload)
}

fn take(payload: &mut Payload, raw: &[u8], start: usize, end: usize) -> bool {
    if !crc_holds(&raw[start..end + CRC], end - start) {
        return false;
    }
    let part = &raw[start..end];
    payload.bytes[payload.len..payload.len + part.len()].copy_from_slice(part);
    payload.len += part.len();
    true
}

fn format_a(raw: &[u8], payload: &mut Payload) -> bool {
    if !take(payload, raw, 0, HEADER) {
        return false;
    }
    let mut start = FIRST_BLOCK;
    while start < raw.len() {
        let end = (start + BLOCK).min(raw.len() - CRC);
        if !take(payload, raw, start, end) {
            return false;
        }
        start = end + CRC;
    }
    true
}

fn format_b(raw: &[u8], payload: &mut Payload) -> bool {
    let total = raw.len();
    if total <= FORMAT_B_THIRD {
        return take(payload, raw, 0, total - CRC);
    }
    take(payload, raw, 0, FORMAT_B_SECOND) && take(payload, raw, FORMAT_B_THIRD, total - CRC)
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn frame(format: WmbusFormat, payload: &[u8]) -> Vec<u8> {
    use super::coding::crc;
    let mut raw = Vec::new();
    let mut seal = |part: &[u8]| {
        raw.extend(part);
        raw.extend(crc(part).to_be_bytes());
    };
    match format {
        WmbusFormat::A => {
            seal(&payload[..HEADER]);
            payload[HEADER..].chunks(BLOCK).for_each(seal);
        }
        WmbusFormat::B => {
            let (second, third) = payload.split_at(payload.len().min(FORMAT_B_SECOND));
            seal(second);
            if !third.is_empty() {
                seal(third);
            }
        }
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_a_strips_every_block_check() {
        let mut payload = vec![0u8; 79];
        payload[0] = 78;
        for (k, byte) in payload.iter_mut().enumerate().skip(1) {
            *byte = k as u8;
        }
        let raw = frame(WmbusFormat::A, &payload);
        assert_eq!(raw_len(WmbusFormat::A, 78), Some(raw.len()));
        let unpacked = unpack(WmbusFormat::A, &raw).unwrap();
        assert_eq!(unpacked.as_slice(), payload.as_slice());
        let mut broken = raw.clone();
        broken[40] ^= 0x10;
        assert_eq!(unpack(WmbusFormat::A, &broken), None);
    }

    #[test]
    fn a_long_format_b_frame_checks_its_third_block() {
        let mut payload = vec![0u8; 146];
        payload[0] = 149;
        for (k, byte) in payload.iter_mut().enumerate().skip(1) {
            *byte = (k * 7) as u8;
        }
        let raw = frame(WmbusFormat::B, &payload);
        assert_eq!(raw.len(), 150);
        let unpacked = unpack(WmbusFormat::B, &raw).unwrap();
        assert_eq!(unpacked.as_slice(), payload.as_slice());
        let mut broken = raw.clone();
        broken[140] ^= 1;
        assert_eq!(unpack(WmbusFormat::B, &broken), None);
        assert_eq!(raw_len(WmbusFormat::B, 11), None);
    }
}
