use num_complex::Complex;
use sdrmm_dsp::ParityCode;

use super::{bits, c4fm, dibits, filler};

const BAUD: f64 = 2_400.0;
const DEVIATION_HZ: f64 = 1_050.0;
const RRC_ALPHA: f64 = 0.2;

const FS1: u64 = 0x57FF_5F75_D577;
const FS2: u64 = 0x5F_F77D;
const FS3: u64 = 0x7D_DFF5;

const CHANNEL_CODES: [u32; 64] = [
    0x57_5F77, 0x57_7577, 0x57_DD75, 0x57_F775, 0x55_577D, 0x55_7D7D, 0x55_D57F, 0x55_FF7F,
    0x5F_555F, 0x5F_7F5F, 0x5F_D75D, 0x5F_FD5D, 0x5D_5D55, 0x5D_7755, 0x5D_DF57, 0x5D_F557,
    0x77_5DD7, 0x77_77D7, 0x77_DFD5, 0x77_F5D5, 0x75_55DD, 0x75_7FDD, 0x75_D7DF, 0x75_FDDF,
    0x7F_57FF, 0x7F_7DFF, 0x7F_D5FD, 0x7F_FFFD, 0x7D_5FF5, 0x7D_75F5, 0x7D_DDF7, 0x7D_F7F7,
    0xD7_55F7, 0xD7_7FF7, 0xD7_D7F5, 0xD7_FDF5, 0xD5_5DFD, 0xD5_77FD, 0xD5_DFFF, 0xD5_F5FF,
    0xDF_5FDF, 0xDF_75DF, 0xDF_DDDD, 0xDF_F7DD, 0xDD_57D5, 0xDD_7DD5, 0xDD_D5D7, 0xDD_FFD7,
    0xF7_5757, 0xF7_7D57, 0xF7_D555, 0xF7_FF55, 0xF5_5F5D, 0xF5_755D, 0xF5_DD5F, 0xF5_F75F,
    0xFF_5D7F, 0xFF_777F, 0xFF_DF7D, 0xFF_F57D, 0xFD_5575, 0xFD_7F75, 0xFD_D777, 0xFD_FD77,
];

pub struct Call {
    pub channel_code: u8,
    pub called: u32,
    pub own: u32,
    pub mode: u8,
}

impl Default for Call {
    fn default() -> Self {
        Self {
            channel_code: 37,
            called: 0x00_FFFF,
            own: 0x12_3456,
            mode: 1,
        }
    }
}

#[must_use]
pub fn transmission(call: &Call, rate: f64) -> Vec<Complex<f32>> {
    transmission_with_voice(call, &[[false; 72]; 32], rate)
}

#[must_use]
pub fn transmission_with_voice(call: &Call, voice: &[[bool; 72]], rate: f64) -> Vec<Complex<f32>> {
    let mut symbols = dibits(&filler(400, 67));
    symbols.extend(dibits(&bits(FS1, 48)));
    symbols.extend(dibits(&header_info(call)));
    symbols.extend(dibits(&channel_code(call.channel_code)));
    symbols.extend(dibits(&header_info(call)));

    for frames in voice.chunks(16) {
        symbols.extend(dibits(&bits(FS2, 24)));
        symbols.extend(superframe(frames, call.channel_code));
    }
    symbols.extend(dibits(&bits(FS3, 24)));
    symbols.extend(dibits(&filler(400, 73)));
    c4fm(&symbols, rate, BAUD, DEVIATION_HZ, RRC_ALPHA)
}

fn superframe(frames: &[[bool; 72]], code: u8) -> Vec<u8> {
    let mut padded = [[false; 72]; 16];
    padded[..frames.len()].copy_from_slice(frames);
    let mut out = Vec::with_capacity(756);
    for section in 0..4 {
        out.extend(dibits(&filler(72, 71 + section as u32)));
        for frame in &padded[section * 4..section * 4 + 4] {
            out.extend(dibits(frame));
        }
        if section == 1 {
            out.extend(dibits(&bits(FS2, 24)));
        } else if section != 3 {
            out.extend(dibits(&channel_code(code)));
        }
    }
    debug_assert_eq!(out.len(), 756);
    out
}

fn channel_code(number: u8) -> Vec<bool> {
    bits(u64::from(CHANNEL_CODES[usize::from(number & 0x3F)]), 24)
}

fn header_info(call: &Call) -> Vec<bool> {
    let mut info = bits(0, 4);
    info.extend(bits(u64::from(call.called), 24));
    info.extend(bits(u64::from(call.own), 24));
    info.extend(bits(u64::from(call.mode) & 0x07, 3));
    info.extend(bits(0, 4 + 2 + 11));

    let mut bytes: Vec<u8> = info
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| chunk.iter().fold(0u8, |acc, &b| acc << 1 | u8::from(b)))
        .collect();
    bytes.push(crc8(&bytes));

    let mut blocks = Vec::with_capacity(120);
    for &byte in &bytes {
        let mut word = [false; 12];
        word[..8].copy_from_slice(&bits(u64::from(byte), 8));
        ParityCode::HAMMING_12_8.encode(&mut word);
        blocks.extend(word);
    }
    let mut interleaved = vec![false; 120];
    for r in 0..12 {
        for c in 0..10 {
            interleaved[r * 10 + c] = blocks[c * 12 + r];
        }
    }
    let mut sequence = [true; 120];
    for i in 9..sequence.len() {
        sequence[i] = sequence[i - 9] ^ sequence[i - 5];
    }
    interleaved
        .into_iter()
        .zip(sequence)
        .map(|(bit, key)| bit ^ key)
        .collect()
}

fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                crc << 1 ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}
