use crate::jpeg::{
    BLOCK_PIXELS, BLOCK_SIDE, BitReader, Block, CodedBlock, Tables, ZIGZAG, decode_block,
    inverse_dct,
};

use super::{HEADER_LEN, Header, Quant, rgb};

/// The longest run of scan bytes one unit may span before it counts as broken.
const MAX_PENDING: usize = 16 * 1024;
const MAX_BLOCKS: usize = 6;

/// How a picture's packets have fared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Packets drawn from.
    pub packets: u32,
    /// Packet ids skipped over.
    pub lost: u32,
    /// Packets that arrived again or out of order and were ignored.
    pub repeated: u32,
    /// Times the scan data failed to decode and drawing waited for the next packet.
    pub broken: u32,
    /// Units drawn.
    pub drawn: usize,
    /// Units in the picture.
    pub units: usize,
}

enum Unit {
    Drawn,
    NeedsMore,
    Broken,
}

/// Draws one picture from its packets as they arrive, in packet order. A lost packet leaves its
/// units black; drawing picks up at the first unit the next packet names.
pub struct Assembler {
    header: Option<Header>,
    quant: Quant,
    rgb: Vec<u8>,
    luma: Vec<u8>,
    pending: Vec<u8>,
    pending_bit: usize,
    last_packet: Option<u16>,
    next_unit: usize,
    in_sync: bool,
    absolute_dc: bool,
    dc: [i32; 3],
    progress: Progress,
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

impl Assembler {
    #[must_use]
    pub fn new() -> Self {
        Self {
            header: None,
            quant: Quant::new(4),
            rgb: Vec::new(),
            luma: Vec::new(),
            pending: Vec::with_capacity(MAX_PENDING),
            pending_bit: 0,
            last_packet: None,
            next_unit: 0,
            in_sync: false,
            absolute_dc: false,
            dc: [0; 3],
            progress: Progress::default(),
        }
    }

    /// The picture being drawn.
    #[must_use]
    pub const fn header(&self) -> Option<&Header> {
        self.header.as_ref()
    }

    /// Clears the canvas for the picture `header` describes.
    pub fn start(&mut self, header: &Header) {
        let pixels = usize::from(header.width) * usize::from(header.height);
        self.header = Some(*header);
        self.quant = Quant::new(header.quality);
        self.rgb.clear();
        self.rgb.resize(pixels * 3, 0);
        self.luma.clear();
        self.luma.resize(pixels, 0);
        self.pending.clear();
        self.pending_bit = 0;
        self.last_packet = None;
        self.next_unit = 0;
        self.in_sync = false;
        self.absolute_dc = false;
        self.dc = [0; 3];
        self.progress = Progress {
            units: header.mcu_count(),
            ..Progress::default()
        };
    }

    /// Forgets the picture.
    pub fn clear(&mut self) {
        self.header = None;
        self.progress = Progress::default();
    }

    /// Draws a checked packet of the picture [`Assembler::start`] began.
    pub fn feed(&mut self, packet: &[u8], header: &Header) {
        let Some(image) = self.header else {
            return;
        };
        let payload_len = header.packet_type.payload_len(packet.len());
        let Some(payload) = packet.get(HEADER_LEN..HEADER_LEN + payload_len) else {
            return;
        };
        if !self.in_order(header.packet_id) {
            return;
        }
        if self.in_sync {
            let head = header
                .first_mcu
                .map_or(payload.len(), |(offset, _)| usize::from(offset));
            self.append(&payload[..head.min(payload.len())]);
            self.draw_pending(&image);
        }
        let Some((offset, unit)) = header.first_mcu else {
            return;
        };
        if self.in_sync && self.next_unit != usize::from(unit) {
            self.progress.broken += 1;
        }
        self.next_unit = usize::from(unit);
        self.pending.clear();
        self.pending_bit = 0;
        self.absolute_dc = true;
        self.in_sync = true;
        self.append(&payload[usize::from(offset)..]);
        self.draw_pending(&image);
    }

    fn in_order(&mut self, packet_id: u16) -> bool {
        let expected = self.last_packet.map_or(0, |last| u32::from(last) + 1);
        if u32::from(packet_id) < expected {
            self.progress.repeated += 1;
            return false;
        }
        if u32::from(packet_id) > expected {
            self.progress.lost += u32::from(packet_id) - expected;
            self.in_sync = false;
        }
        self.last_packet = Some(packet_id);
        self.progress.packets += 1;
        true
    }

    fn append(&mut self, bytes: &[u8]) {
        if self.pending.len() + bytes.len() > MAX_PENDING {
            self.lose_sync();
            return;
        }
        self.pending.extend_from_slice(bytes);
    }

