//! The AMBE-3000R / AMBE-3003 packet format: `0x61`, a big-endian length of everything after
//! the type byte, the type, then fields, each an identifier byte and its data. Packets end in
//! a parity field, `0x2F` and the XOR of every byte after the start byte up to it.

use sdrmm_channels::chip::{CHIP_FRAME_SAMPLES, ChipRate, ChipSpeech};

use crate::ChipError;

pub const START_BYTE: u8 = 0x61;
const HEADER_LEN: usize = 4;
const MAX_PAYLOAD_LEN: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketKind {
    Control = 0x00,
    Channel = 0x01,
    Speech = 0x02,
}

impl PacketKind {
    const fn parse(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::Control),
            0x01 => Some(Self::Channel),
            0x02 => Some(Self::Speech),
            _ => None,
        }
    }
}

pub mod field {
    pub const SPEECH_DATA: u8 = 0x00;
    pub const CHANNEL_DATA: u8 = 0x01;
    pub const RATE_INDEX: u8 = 0x09;
    pub const RATE_WORDS: u8 = 0x0A;
    pub const INIT: u8 = 0x0B;
    pub const PARITY: u8 = 0x2F;
    pub const PRODUCT_ID: u8 = 0x30;
    pub const VERSION: u8 = 0x31;
    pub const RESET: u8 = 0x33;
    pub const READY: u8 = 0x39;
    pub const CHANNEL_0: u8 = 0x40;
    pub const MAX_CHANNELS: u8 = 3;
}

/// What [`field::INIT`] restarts.
pub mod init {
    pub const DECODER: u8 = 0x02;
    pub const BOTH: u8 = 0x03;
}

/// AMBE+2 half rate without FEC: 2450 bit/s of voice.
const HALF_RATE_INDEX: u8 = 34;
/// IMBE without FEC: 4400 bit/s, 88 bits a frame.
const FULL_RATE_WORDS: [u16; 6] = [0x0558, 0x086B, 0x0000, 0x0000, 0x0000, 0x0158];
/// D-STAR AMBE: 2400 bit/s of voice and 1200 of FEC, 72 bits a frame.
const DSTAR_RATE_WORDS: [u16; 6] = [0x0130, 0x0763, 0x4000, 0x0000, 0x0000, 0x0048];

const SPEECH_FIELD_LEN: usize = 2 + 2 * CHIP_FRAME_SAMPLES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub kind: PacketKind,
    /// The fields, parity removed.
    pub fields: Vec<u8>,
}

fn parity(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |acc, byte| acc ^ byte)
}

/// One packet of `kind` holding `fields`, parity included.
#[must_use]
pub fn encode(kind: PacketKind, fields: &[u8]) -> Vec<u8> {
    let length = u16::try_from(fields.len() + 2).unwrap_or(u16::MAX);
    let mut packet = Vec::with_capacity(HEADER_LEN + fields.len() + 2);
    packet.push(START_BYTE);
    packet.extend_from_slice(&length.to_be_bytes());
    packet.push(kind as u8);
    packet.extend_from_slice(fields);
    packet.push(field::PARITY);
    packet.push(parity(&packet[1..]));
    packet
}

/// The field addressing chip channel `channel`, which a single-channel chip goes without.
#[must_use]
pub fn channel_field(channels: u8, channel: u8) -> Option<u8> {
    (channels > 1).then_some(field::CHANNEL_0 + channel)
}

/// The fields that set a chip channel to `rate`.
#[must_use]
pub fn rate_fields(rate: ChipRate) -> Vec<u8> {
    let words = match rate {
        ChipRate::Half => return vec![field::RATE_INDEX, HALF_RATE_INDEX],
        ChipRate::Full => FULL_RATE_WORDS,
        ChipRate::Dstar => DSTAR_RATE_WORDS,
    };
    std::iter::once(field::RATE_WORDS)
        .chain(words.iter().flat_map(|word| word.to_be_bytes()))
        .collect()
}

