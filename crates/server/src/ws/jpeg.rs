use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock, Mutex, OnceLock, Weak},
};

use axum::body::Bytes;
use sdrmm_channels::VideoPicture;

const QUALITY: u32 = 85;
const REMEMBERED: usize = 8;
const BLOCK: usize = 8;
const CELLS: usize = BLOCK * BLOCK;
const COLOR_MCU: usize = 16;
const AC_LIMIT: i32 = 1023;
const ZERO_RUN: u8 = 0xf0;
const END_OF_BLOCK: u8 = 0x00;

const ZIGZAG: [usize; CELLS] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

const LUMA_QUANT: [u8; CELLS] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

const CHROMA_QUANT: [u8; CELLS] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];

const AC_LUMA_VALUES: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

const AC_CHROMA_VALUES: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

/// The factors the fast transform leaves in each row and column, which quantizing divides out.
const TRANSFORM_SCALE: [f32; BLOCK] = [
    1.0,
    1.387_039_8,
    1.306_563,
    1.175_875_6,
    1.0,
    0.785_694_96,
    0.541_196_1,
    0.275_899_38,
];

struct Huffman {
    bits: &'static [u8; 16],
    values: &'static [u8],
    code: [u16; 256],
    size: [u8; 256],
}

impl Huffman {
    fn new(bits: &'static [u8; 16], values: &'static [u8]) -> Self {
        let mut table = Self {
            bits,
            values,
            code: [0; 256],
            size: [0; 256],
        };
        let mut code = 0u16;
        let mut values = values.iter();
        for (length, &count) in (1u8..).zip(bits) {
            for &value in values.by_ref().take(usize::from(count)) {
                table.code[usize::from(value)] = code;
                table.size[usize::from(value)] = length;
                code += 1;
            }
            code <<= 1;
        }
        table
    }
}

struct Quantizer {
    table: [u8; CELLS],
    divide: [f32; CELLS],
}

impl Quantizer {
    fn new(base: &[u8; CELLS]) -> Self {
        let percent = 200 - 2 * QUALITY;
        let table = base.map(|step| ((u32::from(step) * percent + 50) / 100).clamp(1, 255) as u8);
        let mut divide = [0.0; CELLS];
        for (cell, divide) in divide.iter_mut().enumerate() {
            let scale = TRANSFORM_SCALE[cell / BLOCK] * TRANSFORM_SCALE[cell % BLOCK] * 8.0;
            *divide = 1.0 / (f32::from(table[cell]) * scale);
        }
        Self { table, divide }
    }
}

struct Plane {
    quantizer: Quantizer,
    dc: Huffman,
    ac: Huffman,
}

struct Tables {
    luma: Plane,
    chroma: Plane,
}

static TABLES: LazyLock<Tables> = LazyLock::new(|| Tables {
    luma: Plane {
        quantizer: Quantizer::new(&LUMA_QUANT),
        dc: Huffman::new(&DC_LUMA_BITS, &DC_VALUES),
        ac: Huffman::new(&AC_LUMA_BITS, &AC_LUMA_VALUES),
    },
    chroma: Plane {
        quantizer: Quantizer::new(&CHROMA_QUANT),
        dc: Huffman::new(&DC_CHROMA_BITS, &DC_VALUES),
        ac: Huffman::new(&AC_CHROMA_BITS, &AC_CHROMA_VALUES),
    },
});

struct BitWriter {
    out: Vec<u8>,
    held: u32,
    count: u32,
}

impl BitWriter {
    fn put(&mut self, bits: u32, count: u32) {
        self.held = (self.held << count) | (bits & ((1 << count) - 1));
        self.count += count;
        while self.count >= 8 {
            self.count -= 8;
            let byte = (self.held >> self.count) as u8;
            self.out.push(byte);
            if byte == 0xff {
                self.out.push(0);
            }
        }
    }

    fn coded(&mut self, table: &Huffman, symbol: u8) {
        let symbol = usize::from(symbol);
        self.put(u32::from(table.code[symbol]), u32::from(table.size[symbol]));
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            let pad = 8 - self.count;
            self.put((1 << pad) - 1, pad);
        }
        self.out
    }
}

/// How many bits a coefficient takes and the bits themselves, a negative one as its one's
/// complement.
fn magnitude(value: i32) -> (u32, u32) {
    let size = 32 - value.unsigned_abs().leading_zeros();
    let bits = if value < 0 { value - 1 } else { value };
    (size, bits as u32)
}

