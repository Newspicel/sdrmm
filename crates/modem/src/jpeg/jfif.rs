//! Whole baseline JPEG files of one grey component (ITU T.81 sequential Huffman, 8-bit), as
//! LRIT image segments carry them.

use super::{
    BLOCK_PIXELS, BLOCK_SIDE, BitReader, BitWriter, Block, Huffman, HuffmanSpec, LUMA_AC, LUMA_DC,
    Tables, ZIGZAG, decode_block, encode_block, forward_dct, inverse_dct,
};

const SOI: u8 = 0xD8;
const EOI: u8 = 0xD9;
const SOS: u8 = 0xDA;
const DQT: u8 = 0xDB;
const DHT: u8 = 0xC4;
const DRI: u8 = 0xDD;
const SOF_BASELINE: u8 = 0xC0;
const SOF_EXTENDED: u8 = 0xC1;
const RST_FIRST: u8 = 0xD0;
const RST_LAST: u8 = 0xD7;
const TABLES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JpegError {
    NotJpeg,
    Unsupported(&'static str),
    Truncated,
    Corrupt,
}

impl std::fmt::Display for JpegError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotJpeg => f.write_str("not a JPEG file"),
            Self::Unsupported(what) => write!(f, "unsupported JPEG: {what}"),
            Self::Truncated => f.write_str("JPEG file ends early"),
            Self::Corrupt => f.write_str("JPEG data is corrupt"),
        }
    }
}

impl std::error::Error for JpegError {}

#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    width: usize,
    height: usize,
    quant: usize,
}

/// Decodes grey baseline JPEG files, keeping its tables and scratch between them.
pub struct JpegGrey {
    quant: [[u16; BLOCK_PIXELS]; TABLES],
    dc: [Huffman; TABLES],
    ac: [Huffman; TABLES],
    restart: usize,
    frame: Option<Frame>,
    scan: Vec<u8>,
}

impl Default for JpegGrey {
    fn default() -> Self {
        Self::new()
    }
}

fn empty_table() -> Huffman {
    Huffman::build(&[0; 16], &[])
}

fn word(bytes: &[u8], at: usize) -> Result<usize, JpegError> {
    let pair = bytes.get(at..at + 2).ok_or(JpegError::Truncated)?;
    Ok(usize::from(u16::from_be_bytes([pair[0], pair[1]])))
}

impl JpegGrey {
    #[must_use]
    pub fn new() -> Self {
        Self {
            quant: [[1; BLOCK_PIXELS]; TABLES],
            dc: std::array::from_fn(|_| empty_table()),
            ac: std::array::from_fn(|_| empty_table()),
            restart: 0,
            frame: None,
            scan: Vec::new(),
        }
    }

    /// Decodes `data` into `out`, row by row; returns the width and height.
    pub fn decode(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<(usize, usize), JpegError> {
        if data.get(..2) != Some(&[0xFF, SOI]) {
            return Err(JpegError::NotJpeg);
        }
        self.restart = 0;
        self.frame = None;
        let mut at = 2;
        loop {
            while data.get(at) == Some(&0xFF) && data.get(at + 1) == Some(&0xFF) {
                at += 1;
            }
            if data.get(at) != Some(&0xFF) {
                return Err(JpegError::Corrupt);
            }
            let marker = *data.get(at + 1).ok_or(JpegError::Truncated)?;
            if marker == EOI {
                return Err(JpegError::Truncated);
            }
            let len = word(data, at + 2)?;
            let body = data.get(at + 4..at + 2 + len).ok_or(JpegError::Truncated)?;
            match marker {
                DQT => self.read_quant(body)?,
                DHT => self.read_huffman(body)?,
                DRI => self.restart = word(body, 0)?,
                SOF_BASELINE | SOF_EXTENDED => self.read_frame(body)?,
                0xC2 | 0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                    return Err(JpegError::Unsupported("not sequential Huffman"));
                }
                SOS => {
                    let tables = Self::read_scan(body)?;
                    return self.decode_scan(data, at + 2 + len, tables, out);
                }
                _ => {}
            }
            at += 2 + len;
        }
    }

    fn read_quant(&mut self, mut body: &[u8]) -> Result<(), JpegError> {
        while let Some((&head, rest)) = body.split_first() {
            let wide = head >> 4 != 0;
            let table = &mut self.quant[usize::from(head & 3)];
            let size = if wide { 2 * BLOCK_PIXELS } else { BLOCK_PIXELS };
            let values = rest.get(..size).ok_or(JpegError::Truncated)?;
            for (k, &natural) in ZIGZAG.iter().enumerate() {
                table[natural] = if wide {
                    u16::from_be_bytes([values[2 * k], values[2 * k + 1]])
                } else {
                    u16::from(values[k])
                };
            }
            body = &rest[size..];
        }
        Ok(())
    }

