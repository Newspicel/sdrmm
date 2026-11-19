use std::fmt;

use sdrmm_dsp::coding::fec::ccsds_rs::{Basis, CCSDS_BLOCK, CcsdsReedSolomon};
use sdrmm_dsp::coding::fec::crc32_ieee;

use super::{CRC_LEN, HEADER_LEN, MAX_PACKET_LEN, PARITY_LEN, SYNC};

const TYPE_BASE: u8 = 0x66;
const NO_MCU: u16 = 0xFFFF;
const NO_OFFSET: u8 = 0xFF;
const MAX_CALLSIGN: usize = 6;
const CALLSIGN_RADIX: u32 = 40;
const LARGEST_CALLSIGN: u32 = 0xF423_FFFF;
const MIN_PAYLOAD: usize = 2;

/// Whether a packet carries Reed-Solomon parity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PacketType {
    /// 32 bytes of RS(255,223) parity close the packet.
    Fec,
    /// The link brings its own error correction.
    NoFec,
}

impl PacketType {
    const fn byte(self) -> u8 {
        match self {
            Self::Fec => TYPE_BASE,
            Self::NoFec => TYPE_BASE + 1,
        }
    }

    /// Scan bytes a packet of `len` bytes carries.
    #[must_use]
    pub const fn payload_len(self, len: usize) -> usize {
        let overhead = match self {
            Self::Fec => HEADER_LEN + CRC_LEN + PARITY_LEN,
            Self::NoFec => HEADER_LEN + CRC_LEN,
        };
        len.saturating_sub(overhead)
    }
}

/// The luma sampling of a minimum coded unit; chroma is always one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum McuMode {
    /// 16x16 pixels, four luma blocks (4:2:0).
    Square,
    /// 8 wide, 16 tall, two luma blocks.
    Tall,
    /// 16 wide, 8 tall, two luma blocks (4:2:2).
    Wide,
    /// 8x8 pixels, one luma block (4:4:4).
    Single,
}

impl McuMode {
    const fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::Square,
            1 => Self::Tall,
            2 => Self::Wide,
            _ => Self::Single,
        }
    }

    const fn bits(self) -> u8 {
        match self {
            Self::Square => 0,
            Self::Tall => 1,
            Self::Wide => 2,
            Self::Single => 3,
        }
    }

    /// Luma blocks across and down one unit.
    #[must_use]
    pub const fn luma_blocks(self) -> (usize, usize) {
        match self {
            Self::Square => (2, 2),
            Self::Tall => (1, 2),
            Self::Wide => (2, 1),
            Self::Single => (1, 1),
        }
    }

    /// Pixels across and down one unit.
    #[must_use]
    pub const fn size(self) -> (usize, usize) {
        let (across, down) = self.luma_blocks();
        (across * 8, down * 8)
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Square => "4:2:0",
            Self::Tall => "4:4:0",
            Self::Wide => "4:2:2",
            Self::Single => "4:4:4",
        }
    }
}

/// The fixed 15-byte header every SSDV packet opens with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub packet_type: PacketType,
    pub callsign: u32,
    pub image_id: u8,
    pub packet_id: u16,
    pub width: u16,
    pub height: u16,
    /// JPEG quality level, 0 to 7.
    pub quality: u8,
    pub eoi: bool,
    pub mcu_mode: McuMode,
    /// Where the first unit that starts in this packet begins, and which unit it is.
    pub first_mcu: Option<(u8, u16)>,
}

impl Header {
    /// Reads the header of `packet`; `None` when it is too short or not SSDV.
    #[must_use]
    pub fn parse(packet: &[u8]) -> Option<Self> {
        let head = packet.get(..HEADER_LEN)?;
        let packet_type = match head[1] {
            byte if byte == PacketType::Fec.byte() => PacketType::Fec,
            byte if byte == PacketType::NoFec.byte() => PacketType::NoFec,
            _ => return None,
        };
        let mcu_id = u16::from_be_bytes([head[13], head[14]]);
        Some(Self {
            packet_type,
            callsign: u32::from_be_bytes([head[2], head[3], head[4], head[5]]),
            image_id: head[6],
            packet_id: u16::from_be_bytes([head[7], head[8]]),
            width: u16::from(head[9]) * 16,
            height: u16::from(head[10]) * 16,
            quality: (head[11] >> 3 & 7) ^ 4,
            eoi: head[11] >> 2 & 1 == 1,
            mcu_mode: McuMode::from_bits(head[11]),
            first_mcu: (mcu_id != NO_MCU).then_some((head[12], mcu_id)),
        })
    }