fn transform_line(cells: &mut [f32; CELLS], start: usize, step: usize) {
    let at = |index: usize| start + index * step;
    let (sum07, diff07) = (cells[at(0)] + cells[at(7)], cells[at(0)] - cells[at(7)]);
    let (sum16, diff16) = (cells[at(1)] + cells[at(6)], cells[at(1)] - cells[at(6)]);
    let (sum25, diff25) = (cells[at(2)] + cells[at(5)], cells[at(2)] - cells[at(5)]);
    let (sum34, diff34) = (cells[at(3)] + cells[at(4)], cells[at(3)] - cells[at(4)]);

    let (outer, outer_diff) = (sum07 + sum34, sum07 - sum34);
    let (inner, inner_diff) = (sum16 + sum25, sum16 - sum25);
    cells[at(0)] = outer + inner;
    cells[at(4)] = outer - inner;
    let rotated = (inner_diff + outer_diff) * std::f32::consts::FRAC_1_SQRT_2;
    cells[at(2)] = outer_diff + rotated;
    cells[at(6)] = outer_diff - rotated;

    let (low, mid, high) = (diff34 + diff25, diff25 + diff16, diff16 + diff07);
    let shared = (low - high) * 0.382_683_43;
    let low = 0.541_196_1 * low + shared;
    let high = 1.306_563 * high + shared;
    let mid = mid * std::f32::consts::FRAC_1_SQRT_2;
    let (plus, minus) = (diff07 + mid, diff07 - mid);
    cells[at(5)] = minus + low;
    cells[at(3)] = minus - low;
    cells[at(1)] = plus + high;
    cells[at(7)] = plus - high;
}

fn transform(cells: &mut [f32; CELLS]) {
    for row in 0..BLOCK {
        transform_line(cells, row * BLOCK, 1);
    }
    for column in 0..BLOCK {
        transform_line(cells, column, BLOCK);
    }
}

fn write_block(writer: &mut BitWriter, cells: &mut [f32; CELLS], plane: &Plane, dc: &mut i32) {
    transform(cells);
    let level =
        |zigzag: usize| (cells[ZIGZAG[zigzag]] * plane.quantizer.divide[ZIGZAG[zigzag]]).round();
    let now = level(0) as i32;
    let (size, bits) = magnitude(now - *dc);
    *dc = now;
    writer.coded(&plane.dc, size as u8);
    writer.put(bits, size);

    let mut zeros = 0u8;
    for zigzag in 1..CELLS {
        let value = (level(zigzag) as i32).clamp(-AC_LIMIT, AC_LIMIT);
        if value == 0 {
            zeros += 1;
            continue;
        }
        while zeros > 15 {
            writer.coded(&plane.ac, ZERO_RUN);
            zeros -= 16;
        }
        let (size, bits) = magnitude(value);
        writer.coded(&plane.ac, (zeros << 4) | size as u8);
        writer.put(bits, size);
        zeros = 0;
    }
    if zeros > 0 {
        writer.coded(&plane.ac, END_OF_BLOCK);
    }
}

fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) {
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(body);
}

fn quant_segment(out: &mut Vec<u8>, id: u8, quantizer: &Quantizer) {
    let mut body = vec![id];
    body.extend(ZIGZAG.iter().map(|&cell| quantizer.table[cell]));
    segment(out, 0xdb, &body);
}

fn huffman_segment(out: &mut Vec<u8>, class_and_id: u8, table: &Huffman) {
    let mut body = vec![class_and_id];
    body.extend_from_slice(table.bits);
    body.extend_from_slice(table.values);
    segment(out, 0xc4, &body);
}