/// Splits a byte stream into packets, skipping noise before a start byte.
#[derive(Default)]
pub struct Deframer {
    buffer: Vec<u8>,
}

impl Deframer {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn next_packet(&mut self) -> Option<Result<Packet, ChipError>> {
        let start = self.buffer.iter().position(|&byte| byte == START_BYTE);
        self.buffer.drain(..start.unwrap_or(self.buffer.len()));
        let header = self.buffer.get(..HEADER_LEN)?;
        let length = usize::from(u16::from_be_bytes([header[1], header[2]]));
        let kind = header[3];
        if length > MAX_PAYLOAD_LEN {
            self.buffer.drain(..1);
            return Some(Err(ChipError::Protocol(format!("{length} byte packet"))));
        }
        let end = HEADER_LEN + length;
        if self.buffer.len() < end {
            return None;
        }
        let packet: Vec<u8> = self.buffer.drain(..end).collect();
        let Some(kind) = PacketKind::parse(kind) else {
            return Some(Err(ChipError::Protocol(format!("packet type {kind:#04x}"))));
        };
        Some(strip_parity(&packet).map(|fields| Packet {
            kind,
            fields: fields.to_vec(),
        }))
    }
}

fn strip_parity(packet: &[u8]) -> Result<&[u8], ChipError> {
    let fields = &packet[HEADER_LEN..];
    match fields {
        [rest @ .., field::PARITY, sent] => {
            let expected = parity(&packet[1..packet.len() - 1]);
            if *sent == expected {
                Ok(rest)
            } else {
                Err(ChipError::Protocol(format!(
                    "parity {sent:#04x}, expected {expected:#04x}"
                )))
            }
        }
        _ => Ok(fields),
    }
}

/// The text a [`field::PRODUCT_ID`] or [`field::VERSION`] answer carries.
pub fn text(fields: &[u8], id: u8) -> Result<String, ChipError> {
    let rest = match fields {
        [first, rest @ ..] if *first == id => rest,
        _ => return Err(ChipError::Protocol(format!("no field {id:#04x} in answer"))),
    };
    let end = rest
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(rest.len());
    Ok(String::from_utf8_lossy(&rest[..end]).trim().to_owned())
}

/// Checks a control answer: each field the request named, with a zero status after it.
pub fn statuses(fields: &[u8], expected: &[u8]) -> Result<(), ChipError> {
    let mut pairs = fields.as_chunks::<2>().0.iter();
    for &id in expected {
        match pairs.next() {
            Some(&[got, 0]) if got == id => {}
            Some(&[got, status]) if got == id => {
                return Err(ChipError::Refused { field: id, status });
            }
            _ => {
                return Err(ChipError::Protocol(format!(
                    "no status for field {id:#04x}"
                )));
            }
        }
    }
    Ok(())
}