    pub(super) fn write(&self, packet: &mut [u8]) {
        let (offset, mcu_id) = self.first_mcu.unwrap_or((NO_OFFSET, NO_MCU));
        packet[0] = SYNC;
        packet[1] = self.packet_type.byte();
        packet[2..6].copy_from_slice(&self.callsign.to_be_bytes());
        packet[6] = self.image_id;
        packet[7..9].copy_from_slice(&self.packet_id.to_be_bytes());
        packet[9] = (self.width / 16) as u8;
        packet[10] = (self.height / 16) as u8;
        packet[11] = ((self.quality ^ 4) & 7) << 3 | u8::from(self.eoi) << 2 | self.mcu_mode.bits();
        packet[12] = offset;
        packet[13..15].copy_from_slice(&mcu_id.to_be_bytes());
    }

    /// Units across one row of the picture.
    #[must_use]
    pub const fn mcus_per_row(&self) -> usize {
        self.width as usize / self.mcu_mode.size().0
    }

    #[must_use]
    pub const fn mcu_count(&self) -> usize {
        let (across, down) = self.mcu_mode.size();
        (self.width as usize / across) * (self.height as usize / down)
    }

    #[must_use]
    pub fn callsign(&self) -> String {
        decode_callsign(self.callsign)
    }

    /// Whether `other` belongs to the same picture.
    #[must_use]
    pub fn same_image(&self, other: &Self) -> bool {
        self.callsign == other.callsign
            && self.image_id == other.image_id
            && self.width == other.width
            && self.height == other.height
            && self.mcu_mode == other.mcu_mode
            && self.quality == other.quality
    }
}

/// Packs up to six characters of A-Z, 0-9 into the base-40 header field.
#[must_use]
pub fn encode_callsign(callsign: &str) -> u32 {
    callsign
        .bytes()
        .take(MAX_CALLSIGN)
        .rev()
        .fold(0u32, |code, c| {
            let digit = match c {
                b'A'..=b'Z' => u32::from(c - b'A') + 14,
                b'a'..=b'z' => u32::from(c - b'a') + 14,
                b'0'..=b'9' => u32::from(c - b'0') + 1,
                _ => 0,
            };
            code * CALLSIGN_RADIX + digit
        })
}

#[must_use]
pub fn decode_callsign(mut code: u32) -> String {
    let mut callsign = String::new();
    if code > LARGEST_CALLSIGN {
        return callsign;
    }
    while code > 0 {
        let digit = (code % CALLSIGN_RADIX) as u8;
        callsign.push(match digit {
            1..=10 => char::from(b'0' + digit - 1),
            14.. => char::from(b'A' + digit - 14),
            _ => '-',
        });
        code /= CALLSIGN_RADIX;
    }
    callsign
}

/// Why a packet was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketError {
    /// Shorter than a header, CRC and two scan bytes, or longer than 256.
    Length,
    /// The CRC failed and Reed-Solomon could not repair it.
    Corrupt,
    /// The checks passed but the header describes no possible picture.
    Header,
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "SSDV packet length out of range",
            Self::Corrupt => "SSDV packet failed its CRC",
            Self::Header => "SSDV packet header is invalid",
        })
    }
}

impl std::error::Error for PacketError {}

/// A packet that passed its checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checked {
    pub header: Header,
    /// Bytes Reed-Solomon repaired.
    pub corrected: u32,
}

/// Validates SSDV packets: CRC32, then Reed-Solomon repair for FEC packets.
pub struct PacketChecker {
    rs: CcsdsReedSolomon,
}