    fn read_huffman(&mut self, mut body: &[u8]) -> Result<(), JpegError> {
        while let Some((&head, rest)) = body.split_first() {
            let counts: [u8; 16] = rest
                .get(..16)
                .and_then(|counts| counts.try_into().ok())
                .ok_or(JpegError::Truncated)?;
            let total: usize = counts.iter().map(|&count| usize::from(count)).sum();
            let symbols = rest.get(16..16 + total).ok_or(JpegError::Truncated)?;
            let table = Huffman::from_segment(&counts, symbols).ok_or(JpegError::Corrupt)?;
            let slot = usize::from(head & 3);
            if head >> 4 == 0 {
                self.dc[slot] = table;
            } else {
                self.ac[slot] = table;
            }
            body = &rest[16 + total..];
        }
        Ok(())
    }

    fn read_frame(&mut self, body: &[u8]) -> Result<(), JpegError> {
        let head = body.get(..6).ok_or(JpegError::Truncated)?;
        if head[0] != 8 {
            return Err(JpegError::Unsupported("not eight-bit"));
        }
        if head[5] != 1 {
            return Err(JpegError::Unsupported("not one grey component"));
        }
        let component = body.get(6..9).ok_or(JpegError::Truncated)?;
        self.frame = Some(Frame {
            height: word(body, 1)?,
            width: word(body, 3)?,
            quant: usize::from(component[2] & 3),
        });
        Ok(())
    }

    fn read_scan(body: &[u8]) -> Result<(usize, usize), JpegError> {
        if body.first() != Some(&1) {
            return Err(JpegError::Unsupported("not one component a scan"));
        }
        let selector = *body.get(2).ok_or(JpegError::Truncated)?;
        Ok((usize::from(selector >> 4 & 3), usize::from(selector & 3)))
    }

    /// Copies entropy-coded bytes from `at` up to the next marker, undoing byte stuffing;
    /// returns where that marker starts.
    fn unstuff(&mut self, data: &[u8], mut at: usize) -> usize {
        self.scan.clear();
        while let Some(&byte) = data.get(at) {
            if byte != 0xFF {
                self.scan.push(byte);
                at += 1;
                continue;
            }
            match data.get(at + 1) {
                Some(0x00) => {
                    self.scan.push(0xFF);
                    at += 2;
                }
                Some(0xFF) => at += 1,
                _ => return at,
            }
        }
        at
    }

    fn decode_scan(
        &mut self,
        data: &[u8],
        mut at: usize,
        (dc, ac): (usize, usize),
        out: &mut Vec<u8>,
    ) -> Result<(usize, usize), JpegError> {
        let frame = self.frame.ok_or(JpegError::Corrupt)?;
        let across = frame.width.div_ceil(BLOCK_SIDE);
        let blocks = across * frame.height.div_ceil(BLOCK_SIDE);
        out.clear();
        out.resize(frame.width * frame.height, 0);
        let mut done = 0;
        while done < blocks {
            at = self.unstuff(data, at);
            let count = match self.restart {
                0 => blocks - done,
                interval => interval.min(blocks - done),
            };
            let tables = Tables {
                dc: &self.dc[dc],
                ac: &self.ac[ac],
            };
            let mut reader = BitReader::new(&self.scan);
            let mut predictor = 0i32;
            for index in done..done + count {
                let coded = decode_block(&mut reader, tables).ok_or(JpegError::Corrupt)?;
                predictor += coded.dc_difference;
                let mut coefficients = [0f32; BLOCK_PIXELS];
                for (k, &natural) in ZIGZAG.iter().enumerate() {
                    let value = if k == 0 { predictor } else { coded.zigzag[k] };
                    coefficients[natural] =
                        value as f32 * f32::from(self.quant[frame.quant][natural]);
                }
                let mut block: Block = [0; BLOCK_PIXELS];
                inverse_dct(&coefficients, &mut block);
                place(&block, index % across, index / across, frame, out);
            }
            done += count;
            let marker = data.get(at + 1).copied();
            match marker {
                Some(RST_FIRST..=RST_LAST) => at += 2,
                _ if done < blocks => return Err(JpegError::Truncated),
                _ => {}
            }
        }
        Ok((frame.width, frame.height))
    }
}

fn place(block: &Block, column: usize, row: usize, frame: Frame, out: &mut [u8]) {
    for y in 0..BLOCK_SIDE {
        let line = row * BLOCK_SIDE + y;
        if line >= frame.height {
            break;
        }
        let left = column * BLOCK_SIDE;
        let width = BLOCK_SIDE.min(frame.width - left);
        let start = line * frame.width + left;
        out[start..start + width].copy_from_slice(&block[y * BLOCK_SIDE..y * BLOCK_SIDE + width]);
    }
}

