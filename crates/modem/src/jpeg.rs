//! Baseline JPEG building blocks: the zig-zag order, the ITU T.81 Annex K Huffman tables, block
//! entropy coding and the 8x8 DCT pair. Picture formats that carry JPEG-coded blocks (Meteor
//! LRPT, SSDV) build on these.

use std::sync::LazyLock;

mod jfif;

pub use jfif::{JpegError, JpegGrey, encode_grey};

pub const BLOCK_SIDE: usize = 8;
pub const BLOCK_PIXELS: usize = BLOCK_SIDE * BLOCK_SIDE;

/// Eight-bit samples of one 8x8 block, row by row.
pub type Block = [u8; BLOCK_PIXELS];

/// The natural (row-major) index of each zig-zag position.
pub const ZIGZAG: [usize; BLOCK_PIXELS] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

const END_OF_BLOCK: u8 = 0x00;
const ZERO_RUN: u8 = 0xF0;
const MAX_DC_CATEGORY: u8 = 11;
const MAX_AC_CATEGORY: u8 = 10;
const MAX_DC: i32 = 2_047;
const MAX_AC: i32 = 1_023;

/// A Huffman table as a JPEG DHT segment gives it: codes per length 1 to 16, then the symbols.
pub struct HuffmanSpec {
    pub counts: [u8; 16],
    pub symbols: &'static [u8],
}

const DC_SYMBOLS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