impl Default for PacketChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketChecker {
    #[must_use]
    pub fn new() -> Self {
        Self {
            rs: CcsdsReedSolomon::with_basis(Basis::Conventional),
        }
    }

    /// Checks `packet` in place, repairing it when it can.
    ///
    /// # Errors
    /// [`PacketError`] when the packet cannot be trusted.
    pub fn check(&self, packet: &mut [u8]) -> Result<Checked, PacketError> {
        let len = packet.len();
        if !(HEADER_LEN + CRC_LEN + MIN_PAYLOAD..=MAX_PACKET_LEN).contains(&len) {
            return Err(PacketError::Length);
        }
        packet[0] = SYNC;
        let mut corrected = 0;
        let packet_type = match packet[1] {
            byte if byte == PacketType::NoFec.byte() && crc_holds(packet, PacketType::NoFec) => {
                PacketType::NoFec
            }
            byte if byte == PacketType::Fec.byte() && crc_holds(packet, PacketType::Fec) => {
                PacketType::Fec
            }
            _ => {
                corrected = self.repair(packet)?;
                PacketType::Fec
            }
        };
        let header = Header::parse(packet).ok_or(PacketError::Header)?;
        if header.packet_type != packet_type || !plausible(&header, len) {
            return Err(PacketError::Header);
        }
        Ok(Checked { header, corrected })
    }

    fn repair(&self, packet: &mut [u8]) -> Result<u32, PacketError> {
        let len = packet.len();
        if PacketType::Fec.payload_len(len) < MIN_PAYLOAD {
            return Err(PacketError::Corrupt);
        }
        let pad = MAX_PACKET_LEN - len;
        let mut codeword = [0u8; CCSDS_BLOCK];
        codeword[pad..].copy_from_slice(&packet[1..]);
        codeword[pad] = PacketType::Fec.byte();
        let corrected = self.rs.decode(&mut codeword).ok_or(PacketError::Corrupt)?;
        if codeword[..pad].iter().any(|&byte| byte != 0) {
            return Err(PacketError::Corrupt);
        }
        let mut repaired = [0u8; MAX_PACKET_LEN];
        repaired[0] = SYNC;
        repaired[1..len].copy_from_slice(&codeword[pad..]);
        if !crc_holds(&repaired[..len], PacketType::Fec) {
            return Err(PacketError::Corrupt);
        }
        packet.copy_from_slice(&repaired[..len]);
        Ok(corrected)
    }

    /// Writes the CRC and, for FEC packets, the parity of a packet whose header and scan bytes
    /// are in place.
    pub(super) fn seal(&self, packet: &mut [u8], packet_type: PacketType) {
        let len = packet.len();
        let crc_at = HEADER_LEN + packet_type.payload_len(len);
        let crc = crc32_ieee(&packet[1..crc_at]);
        packet[crc_at..crc_at + CRC_LEN].copy_from_slice(&crc.to_be_bytes());
        if packet_type == PacketType::Fec {
            let pad = MAX_PACKET_LEN - len;
            let mut data = [0u8; CCSDS_BLOCK - PARITY_LEN];
            data[pad..].copy_from_slice(&packet[1..len - PARITY_LEN]);
            let mut parity = [0u8; PARITY_LEN];
            self.rs.encode(&data, &mut parity);
            packet[len - PARITY_LEN..].copy_from_slice(&parity);
        }
    }
}

fn crc_holds(packet: &[u8], packet_type: PacketType) -> bool {
    let crc_at = HEADER_LEN + packet_type.payload_len(packet.len());
    packet
        .get(crc_at..crc_at + CRC_LEN)
        .and_then(|bytes| bytes.try_into().ok())
        .is_some_and(|sent: [u8; CRC_LEN]| {
            crc32_ieee(&packet[1..crc_at]) == u32::from_be_bytes(sent)
        })
}

fn plausible(header: &Header, len: usize) -> bool {
    if header.width == 0 || header.height == 0 {
        return false;
    }
    header.first_mcu.is_none_or(|(offset, mcu_id)| {
        usize::from(mcu_id) < header.mcu_count()
            && usize::from(offset) < header.packet_type.payload_len(len)
    })
}
