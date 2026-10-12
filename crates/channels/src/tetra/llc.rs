use super::bits::Reader;

const FCS_BITS: usize = 32;

pub(crate) fn tl_sdu(bits: &[bool]) -> Option<&[bool]> {
    let mut r = Reader::new(bits);
    let kind = r.take(4)?;
    let (header, fcs) = match kind {
        0 => (2, false),
        1 => (1, false),
        2 => (0, false),
        3 => (1, false),
        4 => (2, true),
        5 => (1, true),
        6 => (0, true),
        7 => (1, true),
        _ => return None,
    };
    r.skip(header)?;
    let body = r.rest();
    if !fcs {
        return Some(body);
    }
    let split = body.len().checked_sub(FCS_BITS)?;
    let (sdu, sent) = body.split_at(split);
    (fcs32(sdu) == Reader::new(sent).take(FCS_BITS)?).then_some(sdu)
}

pub(crate) fn fcs32(bits: &[bool]) -> u32 {
    let mut crc = u32::MAX;
    if bits.len() < FCS_BITS {
        crc <<= FCS_BITS - bits.len();
    }
    for &bit in bits {
        let feedback = u32::from(bit) ^ crc >> 31;
        crc <<= 1;
        if feedback == 1 {
            crc ^= 0x04C1_1DB7;
        }
    }
    !crc
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Discriminator {
    Mm,
    Cmce,
    Sndcp,
    Mle,
    Other,
}

pub(crate) fn discriminator(bits: &[bool]) -> Option<(Discriminator, &[bool])> {
    let mut r = Reader::new(bits);
    let kind = match r.take(3)? {
        1 => Discriminator::Mm,
        2 => Discriminator::Cmce,
        4 => Discriminator::Sndcp,
        5 => Discriminator::Mle,
        _ => Discriminator::Other,
    };
    Some((kind, r.rest()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(value: u64, len: usize) -> Vec<bool> {
        (0..len).rev().map(|i| value >> i & 1 == 1).collect()
    }

    #[test]
    fn basic_link_headers_are_stripped() {
        let mut pdu = bits(0b0010, 4);
        pdu.extend(bits(0b010_00111, 8));
        assert_eq!(tl_sdu(&pdu), Some(bits(0b010_00111, 8).as_slice()));
        let mut pdu = bits(0b0000, 4);
        pdu.extend(bits(0b11_0101, 6));
        assert_eq!(tl_sdu(&pdu), Some(bits(0b0101, 4).as_slice()));
    }

    #[test]
    fn a_frame_check_sequence_guards_the_sdu() {
        let sdu = bits(0x2A_5C3, 20);
        let mut pdu = bits(0b0110, 4);
        pdu.extend(&sdu);
        pdu.extend(bits(u64::from(fcs32(&sdu)), 32));
        assert_eq!(tl_sdu(&pdu), Some(sdu.as_slice()));
        let last = pdu.len() - 1;
        pdu[last] = !pdu[last];
        assert_eq!(tl_sdu(&pdu), None);
    }
}