pub const LUMA_DC: HuffmanSpec = HuffmanSpec {
    counts: [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
    symbols: &DC_SYMBOLS,
};

pub const CHROMA_DC: HuffmanSpec = HuffmanSpec {
    counts: [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
    symbols: &DC_SYMBOLS,
};

pub const LUMA_AC: HuffmanSpec = HuffmanSpec {
    counts: [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D],
    symbols: &[
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61,
        0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52,
        0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25,
        0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45,
        0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64,
        0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83,
        0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99,
        0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6,
        0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3,
        0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8,
        0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
    ],
};

pub const CHROMA_AC: HuffmanSpec = HuffmanSpec {
    counts: [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77],
    symbols: &[
        0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61,
        0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33,
        0x52, 0xF0, 0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18,
        0x19, 0x1A, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44,
        0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63,
        0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A,
        0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
        0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4,
        0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA,
        0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7,
        0xE8, 0xE9, 0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
    ],
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code {
    pub bits: u16,
    pub len: u8,
}

/// A canonical Huffman table, for decoding and encoding.
pub struct Huffman {
    max_code: [i32; 17],
    offset: [i32; 17],
    symbols: [u8; 256],
    codes: [Code; 256],
}

impl Huffman {
    #[must_use]
    pub fn new(spec: &HuffmanSpec) -> Self {
        Self::build(&spec.counts, spec.symbols)
    }

    /// A table as a DHT segment defines it; `None` when the counts and symbols disagree.
    #[must_use]
    pub fn from_segment(counts: &[u8; 16], symbols: &[u8]) -> Option<Self> {
        let total: usize = counts.iter().map(|&count| usize::from(count)).sum();
        (total == symbols.len() && total <= 256).then(|| Self::build(counts, symbols))
    }

    fn build(counts: &[u8; 16], symbols: &[u8]) -> Self {
        let mut table = Self {
            max_code: [-1; 17],
            offset: [0; 17],
            symbols: [0; 256],
            codes: [Code { bits: 0, len: 0 }; 256],
        };
        let mut code = 0i32;
        let mut index = 0usize;
        for len in 1..=16 {
            let count = usize::from(counts[len - 1]);
            table.offset[len] = index as i32 - code;
            for _ in 0..count {
                let Some(&symbol) = symbols.get(index) else {
                    return table;
                };
                table.symbols[index.min(255)] = symbol;
                table.codes[usize::from(symbol)] = Code {
                    bits: code as u16,
                    len: len as u8,
                };
                index += 1;
                code += 1;
            }
            table.max_code[len] = if count == 0 { -1 } else { code - 1 };
            code <<= 1;
        }
        table
    }

    /// The code of `symbol`; zero length when the table lacks it.
    #[must_use]
    pub fn code(&self, symbol: u8) -> Code {
        self.codes[usize::from(symbol)]
    }

    /// The next symbol, or `None` when the bits run out or match no code.
    pub fn decode(&self, reader: &mut BitReader<'_>) -> Option<u8> {
        let mut code = 0i32;
        for len in 1..=16 {
            code = code << 1 | i32::from(reader.bit()?);
            if code <= self.max_code[len] {
                return self
                    .symbols
                    .get(usize::try_from(code + self.offset[len]).ok()?)
                    .copied();
            }
        }
        None
    }
}

pub static LUMA_DC_TABLE: LazyLock<Huffman> = LazyLock::new(|| Huffman::new(&LUMA_DC));
pub static LUMA_AC_TABLE: LazyLock<Huffman> = LazyLock::new(|| Huffman::new(&LUMA_AC));
pub static CHROMA_DC_TABLE: LazyLock<Huffman> = LazyLock::new(|| Huffman::new(&CHROMA_DC));
pub static CHROMA_AC_TABLE: LazyLock<Huffman> = LazyLock::new(|| Huffman::new(&CHROMA_AC));

/// The DC and AC tables one component is coded with.
#[derive(Clone, Copy)]
pub struct Tables<'a> {
    pub dc: &'a Huffman,
    pub ac: &'a Huffman,
}

impl Tables<'static> {
    #[must_use]
    pub fn luma() -> Self {
        Self {
            dc: &LUMA_DC_TABLE,
            ac: &LUMA_AC_TABLE,
        }
    }

    #[must_use]
    pub fn chroma() -> Self {
        Self {
            dc: &CHROMA_DC_TABLE,
            ac: &CHROMA_AC_TABLE,
        }
    }
}

/// Reads bits most significant first.
pub struct BitReader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> BitReader<'a> {
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    /// A reader that starts `at` bits into `data`.
    #[must_use]
    pub fn from_bit(data: &'a [u8], at: usize) -> Self {
        Self { data, at }
    }

    /// Bits `data` holds in all.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.data.len() * 8
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Whether every bit has been read.
    #[must_use]
    pub const fn exhausted(&self) -> bool {
        self.at >= self.data.len() * 8
    }

    /// Bits read so far.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.at
    }

    pub fn bit(&mut self) -> Option<bool> {
        let byte = self.data.get(self.at / 8)?;
        let bit = byte >> (7 - self.at % 8) & 1 == 1;
        self.at += 1;
        Some(bit)
    }

    pub fn bits(&mut self, count: u8) -> Option<u16> {
        (0..count).try_fold(0u16, |acc, _| Some(acc << 1 | u16::from(self.bit()?)))
    }
}

/// Writes bits most significant first; JPEG pads a partial byte with ones.
#[derive(Default)]
pub struct BitWriter {
    bytes: Vec<u8>,
    filled: u8,
}

impl BitWriter {
    /// Bits written so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len() * 8 - usize::from((8 - self.filled) % 8)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn bit(&mut self, bit: bool) {
        if self.filled == 0 {
            self.bytes.push(0);
        }
        if bit && let Some(last) = self.bytes.last_mut() {
            *last |= 0x80 >> self.filled;
        }
        self.filled = (self.filled + 1) % 8;
    }

    pub fn bits(&mut self, value: u16, count: u8) {
        for shift in (0..count).rev() {
            self.bit(value >> shift & 1 == 1);
        }
    }

    pub fn code(&mut self, code: Code) {
        self.bits(code.bits, code.len);
    }

    /// Pads the partial byte with ones.
    pub fn align(&mut self) {
        while self.filled != 0 {
            self.bit(true);
        }
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn finish(mut self) -> Vec<u8> {
        self.align();
        self.bytes
    }
}

