use std::fmt;

use crate::jpeg::{
    BLOCK_PIXELS, BLOCK_SIDE, BitWriter, Block, Tables, ZIGZAG, encode_block, forward_dct,
};

use super::{
    HEADER_LEN, Header, MAX_PACKET_LEN, MAX_SIDE, McuMode, PacketChecker, PacketType, Quant,
    encode_callsign, ycbcr,
};

const MIN_PAYLOAD: usize = 2;
const FILL_STEP: u8 = 245;
const FILL_ADD: u8 = 45;

/// What [`encode`] makes the packets of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncoderSettings {
    pub callsign: String,
    pub image_id: u8,
    /// JPEG quality level, 0 to 7.
    pub quality: u8,
    pub packet_type: PacketType,
    pub mcu_mode: McuMode,
    /// Bytes per packet, at most 256.
    pub packet_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// Width and height must be multiples of 16, at most 4080.
    Size,
    /// The pixel buffer does not hold width x height RGB pixels.
    Pixels,
    /// The packet length leaves no room for scan data.
    PacketLength,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Size => "SSDV pictures are multiples of 16 pixels, at most 4080",
            Self::Pixels => "pixel buffer does not match the picture size",
            Self::PacketLength => "SSDV packet too short for scan data",
        })
    }
}

impl std::error::Error for EncodeError {}

struct Planes {
    width: usize,
    planes: [Vec<f32>; 3],
}

impl Planes {
    fn new(width: usize, rgb: &[u8]) -> Self {
        let mut planes: [Vec<f32>; 3] = std::array::from_fn(|_| Vec::with_capacity(rgb.len() / 3));
        for pixel in rgb.as_chunks::<3>().0 {
            for (plane, value) in planes.iter_mut().zip(ycbcr(*pixel)) {
                plane.push(value);
            }
        }
        Self { width, planes }
    }

    /// The block of `component` whose top left is (`left`, `top`), each sample the mean of a
    /// `step_x` by `step_y` patch.
    fn block(&self, component: usize, left: usize, top: usize, step: (usize, usize)) -> Block {
        let plane = &self.planes[component];
        let area = (step.0 * step.1) as f32;
        std::array::from_fn(|index| {
            let (y, x) = (index / BLOCK_SIDE, index % BLOCK_SIDE);
            let mut sum = 0.0;
            for dy in 0..step.1 {
                for dx in 0..step.0 {
                    sum += plane[(top + y * step.1 + dy) * self.width + left + x * step.0 + dx];
                }
            }
            (sum / area).round().clamp(0.0, 255.0) as u8
        })
    }
}

/// One unit's blocks, quantised, in zig-zag order: the luma blocks then Cb and Cr.
fn unit_blocks(planes: &Planes, quant: &Quant, mode: McuMode, unit: usize) -> Vec<[i32; 64]> {
    let (unit_w, unit_h) = mode.size();
    let (across, down) = mode.luma_blocks();
    let per_row = planes.width / unit_w;
    let (left, top) = (unit % per_row * unit_w, unit / per_row * unit_h);
    let mut blocks = Vec::with_capacity(across * down + 2);
    for row in 0..down {
        for column in 0..across {
            let block = planes.block(
                0,
                left + column * BLOCK_SIDE,
                top + row * BLOCK_SIDE,
                (1, 1),
            );
            blocks.push(quantised(&block, &quant.luma));
        }
    }
    for component in 1..3 {
        let block = planes.block(component, left, top, (across, down));
        blocks.push(quantised(&block, &quant.chroma));
    }
    blocks
}

fn quantised(block: &Block, steps: &[f32; BLOCK_PIXELS]) -> [i32; BLOCK_PIXELS] {
    let coefficients = forward_dct(block);
    std::array::from_fn(|k| {
        let natural = ZIGZAG[k];
        (coefficients[natural] / steps[natural]).round() as i32
    })
}

/// The scan bits and, per packet, the first unit that starts in it.
struct Scan {
    writer: BitWriter,
    payload: usize,
    first: Vec<Option<(u8, u16)>>,
    dc: [i32; 3],
}