fn headers(width: u16, height: u16, color: bool) -> Vec<u8> {
    let tables = &*TABLES;
    let mut out = vec![0xff, 0xd8];
    segment(&mut out, 0xe0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    quant_segment(&mut out, 0, &tables.luma.quantizer);
    if color {
        quant_segment(&mut out, 1, &tables.chroma.quantizer);
    }
    let mut frame = vec![8];
    frame.extend_from_slice(&height.to_be_bytes());
    frame.extend_from_slice(&width.to_be_bytes());
    if color {
        frame.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    } else {
        frame.extend_from_slice(&[1, 1, 0x11, 0]);
    }
    segment(&mut out, 0xc0, &frame);
    huffman_segment(&mut out, 0x00, &tables.luma.dc);
    huffman_segment(&mut out, 0x10, &tables.luma.ac);
    if color {
        huffman_segment(&mut out, 0x01, &tables.chroma.dc);
        huffman_segment(&mut out, 0x11, &tables.chroma.ac);
        segment(&mut out, 0xda, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);
    } else {
        segment(&mut out, 0xda, &[1, 1, 0x00, 0, 63, 0]);
    }
    out
}

fn write_gray(writer: &mut BitWriter, width: usize, height: usize, luma: &[u8]) {
    let mut dc = 0;
    let mut cells = [0.0; CELLS];
    for top in (0..height).step_by(BLOCK) {
        for left in (0..width).step_by(BLOCK) {
            for (cell, value) in cells.iter_mut().enumerate() {
                let row = (top + cell / BLOCK).min(height - 1);
                let column = (left + cell % BLOCK).min(width - 1);
                *value = f32::from(luma[row * width + column]) - 128.0;
            }
            write_block(writer, &mut cells, &TABLES.luma, &mut dc);
        }
    }
}

struct ColorMcu {
    luma: [f32; COLOR_MCU * COLOR_MCU],
    blue: [f32; CELLS],
    red: [f32; CELLS],
}

impl ColorMcu {
    fn load(&mut self, rgb: &[u8], width: usize, height: usize, left: usize, top: usize) {
        self.blue = [0.0; CELLS];
        self.red = [0.0; CELLS];
        for y in 0..COLOR_MCU {
            let row = (top + y).min(height - 1) * width;
            for x in 0..COLOR_MCU {
                let at = (row + (left + x).min(width - 1)) * 3;
                let (r, g, b) = (
                    f32::from(rgb[at]),
                    f32::from(rgb[at + 1]),
                    f32::from(rgb[at + 2]),
                );
                self.luma[y * COLOR_MCU + x] = 0.299 * r + 0.587 * g + 0.114 * b - 128.0;
                let chroma = (y / 2) * BLOCK + x / 2;
                self.blue[chroma] += (-0.168_736 * r - 0.331_264 * g + 0.5 * b) * 0.25;
                self.red[chroma] += (0.5 * r - 0.418_688 * g - 0.081_312 * b) * 0.25;
            }
        }
    }

    fn luma_block(&self, quarter: usize) -> [f32; CELLS] {
        let (left, top) = ((quarter % 2) * BLOCK, (quarter / 2) * BLOCK);
        std::array::from_fn(|cell| {
            self.luma[(top + cell / BLOCK) * COLOR_MCU + left + cell % BLOCK]
        })
    }
}

fn write_color(writer: &mut BitWriter, width: usize, height: usize, rgb: &[u8]) {
    let (mut luma_dc, mut blue_dc, mut red_dc) = (0, 0, 0);
    let mut mcu = ColorMcu {
        luma: [0.0; COLOR_MCU * COLOR_MCU],
        blue: [0.0; CELLS],
        red: [0.0; CELLS],
    };
    for top in (0..height).step_by(COLOR_MCU) {
        for left in (0..width).step_by(COLOR_MCU) {
            mcu.load(rgb, width, height, left, top);
            for quarter in 0..4 {
                let mut cells = mcu.luma_block(quarter);
                write_block(writer, &mut cells, &TABLES.luma, &mut luma_dc);
            }
            write_block(writer, &mut mcu.blue, &TABLES.chroma, &mut blue_dc);
            write_block(writer, &mut mcu.red, &TABLES.chroma, &mut red_dc);
        }
    }
}

/// A baseline JPEG of the picture, colour at half chroma resolution. `None` when the picture
/// holds no pixels or not the number its size asks for.
pub(crate) fn encode(picture: &VideoPicture) -> Option<Vec<u8>> {
    let (width, height) = (usize::from(picture.width), usize::from(picture.height));
    let pixels = width * height;
    let color = !picture.rgb.is_empty();
    let fits = if color {
        picture.rgb.len() == pixels * 3
    } else {
        picture.luma.len() == pixels
    };
    if pixels == 0 || !fits {
        return None;
    }
    let mut writer = BitWriter {
        out: headers(picture.width, picture.height, color),
        held: 0,
        count: 0,
    };
    if color {
        write_color(&mut writer, width, height, &picture.rgb);
    } else {
        write_gray(&mut writer, width, height, &picture.luma);
    }
    let mut out = writer.finish();
    out.extend_from_slice(&[0xff, 0xd9]);
    Some(out)
}

type Encoded = Arc<OnceLock<Option<Bytes>>>;

/// The pictures encoded lately, so every client watching one stream shares one encoding.
#[derive(Default)]
pub(crate) struct Memo {
    recent: Mutex<VecDeque<(Weak<VideoPicture>, Encoded)>>,
}

impl Memo {
    /// Encodes on the calling thread, or waits for whoever already is.
    pub(crate) fn encoded(&self, picture: &Arc<VideoPicture>) -> Option<Bytes> {
        self.slot(picture)
            .get_or_init(|| encode(picture).map(Bytes::from))
            .clone()
    }

    fn slot(&self, picture: &Arc<VideoPicture>) -> Encoded {
        let mut recent = self
            .recent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, encoded)) = recent
            .iter()
            .find(|(known, _)| known.as_ptr() == Arc::as_ptr(picture))
        {
            return encoded.clone();
        }
        if recent.len() >= REMEMBERED {
            recent.pop_front();
        }
        let encoded = Encoded::default();
        recent.push_back((Arc::downgrade(picture), encoded.clone()));
        encoded
    }
}

#[cfg(test)]
mod tests;