/// The bits needed for `value`'s magnitude: its JPEG size category.
#[must_use]
pub fn magnitude_category(value: i32) -> u8 {
    (32 - value.unsigned_abs().leading_zeros()) as u8
}

/// The `category` bits that carry `value`, ones' complement when negative.
#[must_use]
pub fn magnitude_bits(value: i32, category: u8) -> u16 {
    if value >= 0 {
        value as u16
    } else {
        (value + (1 << category) - 1) as u16
    }
}

/// The value `category` raw bits carry.
#[must_use]
pub fn extend(raw: u16, category: u8) -> i32 {
    if category == 0 {
        return 0;
    }
    let raw = i32::from(raw);
    if raw < 1 << (category - 1) {
        raw - (1 << category) + 1
    } else {
        raw
    }
}

/// One entropy-coded block: the DC difference and the AC coefficients, in zig-zag order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodedBlock {
    pub dc_difference: i32,
    pub zigzag: [i32; BLOCK_PIXELS],
}

/// Reads one block. `None` when the bits run out or a code is invalid.
pub fn decode_block(reader: &mut BitReader<'_>, tables: Tables<'_>) -> Option<CodedBlock> {
    let category = tables.dc.decode(reader)?;
    if category > MAX_DC_CATEGORY {
        return None;
    }
    let dc_difference = extend(reader.bits(category)?, category);
    let mut zigzag = [0i32; BLOCK_PIXELS];
    let mut index = 1;
    while index < BLOCK_PIXELS {
        let symbol = tables.ac.decode(reader)?;
        if symbol == END_OF_BLOCK {
            break;
        }
        if symbol == ZERO_RUN {
            index += 16;
            continue;
        }
        let (run, size) = (usize::from(symbol >> 4), symbol & 0x0F);
        index += run;
        let slot = zigzag.get_mut(index)?;
        *slot = extend(reader.bits(size)?, size);
        index += 1;
    }
    Some(CodedBlock {
        dc_difference,
        zigzag,
    })
}

/// Writes one block: the DC difference, then the AC coefficients `zigzag[1..]`.
pub fn encode_block(
    writer: &mut BitWriter,
    dc_difference: i32,
    zigzag: &[i32; BLOCK_PIXELS],
    tables: Tables<'_>,
) {
    let difference = dc_difference.clamp(-MAX_DC, MAX_DC);
    let category = magnitude_category(difference);
    writer.code(tables.dc.code(category));
    writer.bits(magnitude_bits(difference, category), category);
    let mut run = 0u8;
    for &value in &zigzag[1..] {
        if value == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            writer.code(tables.ac.code(ZERO_RUN));
            run -= 16;
        }
        let value = value.clamp(-MAX_AC, MAX_AC);
        let size = magnitude_category(value).min(MAX_AC_CATEGORY);
        writer.code(tables.ac.code(run << 4 | size));
        writer.bits(magnitude_bits(value, size), size);
        run = 0;
    }
    if run > 0 {
        writer.code(tables.ac.code(END_OF_BLOCK));
    }
}

static COSINES: LazyLock<[[f32; BLOCK_SIDE]; BLOCK_SIDE]> = LazyLock::new(|| {
    std::array::from_fn(|x| {
        std::array::from_fn(|u| {
            let scale = if u == 0 {
                std::f32::consts::FRAC_1_SQRT_2
            } else {
                1.0
            };
            scale * ((2 * x + 1) as f32 * u as f32 * std::f32::consts::PI / 16.0).cos() / 2.0
        })
    })
});

