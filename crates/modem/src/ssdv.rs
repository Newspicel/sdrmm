//! SSDV, the packetised JPEG of amateur balloon and satellite downlinks: 256-byte packets, each
//! a header, a slice of baseline JPEG scan data, a CRC32 and optionally RS(255,223) parity.
//! Every packet names the first coding unit that starts in it, so a lost packet costs only the
//! units it carried.
//!
//! [`PacketChecker`] validates and repairs packets, [`Assembler`] draws them into a picture as
//! they arrive, [`encode`] makes them.

mod assemble;
mod encode;
mod packet;
#[cfg(test)]
mod tests;

pub use assemble::{Assembler, Progress};
pub use encode::{EncodeError, EncoderSettings, encode};
pub use packet::{
    Checked, Header, McuMode, PacketChecker, PacketError, PacketType, decode_callsign,
    encode_callsign,
};

use crate::jpeg::{BLOCK_PIXELS, ZIGZAG};

/// The length of a standard packet.
pub const MAX_PACKET_LEN: usize = 256;
pub const HEADER_LEN: usize = 15;
pub const CRC_LEN: usize = 4;
pub const PARITY_LEN: usize = 32;
/// The first byte of every packet.
pub const SYNC: u8 = 0x55;
/// The largest side SSDV can describe.
pub const MAX_SIDE: u16 = 4_080;

const LUMA_QUANT: [u8; BLOCK_PIXELS] = [
    0x10, 0x0C, 0x0C, 0x0E, 0x0C, 0x0A, 0x10, 0x0E, 0x0E, 0x0E, 0x12, 0x12, 0x10, 0x14, 0x18, 0x28,
    0x1A, 0x18, 0x16, 0x16, 0x18, 0x32, 0x24, 0x26, 0x1E, 0x28, 0x3A, 0x34, 0x3E, 0x3C, 0x3A, 0x34,
    0x38, 0x38, 0x40, 0x48, 0x5C, 0x4E, 0x40, 0x44, 0x58, 0x46, 0x38, 0x38, 0x50, 0x6E, 0x52, 0x58,
    0x60, 0x62, 0x68, 0x68, 0x68, 0x3E, 0x4E, 0x72, 0x7A, 0x70, 0x64, 0x78, 0x5C, 0x66, 0x68, 0x64,
];

const CHROMA_QUANT: [u8; BLOCK_PIXELS] = [
    0x12, 0x12, 0x12, 0x16, 0x16, 0x16, 0x30, 0x1A, 0x1A, 0x30, 0x64, 0x42, 0x38, 0x42, 0x64, 0x64,
    0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64,
    0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64,
    0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64, 0x64,
];

/// Percent scale of the base tables per quality level 0 to 7.
const QUALITY_SCALES: [u32; 8] = [5_000, 357, 172, 116, 100, 58, 28, 0];

/// Dequantisation steps in natural order, one table for luma and one for chroma.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Quant {
    pub(crate) luma: [f32; BLOCK_PIXELS],
    pub(crate) chroma: [f32; BLOCK_PIXELS],
}

impl Quant {
    pub(crate) fn new(quality: u8) -> Self {
        Self {
            luma: scaled(&LUMA_QUANT, quality),
            chroma: scaled(&CHROMA_QUANT, quality),
        }
    }

    pub(crate) const fn component(&self, component: usize) -> &[f32; BLOCK_PIXELS] {
        if component == 0 {
            &self.luma
        } else {
            &self.chroma
        }
    }
}

fn scaled(base: &[u8; BLOCK_PIXELS], quality: u8) -> [f32; BLOCK_PIXELS] {
    let scale = QUALITY_SCALES[usize::from(quality.min(7))];
    let mut natural = [0f32; BLOCK_PIXELS];
    for (&step, &position) in base.iter().zip(&ZIGZAG) {
        let value = ((u32::from(step) * scale + 50) / 100).clamp(1, 255);
        natural[position] = value as f32;
    }
    natural
}

/// JFIF full-range `Y'CbCr` to RGB.
pub(crate) fn rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let (y, cb, cr) = (f32::from(y), f32::from(cb) - 128.0, f32::from(cr) - 128.0);
    let clamp = |value: f32| value.round().clamp(0.0, 255.0) as u8;
    [
        clamp(y + 1.402 * cr),
        clamp(y - 0.344_136 * cb - 0.714_136 * cr),
        clamp(y + 1.772 * cb),
    ]
}

/// RGB to JFIF full-range `Y'CbCr`.
pub(crate) fn ycbcr(pixel: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = pixel.map(f32::from);
    [
        0.299 * r + 0.587 * g + 0.114 * b,
        128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b,
        128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b,
    ]
}
