use std::sync::LazyLock;

use sdrmm_dsp::{ReedSolomon, crc_msb_bits};

pub(crate) const RS_LENGTH: usize = 63;
pub(crate) const PARITY: usize = 28;
const MAX_PDU_BYTES: usize = 23;
const CRC12_POLY: u32 = 0x897;
const CRC16_POLY: u32 = 0x1021;

/// RS(63,35) over GF(64) on x^6 + x + 1, roots from x: sent shortened and punctured.
static RS: LazyLock<ReedSolomon> = LazyLock::new(|| ReedSolomon::with_field(6, 0x43, 1, PARITY));

/// Where a signalling channel's code sits in a burst body.
pub(crate) struct Layout {
    spans: &'static [(usize, usize)],
    /// The first sent symbol: those before it are shortened, zero and never sent.
    pub(crate) first: usize,
    /// Symbols sent; the parity after them is punctured.
    pub(crate) present: usize,
    pdu_bits: usize,
    crc16: bool,
}

impl Layout {
    /// The body dibits the code occupies, in bit order.
    pub(crate) fn dibits(&self) -> impl Iterator<Item = usize> + '_ {
        self.spans.iter().flat_map(|&(start, end)| start..end)
    }

    fn checked_bits(&self) -> usize {
        if self.crc16 {
            self.pdu_bits
        } else {
            self.pdu_bits + 12
        }
    }

    /// The bytes that hold MAC messages: the LCCH's CRC-16 is inside its PDU bits.
    fn message_bytes(&self) -> usize {
        if self.crc16 {
            (self.pdu_bits - 16) / 8
        } else {
            self.pdu_bits / 8
        }
    }
}

pub(crate) const FACCH: Layout = Layout {
    spans: &[(1, 37), (38, 69), (90, 122), (123, 159)],
    first: 9,
    present: 45,
    pdu_bits: 144,
    crc16: false,
};

pub(crate) const SACCH: Layout = Layout {
    spans: &[(1, 37), (38, 122), (123, 159)],
    first: 5,
    present: 52,
    pdu_bits: 168,
    crc16: false,
};

pub(crate) const LCCH: Layout = Layout {
    spans: &[(1, 37), (38, 122), (123, 159)],
    first: 5,
    present: 52,
    pdu_bits: 180,
    crc16: true,
};

/// A CRC-valid MAC PDU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MacPdu {
    bytes: [u8; MAX_PDU_BYTES],
    len: usize,
    pub(crate) corrected: u32,
}