    fn lose_sync(&mut self) {
        self.progress.broken += 1;
        self.in_sync = false;
        self.pending.clear();
        self.pending_bit = 0;
    }

    fn draw_pending(&mut self, image: &Header) {
        while self.in_sync && self.next_unit < image.mcu_count() {
            match self.draw_unit(image) {
                Unit::Drawn => {}
                Unit::NeedsMore => break,
                Unit::Broken => self.lose_sync(),
            }
        }
        let consumed = (self.pending_bit / 8).min(self.pending.len());
        self.pending.drain(..consumed);
        self.pending_bit -= consumed * 8;
    }

    fn draw_unit(&mut self, image: &Header) -> Unit {
        let (across, down) = image.mcu_mode.luma_blocks();
        let luma_blocks = across * down;
        let mut reader = BitReader::from_bit(&self.pending, self.pending_bit);
        let mut coded = [CodedBlock {
            dc_difference: 0,
            zigzag: [0; BLOCK_PIXELS],
        }; MAX_BLOCKS];
        for (part, slot) in coded.iter_mut().take(luma_blocks + 2).enumerate() {
            let tables = if part < luma_blocks {
                Tables::luma()
            } else {
                Tables::chroma()
            };
            match decode_block(&mut reader, tables) {
                Some(block) => *slot = block,
                None if reader.exhausted() => return Unit::NeedsMore,
                None => return Unit::Broken,
            }
        }
        self.pending_bit = reader.position();
        let blocks = self.reconstruct(&coded[..luma_blocks + 2], luma_blocks);
        self.paint(image, &blocks, across);
        self.next_unit += 1;
        self.progress.drawn += 1;
        Unit::Drawn
    }

    fn reconstruct(&mut self, coded: &[CodedBlock], luma_blocks: usize) -> [Block; MAX_BLOCKS] {
        let mut blocks = [[0u8; BLOCK_PIXELS]; MAX_BLOCKS];
        for (part, (block, out)) in coded.iter().zip(&mut blocks).enumerate() {
            let component = part.saturating_sub(luma_blocks - 1);
            let opens_component = part == 0 || part >= luma_blocks;
            self.dc[component] = if self.absolute_dc && opens_component {
                block.dc_difference
            } else {
                self.dc[component] + block.dc_difference
            };
            let steps = self.quant.component(component);
            let mut coefficients = [0f32; BLOCK_PIXELS];
            coefficients[0] = self.dc[component] as f32 * steps[0];
            for (k, &value) in block.zigzag.iter().enumerate().skip(1) {
                let natural = ZIGZAG[k];
                coefficients[natural] = value as f32 * steps[natural];
            }
            inverse_dct(&coefficients, out);
        }
        self.absolute_dc = false;
        blocks
    }

    fn paint(&mut self, image: &Header, blocks: &[Block; MAX_BLOCKS], across: usize) {
        let (unit_w, unit_h) = image.mcu_mode.size();
        let luma_blocks = across * (unit_h / BLOCK_SIDE);
        let per_row = image.mcus_per_row();
        let width = usize::from(image.width);
        let left = self.next_unit % per_row * unit_w;
        let top = self.next_unit / per_row * unit_h;
        let (cb, cr) = (&blocks[luma_blocks], &blocks[luma_blocks + 1]);
        for y in 0..unit_h {
            for x in 0..unit_w {
                let block = &blocks[y / BLOCK_SIDE * across + x / BLOCK_SIDE];
                let luma = block[y % BLOCK_SIDE * BLOCK_SIDE + x % BLOCK_SIDE];
                let chroma = y * BLOCK_SIDE / unit_h * BLOCK_SIDE + x * BLOCK_SIDE / unit_w;
                let at = (top + y) * width + left + x;
                self.luma[at] = luma;
                self.rgb[at * 3..at * 3 + 3].copy_from_slice(&rgb(luma, cb[chroma], cr[chroma]));
            }
        }
    }

    #[must_use]
    pub const fn progress(&self) -> Progress {
        self.progress
    }

    /// Whether every unit has been drawn.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.header.is_some() && self.progress.drawn == self.progress.units
    }

    /// Pixel rows down to the last unit drawn.
    #[must_use]
    pub fn lines(&self) -> u16 {
        let Some(image) = self.header else {
            return 0;
        };
        let rows = self.next_unit.div_ceil(image.mcus_per_row());
        let lines = rows * image.mcu_mode.size().1;
        u16::try_from(lines).unwrap_or(u16::MAX).min(image.height)
    }

    /// The canvas, three bytes a pixel, row by row.
    #[must_use]
    pub fn rgb(&self) -> &[u8] {
        &self.rgb
    }

    /// The luma plane, row by row.
    #[must_use]
    pub fn luma(&self) -> &[u8] {
        &self.luma
    }
}