impl Scan {
    /// Starts unit `unit` byte aligned when no unit starts yet in the packet being filled; its
    /// DC values are then absolute.
    fn open_unit(&mut self, unit: usize) -> bool {
        let filling = self.writer.len() / 8 / self.payload;
        if self.first.get(filling).copied().flatten().is_some() {
            return false;
        }
        self.writer.align();
        let at = self.writer.len() / 8;
        let packet = at / self.payload;
        if self.first.len() <= packet {
            self.first.resize(packet + 1, None);
        }
        self.first[packet] = Some(((at % self.payload) as u8, unit as u16));
        true
    }

    fn write_unit(&mut self, blocks: &[[i32; 64]], absolute: bool) {
        let luma_blocks = blocks.len() - 2;
        for (part, zigzag) in blocks.iter().enumerate() {
            let component = part.saturating_sub(luma_blocks - 1);
            let opens_component = part == 0 || part >= luma_blocks;
            let difference = if absolute && opens_component {
                zigzag[0]
            } else {
                zigzag[0] - self.dc[component]
            };
            self.dc[component] = zigzag[0];
            let tables = if component == 0 {
                Tables::luma()
            } else {
                Tables::chroma()
            };
            encode_block(&mut self.writer, difference, zigzag, tables);
        }
    }
}

/// Encodes an RGB picture, three bytes a pixel row by row, into SSDV packets.
///
/// # Errors
/// [`EncodeError`] when the picture or the packet length is out of range.
pub fn encode(
    settings: &EncoderSettings,
    width: u16,
    height: u16,
    rgb: &[u8],
) -> Result<Vec<Vec<u8>>, EncodeError> {
    let valid = |side: u16| side > 0 && side.is_multiple_of(16) && side <= MAX_SIDE;
    if !valid(width) || !valid(height) {
        return Err(EncodeError::Size);
    }
    let (w, h) = (usize::from(width), usize::from(height));
    if rgb.len() != w * h * 3 {
        return Err(EncodeError::Pixels);
    }
    let payload = settings.packet_type.payload_len(settings.packet_len);
    if settings.packet_len > MAX_PACKET_LEN || payload < MIN_PAYLOAD {
        return Err(EncodeError::PacketLength);
    }
    let quality = settings.quality.min(7);
    let quant = Quant::new(quality);
    let planes = Planes::new(w, rgb);
    let header = Header {
        packet_type: settings.packet_type,
        callsign: encode_callsign(&settings.callsign),
        image_id: settings.image_id,
        packet_id: 0,
        width,
        height,
        quality,
        eoi: false,
        mcu_mode: settings.mcu_mode,
        first_mcu: None,
    };
    let mut scan = Scan {
        writer: BitWriter::default(),
        payload,
        first: Vec::new(),
        dc: [0; 3],
    };
    for unit in 0..header.mcu_count() {
        let absolute = scan.open_unit(unit);
        scan.write_unit(
            &unit_blocks(&planes, &quant, settings.mcu_mode, unit),
            absolute,
        );
    }
    Ok(packets(&header, scan, settings.packet_len))
}

fn packets(header: &Header, scan: Scan, packet_len: usize) -> Vec<Vec<u8>> {
    let checker = PacketChecker::new();
    let payload = scan.payload;
    let bytes = scan.writer.finish();
    let count = bytes.len().div_ceil(payload);
    let mut fill = 0u8;
    (0..count)
        .map(|index| {
            let mut packet = vec![0u8; packet_len];
            Header {
                packet_id: index as u16,
                eoi: index + 1 == count,
                first_mcu: scan.first.get(index).copied().flatten(),
                ..*header
            }
            .write(&mut packet);
            let chunk = &bytes[index * payload..bytes.len().min((index + 1) * payload)];
            let body = &mut packet[HEADER_LEN..HEADER_LEN + payload];
            body[..chunk.len()].copy_from_slice(chunk);
            for byte in &mut body[chunk.len()..] {
                fill = fill.wrapping_mul(FILL_STEP).wrapping_add(FILL_ADD);
                *byte = fill;
            }
            checker.seal(&mut packet, header.packet_type);
            packet
        })
        .collect()
}