impl MacPdu {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub(crate) fn kind(&self) -> PduKind {
        match self.bytes[0] >> 5 {
            0 => PduKind::Signal,
            1 => PduKind::Ptt,
            2 => PduKind::EndPtt,
            3 => PduKind::Idle,
            4 => PduKind::Active,
            6 => PduKind::Hangtime,
            other => PduKind::Reserved(other),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PduKind {
    Signal,
    Ptt,
    EndPtt,
    Idle,
    Active,
    Hangtime,
    Reserved(u8),
}

/// Decodes a signalling burst whose body dibits are already descrambled.
pub(crate) fn decode(body: &[u8], layout: &Layout) -> Option<MacPdu> {
    let mut received = [0u8; RS_LENGTH];
    for (index, at) in layout.dibits().enumerate() {
        let dibit = body.get(at)?;
        let symbol = layout.first + index / 3;
        received[symbol] = received[symbol] << 2 | dibit & 3;
    }
    let sent = layout.first + layout.present;
    let mut punctured = [0usize; RS_LENGTH];
    let mut erased = 0;
    for index in sent..RS_LENGTH {
        punctured[erased] = index;
        erased += 1;
    }
    if let Some(pdu) = correct(received, &punctured[..erased], layout) {
        return Some(pdu);
    }
    for index in 0..layout.first {
        punctured[erased] = index;
        erased += 1;
    }
    correct(received, &punctured[..erased], layout)
}

fn correct(received: [u8; RS_LENGTH], erasures: &[usize], layout: &Layout) -> Option<MacPdu> {
    let mut codeword = received;
    RS.decode_with_erasures(&mut codeword, erasures)?;
    let sent = layout.first..layout.first + layout.present;
    let corrected = sent
        .filter(|&index| codeword[index] != received[index])
        .count() as u32;
    let bits = codeword[layout.first..]
        .iter()
        .flat_map(|&symbol| (0..6).rev().map(move |shift| symbol >> shift & 1 == 1))
        .take(layout.checked_bits());
    let mut pdu = MacPdu {
        bytes: [0; MAX_PDU_BYTES],
        len: layout.message_bytes(),
        corrected,
    };
    let mut check = 0u32;
    for (index, bit) in bits.enumerate() {
        if index < layout.pdu_bits {
            pdu.bytes[index / 8] |= u8::from(bit) << (7 - index % 8);
        } else {
            check = check << 1 | u32::from(bit);
        }
    }
    crc_holds(&pdu.bytes, layout, check).then_some(pdu)
}

fn crc_holds(bytes: &[u8], layout: &Layout, check: u32) -> bool {
    let bits =
        |count: usize| (0..count).map(move |index| bytes[index / 8] >> (7 - index % 8) & 1 == 1);
    if layout.crc16 {
        let message = layout.pdu_bits - 16;
        let sent = bits(layout.pdu_bits)
            .skip(message)
            .fold(0u32, |word, bit| word << 1 | u32::from(bit));
        crc_msb_bits(CRC16_POLY, 16, 0, bits(message)) ^ 0xFFFF == sent
    } else {
        crc_msb_bits(CRC12_POLY, 12, 0, bits(layout.pdu_bits)) ^ 0xFFF == check
    }
}

/// The length of a MAC message from its opcode, for those whose length is not in the message,
/// as `GopherTrunk` carries over from `OP25`.
const MESSAGE_LENGTH: [u8; 256] = [
    0, 7, 8, 7, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    0, 14, 15, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    5, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    9, 7, 9, 0, 9, 8, 9, 0, 10, 10, 9, 0, 10, 0, 0, 0, //
    0, 0, 0, 0, 9, 7, 0, 0, 10, 0, 7, 0, 10, 8, 14, 7, //
    9, 9, 0, 0, 9, 0, 0, 9, 10, 0, 7, 10, 10, 7, 0, 9, //
    9, 29, 9, 9, 9, 9, 10, 13, 9, 9, 9, 11, 9, 9, 0, 0, //
    8, 0, 0, 7, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    16, 0, 0, 11, 13, 11, 11, 11, 10, 0, 0, 0, 0, 0, 0, 0, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    11, 0, 0, 8, 15, 12, 15, 32, 12, 12, 0, 27, 14, 29, 29, 32, //
    0, 0, 0, 0, 0, 0, 9, 0, 14, 29, 11, 27, 14, 0, 40, 11, //
    28, 0, 0, 14, 17, 14, 0, 0, 16, 8, 11, 0, 13, 19, 0, 0, //
    0, 0, 16, 14, 0, 0, 12, 0, 22, 0, 11, 13, 11, 0, 15, 0, //
];

const NULL: u8 = 0x00;
const NULL_AVOID_ZERO_BIAS: u8 = 0x08;
const INDIRECT_GROUP_PAGING: u8 = 0x11;
const INDIVIDUAL_PAGING: u8 = 0x12;
const VENDOR_PARTITION: u8 = 2;
/// A `MAC_SIGNAL`'s messages end here; the NAC follows them.
const SIGNAL_MESSAGE_BYTES: usize = 18;

/// One MAC message, or the bytes after one whose length the decoder does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Message<'a> {
    Known(&'a [u8]),
    Unsized(&'a [u8]),
}

/// The MAC messages a PDU carries, without the null fill.
pub(crate) fn messages(pdu: &MacPdu) -> impl Iterator<Item = Message<'_>> {
    let end = match pdu.kind() {
        PduKind::Ptt | PduKind::EndPtt => 0,
        PduKind::Signal => pdu.len.min(SIGNAL_MESSAGE_BYTES),
        _ => pdu.len,
    };
    let bytes = &pdu.bytes[..end];
    let mut at = 1;
    std::iter::from_fn(move || {
        loop {
            let rest = bytes.get(at..).filter(|rest| !rest.is_empty())?;
            if rest[0] == NULL {
                return None;
            }
            let len = message_length(rest);
            if len == 0 || len > rest.len() {
                at = bytes.len();
                return Some(Message::Unsized(rest));
            }
            at += len;
            if rest[0] != NULL_AVOID_ZERO_BIAS {
                return Some(Message::Known(&rest[..len]));
            }
        }
    })
}

fn message_length(message: &[u8]) -> usize {
    let opcode = message[0];
    let at = |index: usize| message.get(index).copied().map_or(0, usize::from);
    match opcode {
        NULL_AVOID_ZERO_BIAS => at(1) & 0x3F,
        INDIRECT_GROUP_PAGING => ((at(1) & 3) + 1) * 2 + 2,
        INDIVIDUAL_PAGING => ((at(1) & 3) + 1) * 3 + 2,
        _ if opcode >> 6 == VENDOR_PARTITION && at(2) & 0x3F != 0 => at(2) & 0x3F,
        _ => usize::from(MESSAGE_LENGTH[usize::from(opcode)]),
    }
}

#[cfg(test)]
pub(crate) fn encode(bytes: &[u8], layout: &Layout) -> [u8; 160] {
    let mut bits = Vec::with_capacity(layout.checked_bits());
    for index in 0..layout.pdu_bits {
        bits.push(
            bytes
                .get(index / 8)
                .is_some_and(|byte| byte >> (7 - index % 8) & 1 == 1),
        );
    }
    if layout.crc16 {
        let message = layout.pdu_bits - 16;
        let crc = crc_msb_bits(CRC16_POLY, 16, 0, bits[..message].iter().copied()) ^ 0xFFFF;
        for (offset, bit) in bits[message..].iter_mut().enumerate() {
            *bit = crc >> (15 - offset) & 1 == 1;
        }
    } else {
        let crc = crc_msb_bits(CRC12_POLY, 12, 0, bits.iter().copied()) ^ 0xFFF;
        bits.extend((0..12).rev().map(|shift| crc >> shift & 1 == 1));
    }
    let data_symbols = RS_LENGTH - PARITY;
    let mut data = vec![0u8; data_symbols];
    for (index, chunk) in bits.chunks(6).enumerate() {
        let symbol = chunk
            .iter()
            .enumerate()
            .fold(0u8, |acc, (bit, &set)| acc | u8::from(set) << (5 - bit));
        data[layout.first + index] = symbol;
    }
    let mut codeword = Vec::new();
    RS.encode(&data, &mut codeword);
    let mut body = [0u8; 160];
    let symbols = &codeword[layout.first..layout.first + layout.present];
    for (index, at) in layout.dibits().enumerate() {
        body[at] = symbols[index / 3] >> (4 - 2 * (index % 3)) & 3;
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGROUP: [u8; 11] = [
        0x81, 0x90, 0x0B, 0x0C, 0x82, 0x00, 0xC9, 0x00, 0xC9, 0x00, 0xC9,
    ];

    fn idle_with(message: &[u8]) -> [u8; 21] {
        let mut pdu = [0u8; 21];
        pdu[0] = 3 << 5;
        pdu[1..=message.len()].copy_from_slice(message);
        pdu
    }

    #[test]
    fn a_sacch_round_trips_through_the_code() {
        let body = encode(&idle_with(&REGROUP), &SACCH);
        let pdu = decode(&body, &SACCH).expect("decodes");
        assert_eq!(pdu.kind(), PduKind::Idle);
        assert_eq!(pdu.corrected, 0);
        assert_eq!(
            messages(&pdu).collect::<Vec<_>>(),
            vec![Message::Known(&REGROUP)]
        );
    }

    #[test]
    fn a_facch_corrects_symbol_errors() {
        let mut pdu = [0u8; 18];
        pdu[0] = 4 << 5;
        pdu[1..8].copy_from_slice(&[0x01, 0x00, 0x00, 0xC9, 0x12, 0x34, 0x56]);
        let mut body = encode(&pdu, &FACCH);
        for at in [1, 20, 40, 100] {
            body[at] ^= 3;
        }
        let decoded = decode(&body, &FACCH).expect("corrects");
        assert_eq!(&decoded.bytes()[..8], &pdu[..8]);
        assert!(decoded.corrected > 0);
    }

    #[test]
    fn an_lcch_checks_its_crc16() {
        let mut pdu = [0u8; 23];
        pdu[1] = 0x7B;
        let body = encode(&pdu, &LCCH);
        assert!(decode(&body, &LCCH).is_some());
        let mut wrong = body;
        wrong[30] ^= 1;
        wrong[31] ^= 1;
        wrong[32] ^= 1;
        wrong[60] ^= 2;
        wrong[61] ^= 2;
        wrong[62] ^= 2;
        wrong[90] ^= 1;
        wrong[91] ^= 1;
        assert!(decode(&wrong, &LCCH).is_none_or(|pdu| pdu.bytes()[1] == 0x7B));
    }

    #[test]
    fn a_damaged_crc_is_refused() {
        let pdu = idle_with(&REGROUP);
        let body = encode(&pdu, &SACCH);
        let mut flipped = pdu;
        flipped[5] ^= 0x10;
        let other = encode(&flipped, &SACCH);
        let mut mixed = body;
        mixed[..40].copy_from_slice(&other[..40]);
        assert!(decode(&mixed, &SACCH).is_none_or(|decoded| decoded.bytes() != &flipped[..]));
    }

    #[test]
    fn null_fill_ends_the_messages_and_bias_fill_is_skipped() {
        let mut bytes = [0u8; 21];
        bytes[0] = 4 << 5;
        bytes[1..3].copy_from_slice(&[NULL_AVOID_ZERO_BIAS, 2]);
        bytes[3..10].copy_from_slice(&[0x01, 0x40, 0x00, 0x65, 0x00, 0x00, 0x07]);
        let pdu = MacPdu {
            bytes: {
                let mut padded = [0u8; MAX_PDU_BYTES];
                padded[..21].copy_from_slice(&bytes);
                padded
            },
            len: 21,
            corrected: 0,
        };
        assert_eq!(
            messages(&pdu).collect::<Vec<_>>(),
            vec![Message::Known(&bytes[3..10])]
        );
    }

    #[test]
    fn an_opcode_of_unknown_length_surfaces_the_rest() {
        let mut padded = [0u8; MAX_PDU_BYTES];
        padded[0] = 3 << 5;
        padded[1] = 0x04;
        padded[2] = 0xAA;
        let pdu = MacPdu {
            bytes: padded,
            len: 21,
            corrected: 0,
        };
        let found: Vec<_> = messages(&pdu).collect();
        assert_eq!(found, vec![Message::Unsized(&padded[1..21])]);
    }
}