/// Natural-order coefficients to level-shifted samples.
pub fn inverse_dct(coefficients: &[f32; BLOCK_PIXELS], pixels: &mut Block) {
    let cos = &*COSINES;
    let mut rows = [0f32; BLOCK_PIXELS];
    for y in 0..BLOCK_SIDE {
        for u in 0..BLOCK_SIDE {
            rows[y * BLOCK_SIDE + u] = (0..BLOCK_SIDE)
                .map(|v| cos[y][v] * coefficients[v * BLOCK_SIDE + u])
                .sum();
        }
    }
    for y in 0..BLOCK_SIDE {
        for x in 0..BLOCK_SIDE {
            let value: f32 = (0..BLOCK_SIDE)
                .map(|u| cos[x][u] * rows[y * BLOCK_SIDE + u])
                .sum();
            pixels[y * BLOCK_SIDE + x] = (value + 128.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Samples to natural-order coefficients, after the level shift.
#[must_use]
pub fn forward_dct(pixels: &Block) -> [f32; BLOCK_PIXELS] {
    let cos = &*COSINES;
    let mut rows = [0f32; BLOCK_PIXELS];
    for y in 0..BLOCK_SIDE {
        for u in 0..BLOCK_SIDE {
            rows[y * BLOCK_SIDE + u] = (0..BLOCK_SIDE)
                .map(|x| cos[x][u] * (f32::from(pixels[y * BLOCK_SIDE + x]) - 128.0))
                .sum();
        }
    }
    std::array::from_fn(|index| {
        let (v, u) = (index / BLOCK_SIDE, index % BLOCK_SIDE);
        (0..BLOCK_SIDE)
            .map(|y| cos[y][v] * rows[y * BLOCK_SIDE + u])
            .sum()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zigzag_is_a_permutation() {
        let mut seen = [false; BLOCK_PIXELS];
        for &index in &ZIGZAG {
            seen[index] = true;
        }
        assert!(seen.iter().all(|&hit| hit));
    }

    #[test]
    fn the_ac_tables_cover_every_run_and_size_once() {
        for spec in [&LUMA_AC, &CHROMA_AC] {
            let mut seen = std::collections::HashSet::new();
            for &symbol in spec.symbols {
                assert!(seen.insert(symbol));
                let size = symbol & 0x0F;
                assert!(size <= 10 && (size > 0 || symbol == 0 || symbol == 0xF0));
            }
            assert_eq!(seen.len(), 162);
            assert_eq!(
                spec.counts.iter().map(|&c| usize::from(c)).sum::<usize>(),
                162
            );
        }
    }

    #[test]
    fn the_dct_pair_round_trips() {
        let block: Block = std::array::from_fn(|index| (index * 3 % 256) as u8);
        let mut back = [0u8; BLOCK_PIXELS];
        inverse_dct(&forward_dct(&block), &mut back);
        for (a, b) in block.iter().zip(&back) {
            assert!(a.abs_diff(*b) <= 1);
        }
    }

    #[test]
    fn categories_and_magnitudes_invert() {
        for value in -1000..=1000 {
            let category = magnitude_category(value);
            assert_eq!(extend(magnitude_bits(value, category), category), value);
        }
    }

    #[test]
    fn blocks_round_trip_through_both_table_sets() {
        let mut zigzag = [0i32; BLOCK_PIXELS];
        zigzag[1] = -5;
        zigzag[2] = 300;
        zigzag[40] = 1;
        zigzag[63] = -2;
        for tables in [Tables::luma(), Tables::chroma()] {
            let mut writer = BitWriter::default();
            encode_block(&mut writer, -17, &zigzag, tables);
            encode_block(&mut writer, 4, &[0; BLOCK_PIXELS], tables);
            let bits = writer.len();
            let bytes = writer.finish();
            let mut reader = BitReader::new(&bytes);
            let first = decode_block(&mut reader, tables).expect("first block");
            assert_eq!(first.dc_difference, -17);
            assert_eq!(&first.zigzag[1..], &zigzag[1..]);
            let second = decode_block(&mut reader, tables).expect("second block");
            assert_eq!(second.dc_difference, 4);
            assert_eq!(reader.position(), bits);
        }
    }

    #[test]
    fn padding_ones_never_complete_a_code() {
        let mut reader = BitReader::new(&[0xFF]);
        assert_eq!(decode_block(&mut reader, Tables::luma()), None);
    }
}