/// The chip channel a speech answer came from, if it names one, and its samples.
pub fn speech(fields: &[u8]) -> Result<(Option<u8>, ChipSpeech), ChipError> {
    let (channel, rest) = match fields {
        [id, rest @ ..]
            if (field::CHANNEL_0..field::CHANNEL_0 + field::MAX_CHANNELS).contains(id) =>
        {
            (Some(id - field::CHANNEL_0), rest)
        }
        _ => (None, fields),
    };
    let rest = if rest.len() == SPEECH_FIELD_LEN + 1 {
        &rest[1..]
    } else {
        rest
    };
    let samples = match rest {
        [field::SPEECH_DATA, count, samples @ ..]
            if usize::from(*count) == CHIP_FRAME_SAMPLES
                && samples.len() == 2 * CHIP_FRAME_SAMPLES =>
        {
            samples
        }
        _ => {
            return Err(ChipError::Protocol(format!(
                "{} byte speech answer",
                fields.len()
            )));
        }
    };
    let mut speech = [0i16; CHIP_FRAME_SAMPLES];
    for (sample, pair) in speech.iter_mut().zip(samples.as_chunks::<2>().0) {
        *sample = i16::from_be_bytes(*pair);
    }
    Ok((channel, speech))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reset_carries_its_length_type_and_parity() {
        assert_eq!(
            encode(PacketKind::Control, &[field::RESET]),
            [0x61, 0x00, 0x03, 0x00, 0x33, 0x2F, 0x1F]
        );
    }

    #[test]
    fn packets_survive_noise_and_split_reads() {
        let packet = encode(PacketKind::Control, &[field::PRODUCT_ID, b'A', 0]);
        let mut deframer = Deframer::default();
        deframer.push(&[0x00, 0xFF]);
        deframer.push(&packet[..3]);
        assert!(deframer.next_packet().is_none());
        deframer.push(&packet[3..]);
        let parsed = deframer.next_packet().expect("a packet").expect("valid");
        assert_eq!(parsed.kind, PacketKind::Control);
        assert_eq!(text(&parsed.fields, field::PRODUCT_ID).expect("text"), "A");
        assert!(deframer.next_packet().is_none());
    }

    #[test]
    fn a_packet_without_parity_reads_as_sent() {
        let mut deframer = Deframer::default();
        deframer.push(&[0x61, 0x00, 0x01, 0x00, field::READY]);
        let parsed = deframer.next_packet().expect("a packet").expect("valid");
        assert_eq!(parsed.fields, [field::READY]);
    }

    #[test]
    fn bad_parity_is_reported() {
        let mut packet = encode(PacketKind::Control, &[field::READY]);
        let last = packet.len() - 1;
        packet[last] ^= 1;
        let mut deframer = Deframer::default();
        deframer.push(&packet);
        assert!(matches!(
            deframer.next_packet(),
            Some(Err(ChipError::Protocol(_)))
        ));
    }

    #[test]
    fn rates_set_the_documented_words() {
        assert_eq!(rate_fields(ChipRate::Half), [0x09, 34]);
        let full = rate_fields(ChipRate::Full);
        assert_eq!(full.len(), 13);
        assert_eq!(full[..3], [0x0A, 0x05, 0x58]);
        assert_eq!(
            full[12], 88,
            "the last word's low byte is the frame's bit count"
        );
        assert_eq!(rate_fields(ChipRate::Dstar)[12], 72);
    }

    #[test]
    fn statuses_name_the_refused_field() {
        assert!(statuses(&[0x09, 0, 0x0B, 0], &[0x09, 0x0B]).is_ok());
        assert!(matches!(
            statuses(&[0x09, 0, 0x0B, 5], &[0x09, 0x0B]),
            Err(ChipError::Refused {
                field: 0x0B,
                status: 5
            })
        ));
    }

    fn speech_fields(prefix: &[u8]) -> Vec<u8> {
        let mut fields = prefix.to_vec();
        fields.extend([field::SPEECH_DATA, 160]);
        fields.extend((0..160i16).flat_map(|i| (i * 3 - 200).to_be_bytes()));
        fields
    }

    #[test]
    fn speech_reads_with_or_without_a_channel() {
        let (channel, samples) = speech(&speech_fields(&[])).expect("speech");
        assert_eq!((channel, samples[0], samples[159]), (None, -200, 277));
        let (channel, _) = speech(&speech_fields(&[0x42])).expect("speech");
        assert_eq!(channel, Some(2));
        let (channel, samples) = speech(&speech_fields(&[0x41, 0x00])).expect("speech");
        assert_eq!((channel, samples[1]), (Some(1), -197));
    }

    #[test]
    fn short_speech_is_an_error() {
        assert!(speech(&[field::SPEECH_DATA, 160, 0, 0]).is_err());
    }

    #[test]
    fn only_a_multi_channel_chip_addresses_channels() {
        assert_eq!(channel_field(1, 0), None);
        assert_eq!(channel_field(3, 2), Some(0x42));
    }
}