const ENCODE_QUANT: [u8; BLOCK_PIXELS] = [6; BLOCK_PIXELS];

fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(body);
}

fn huffman_body(class: u8, spec: &HuffmanSpec) -> Vec<u8> {
    let mut body = vec![class << 4];
    body.extend_from_slice(&spec.counts);
    body.extend_from_slice(spec.symbols);
    body
}

fn stuffed(bytes: &[u8], out: &mut Vec<u8>) {
    for &byte in bytes {
        out.push(byte);
        if byte == 0xFF {
            out.push(0);
        }
    }
}

/// A grey baseline JPEG of `pixels` with the Annex K luminance tables and a flat quantizer,
/// with a restart marker every `restart` blocks when it is not zero. The sender's side of
/// [`JpegGrey`].
#[must_use]
pub fn encode_grey(pixels: &[u8], width: usize, height: usize, restart: usize) -> Vec<u8> {
    let mut out = vec![0xFF, SOI];
    segment(&mut out, 0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    let mut dqt = vec![0];
    dqt.extend_from_slice(&ENCODE_QUANT);
    segment(&mut out, DQT, &dqt);
    let mut sof = vec![8];
    sof.extend_from_slice(&(height as u16).to_be_bytes());
    sof.extend_from_slice(&(width as u16).to_be_bytes());
    sof.extend_from_slice(&[1, 1, 0x11, 0]);
    segment(&mut out, SOF_BASELINE, &sof);
    segment(&mut out, DHT, &huffman_body(0, &LUMA_DC));
    segment(&mut out, DHT, &huffman_body(1, &LUMA_AC));
    if restart > 0 {
        segment(&mut out, DRI, &(restart as u16).to_be_bytes());
    }
    segment(&mut out, SOS, &[1, 1, 0x00, 0, 63, 0]);
    let across = width.div_ceil(8);
    let blocks = across * height.div_ceil(8);
    let mut writer = BitWriter::default();
    let mut previous = 0;
    for index in 0..blocks {
        if restart > 0 && index > 0 && index % restart == 0 {
            stuffed(&std::mem::take(&mut writer).finish(), &mut out);
            out.extend_from_slice(&[0xFF, RST_FIRST + ((index / restart - 1) % 8) as u8]);
            previous = 0;
        }
        let block: Block = std::array::from_fn(|pixel| {
            let x = ((index % across) * 8 + pixel % 8).min(width - 1);
            let y = ((index / across) * 8 + pixel / 8).min(height - 1);
            pixels[y * width + x]
        });
        let natural = forward_dct(&block);
        let zigzag: [i32; BLOCK_PIXELS] = std::array::from_fn(|k| {
            (natural[ZIGZAG[k]] / f32::from(ENCODE_QUANT[ZIGZAG[k]])).round() as i32
        });
        encode_block(&mut writer, zigzag[0] - previous, &zigzag, Tables::luma());
        previous = zigzag[0];
    }
    stuffed(&writer.finish(), &mut out);
    out.extend_from_slice(&[0xFF, EOI]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: usize, height: usize) -> Vec<u8> {
        (0..width * height)
            .map(|index| {
                let (x, y) = (index % width, index / width);
                (128.0 + 90.0 * ((x as f32) * 0.05).sin() * ((y as f32) * 0.07).cos()) as u8
            })
            .collect()
    }

    #[test]
    fn grey_files_decode_with_and_without_restarts() {
        let (width, height) = (61, 37);
        let sent = picture(width, height);
        let mut decoder = JpegGrey::new();
        for restart in [0, 3] {
            let file = encode_grey(&sent, width, height, restart);
            let mut got = Vec::new();
            assert_eq!(decoder.decode(&file, &mut got), Ok((width, height)));
            let error = sent
                .iter()
                .zip(&got)
                .map(|(&a, &b)| f64::from(a.abs_diff(b)))
                .sum::<f64>()
                / sent.len() as f64;
            assert!(error < 3.0, "restart {restart}: mean error {error}");
        }
    }

    #[test]
    fn broken_files_are_errors() {
        let file = encode_grey(&picture(16, 16), 16, 16, 0);
        let mut decoder = JpegGrey::new();
        let mut out = Vec::new();
        assert_eq!(decoder.decode(b"GIF89a", &mut out), Err(JpegError::NotJpeg));
        assert!(decoder.decode(&file[..file.len() / 2], &mut out).is_err());
        let mut colour = file.clone();
        let sof = colour
            .windows(2)
            .position(|pair| pair == [0xFF, SOF_BASELINE])
            .expect("frame");
        colour[sof + 9] = 3;
        assert_eq!(
            decoder.decode(&colour, &mut out),
            Err(JpegError::Unsupported("not one grey component"))
        );
    }
}
