//! Advertising data structures of the Core Specification Supplement, part A.

pub const FLAGS: u8 = 0x01;
pub const UUID16_SOME: u8 = 0x02;
pub const UUID16_ALL: u8 = 0x03;
pub const UUID32_SOME: u8 = 0x04;
pub const UUID32_ALL: u8 = 0x05;
pub const UUID128_SOME: u8 = 0x06;
pub const UUID128_ALL: u8 = 0x07;
pub const NAME_SHORT: u8 = 0x08;
pub const NAME_COMPLETE: u8 = 0x09;
pub const TX_POWER: u8 = 0x0A;
pub const SERVICE_DATA_16: u8 = 0x16;
pub const APPEARANCE: u8 = 0x19;
pub const SERVICE_DATA_32: u8 = 0x20;
pub const SERVICE_DATA_128: u8 = 0x21;
pub const URI: u8 = 0x24;
pub const MANUFACTURER: u8 = 0xFF;

/// Each `(type, body)` of advertising data, stopping at the first structure that overruns.
pub fn structures(data: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    let mut rest = data;
    std::iter::from_fn(move || {
        let (&length, tail) = rest.split_first()?;
        let (structure, next) = tail.split_at_checked(usize::from(length))?;
        let (&kind, body) = structure.split_first()?;
        rest = next;
        Some((kind, body))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structures_split_by_length_and_stop_at_an_overrun() {
        let data = [
            0x02, FLAGS, 0x06, 0x03, NAME_SHORT, b'h', b'i', 0x05, URI, 1,
        ];
        let read: Vec<_> = structures(&data).collect();
        assert_eq!(read, vec![(FLAGS, &[0x06][..]), (NAME_SHORT, &b"hi"[..])]);
    }
}
