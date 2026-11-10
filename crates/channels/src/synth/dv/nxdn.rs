use blip25_vocoder::halfrate::pack_natural;
use num_complex::Complex;
use sdrmm_dsp::{crc_msb_bits, fec::conv};

use super::{bits, c4fm, dibits, filler, half_rate_air};
use crate::dv::nxdn::privacy::{CipherKey, FRAME_BITS, FRAMES_PER_SUPERFRAME, STREAM_BYTES};
use crate::dv::vocoder::{AMBE_3600_INTERLEAVE, half_frame, half_rate_code_vectors};

const VOICE_CALL: u8 = 0x01;
const VOICE_CALL_IV: u8 = 0x03;
const TX_RELEASE: u8 = 0x08;
const SCRAMBLER: u8 = 1;
const VOICE_PER_FRAME: usize = 4;
const SUPERFRAMES_PER_IV: usize = 2;

const RRC_ALPHA: f64 = 0.2;
const FSW: u64 = 0x000C_DF59;
const SACCH_PUNCTURES: [usize; 12] = [5, 11, 17, 23, 29, 35, 41, 47, 53, 59, 65, 71];
const FACCH_PUNCTURES: [usize; 48] = [
    1, 5, 9, 13, 17, 21, 25, 29, 33, 37, 41, 45, 49, 53, 57, 61, 65, 69, 73, 77, 81, 85, 89, 93,
    97, 101, 105, 109, 113, 117, 121, 125, 129, 133, 137, 141, 145, 149, 153, 157, 161, 165, 169,
    173, 177, 181, 185, 189,
];

pub struct Shape {
    pub baud: f64,
    pub deviation_hz: f64,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            baud: 2_400.0,
            deviation_hz: 1_050.0,
        }
    }
}

#[must_use]
pub fn transmission(shape: &Shape, rf_channel: u8, outbound: bool, rate: f64) -> Vec<Complex<f32>> {
    transmission_inner(shape, rf_channel, outbound, None, rate)
}

#[must_use]
pub fn addressed_transmission(
    shape: &Shape,
    ran: u8,
    source: u16,
    destination: u16,
    group: bool,
    rate: f64,
) -> Vec<Complex<f32>> {
    let call = layer3(0x01, source, destination, group);
    let release = layer3(0x08, source, destination, group);
    let mut symbols = dibits(&filler(400, 53));
    symbols.extend(addressed_frame(0, ran, &call, None, true));
    for quarter in 0..4 {
        symbols.extend(addressed_frame(2, ran, &call, Some(quarter), false));
    }
    symbols.extend(addressed_frame(0, ran, &release, None, true));
    symbols.extend(frame(1, 1, true, None));
    symbols.extend(dibits(&filler(200, 59)));
    c4fm(&symbols, rate, shape.baud, shape.deviation_hz, RRC_ALPHA)
}

#[must_use]
pub fn aliased_transmission(shape: &Shape, ran: u8, alias: &str, rate: f64) -> Vec<Complex<f32>> {
    let mut text = [0u8; 16];
    for (slot, byte) in text.iter_mut().zip(alias.bytes()) {
        *slot = byte;
    }
    let mut symbols = dibits(&filler(400, 53));
    for (index, chunk) in text.chunks(4).enumerate() {
        let mut message = [false; 80];
        put_bits(&mut message, 0, 32, 0x3F68_8204);
        put_bits(&mut message, 32, 8, (index as u32 + 1) << 4 | 4);
        for (i, &byte) in chunk.iter().enumerate() {
            put_bits(&mut message, 40 + i * 8, 8, u32::from(byte));
        }
        for quarter in 0..4 {
            symbols.extend(addressed_frame(2, ran, &message, Some(quarter), false));
        }
    }
    symbols.extend(frame(1, 1, true, None));
    symbols.extend(dibits(&filler(200, 59)));
    c4fm(&symbols, rate, shape.baud, shape.deviation_hz, RRC_ALPHA)
}

fn addressed_frame(
    functional: u8,
    ran: u8,
    layer3: &[bool; 80],
    quarter: Option<usize>,
    facch: bool,
) -> Vec<u8> {
    let mut lich = bits(1, 2);
    lich.extend(bits(u64::from(functional), 2));
    lich.extend(bits(if facch { 0 } else { 3 }, 2));
    lich.push(true);
    lich.push(lich_parity(&lich));

    let out = dibits(&bits(FSW, 20));
    let mut post: Vec<u8> = lich.into_iter().map(|bit| u8::from(bit) << 1).collect();
    let (structure, data) = match quarter {
        Some(quarter) => (3 - quarter as u8, &layer3[quarter * 18..quarter * 18 + 18]),
        None => (0, &layer3[..18]),
    };
    post.extend(dibits(&encode_sacch(structure, ran, data)));
    if facch {
        let coded = dibits(&encode_facch(layer3));
        post.extend_from_slice(&coded);
        post.extend_from_slice(&coded);
    } else {
        post.extend(dibits(&filler(288, 79 + u32::from(functional))));
    }
    finish_frame(out, post)
}

/// A group call enciphered with `key` (scrambler, DES-OFB or AES by its length) under `key_id`.
/// DES and AES send a VCALL_IV ahead of every second superframe, the first one `iv`. `voice`
/// holds the clear frames' transmitted bits, 16 per superframe.
#[must_use]
pub fn keyed_transmission(
    shape: &Shape,
    ran: u8,
    key_id: u8,
    key: &str,
    iv: u64,
    voice: &[[bool; 72]],
    rate: f64,
) -> Vec<Complex<f32>> {
    let cipher = CipherKey::parse(key).unwrap_or_else(|| panic!("{key:?} is no NXDN key"));
    let mut call = layer3(VOICE_CALL, 1, 100, true);
    put_bits(&mut call, 56, 2, u32::from(cipher.cipher()));
    put_bits(&mut call, 58, 6, u32::from(key_id));
    let mut symbols = dibits(&filler(400, 53));
    symbols.extend(addressed_frame(0, ran, &call, None, true));
    let mut stream = [0u8; STREAM_BYTES];
    for (index, superframe) in voice
        .as_chunks::<FRAMES_PER_SUPERFRAME>()
        .0
        .iter()
        .enumerate()
    {
        let iv = iv.wrapping_add(index as u64);
        let half = index % SUPERFRAMES_PER_IV;
        if cipher.cipher() == SCRAMBLER || half == 0 {
            cipher.stream(iv, &mut stream);
        }
        if cipher.cipher() != SCRAMBLER && half == 0 {
            let mut message = [false; 80];
            put_bits(&mut message, 2, 6, u32::from(VOICE_CALL_IV));
            put_bits(&mut message, 8, 32, (iv >> 32) as u32);
            put_bits(&mut message, 40, 32, iv as u32);
            symbols.extend(addressed_frame(0, ran, &message, None, true));
        }
        let offset = if cipher.cipher() == SCRAMBLER {
            0
        } else {
            half
        };
        for quarter in 0..4 {
            let frames: [[bool; 72]; 4] = std::array::from_fn(|slot| {
                let position = offset * FRAMES_PER_SUPERFRAME + quarter * VOICE_PER_FRAME + slot;
                encipher(
                    &superframe[quarter * VOICE_PER_FRAME + slot],
                    &stream,
                    position,
                )
            });
            symbols.extend(voice_frame(ran, &call, quarter, &frames));
        }
    }
    let release = layer3(TX_RELEASE, 1, 100, true);
    symbols.extend(addressed_frame(0, ran, &release, None, true));
    symbols.extend(frame(1, 1, true, None));
    symbols.extend(dibits(&filler(200, 59)));
    c4fm(&symbols, rate, shape.baud, shape.deviation_hz, RRC_ALPHA)
}

fn encipher(air: &[bool; 72], stream: &[u8; STREAM_BYTES], position: usize) -> [bool; 72] {
    let frame = half_frame(half_rate_code_vectors(air, &AMBE_3600_INTERLEAVE));
    let mut natural = pack_natural(&frame.info);
    for bit in 0..FRAME_BITS {
        let at = position * FRAME_BITS + bit;
        natural[bit / 8] ^= (stream[at / 8] >> (7 - at % 8) & 1) << (7 - bit % 8);
    }
    half_rate_air(&natural)
}

/// A traffic frame of four voice frames and the SACCH quarter of `layer3`.
fn voice_frame(ran: u8, layer3: &[bool; 80], quarter: usize, voice: &[[bool; 72]; 4]) -> Vec<u8> {
    let mut lich = bits(1, 2);
    lich.extend(bits(2, 2));
    lich.extend(bits(3, 2));
    lich.push(true);
    lich.push(lich_parity(&lich));
    let out = dibits(&bits(FSW, 20));
    let mut post: Vec<u8> = lich.into_iter().map(|bit| u8::from(bit) << 1).collect();
    let structure = 3 - quarter as u8;
    post.extend(dibits(&encode_sacch(
        structure,
        ran,
        &layer3[quarter * 18..quarter * 18 + 18],
    )));
    for frame in voice {
        post.extend(dibits(frame));
    }
    finish_frame(out, post)
}

fn lich_parity(lich: &[bool]) -> bool {
    lich[..4].iter().fold(false, |parity, &bit| parity ^ bit)
}

fn layer3(message_type: u8, source: u16, destination: u16, group: bool) -> [bool; 80] {
    let mut out = [false; 80];
    put_bits(&mut out, 2, 6, u32::from(message_type));
    out[16] = !group;
    put_bits(&mut out, 24, 16, u32::from(source));
    put_bits(&mut out, 40, 16, u32::from(destination));
    out
}

fn put_bits(out: &mut [bool], start: usize, len: usize, value: u32) {
    for i in 0..len {
        out[start + i] = value >> (len - 1 - i) & 1 != 0;
    }
}

fn encode_sacch(structure: u8, ran: u8, data: &[bool]) -> Vec<bool> {
    let mut info = bits(u64::from(structure), 2);
    info.extend(bits(u64::from(ran & 0x3f), 6));
    info.extend_from_slice(data);
    append_crc(&mut info, 0x27, 0x3f, 6);
    info.extend([false; 4]);
    let mut coded = Vec::new();
    conv::encode(&info, &mut coded);
    let punctured: Vec<bool> = coded
        .into_iter()
        .enumerate()
        .filter_map(|(i, bit)| (!SACCH_PUNCTURES.contains(&i)).then_some(bit))
        .collect();
    let mut interleaved = vec![false; 60];
    for (i, bit) in punctured.into_iter().enumerate() {
        interleaved[(i % 12) * 5 + i / 12] = bit;
    }
    interleaved
}

fn encode_facch(data: &[bool; 80]) -> Vec<bool> {
    let mut info = data.to_vec();
    append_crc(&mut info, 0x080f, 0x0fff, 12);
    info.extend([false; 4]);
    let mut coded = Vec::new();
    conv::encode(&info, &mut coded);
    let punctured: Vec<bool> = coded
        .into_iter()
        .enumerate()
        .filter_map(|(i, bit)| (!FACCH_PUNCTURES.contains(&i)).then_some(bit))
        .collect();
    let mut interleaved = vec![false; 144];
    for (i, bit) in punctured.into_iter().enumerate() {
        interleaved[(i % 16) * 9 + i / 16] = bit;
    }
    interleaved
}

fn append_crc(info: &mut Vec<bool>, poly: u32, init: u32, width: u32) {
    let crc = crc_msb_bits(poly, width, init, info.iter().copied());
    info.extend((0..width).map(|i| crc >> (width - 1 - i) & 1 != 0));
}

#[must_use]
#[allow(dead_code)]
pub(crate) fn transmission_with_voice(
    shape: &Shape,
    rf_channel: u8,
    outbound: bool,
    voice: &[[[bool; 72]; 4]; 5],
    rate: f64,
) -> Vec<Complex<f32>> {
    transmission_inner(shape, rf_channel, outbound, Some(voice), rate)
}

fn transmission_inner(
    shape: &Shape,
    rf_channel: u8,
    outbound: bool,
    voice: Option<&[[[bool; 72]; 4]; 5]>,
    rate: f64,
) -> Vec<Complex<f32>> {
    let mut symbols = dibits(&filler(400, 53));
    for (index, functional) in [0u8, 2, 2, 2, 0].into_iter().enumerate() {
        symbols.extend(frame(
            rf_channel,
            functional,
            outbound,
            voice.map(|frames| &frames[index]),
        ));
    }
    symbols.extend(dibits(&filler(200, 59)));
    c4fm(&symbols, rate, shape.baud, shape.deviation_hz, RRC_ALPHA)
}

fn frame(
    rf_channel: u8,
    functional: u8,
    outbound: bool,
    voice: Option<&[[bool; 72]; 4]>,
) -> Vec<u8> {
    let mut lich = bits(u64::from(rf_channel) & 0x03, 2);
    lich.extend(bits(u64::from(functional) & 0x03, 2));
    lich.extend(bits(3, 2));
    lich.push(outbound);
    lich.push(lich_parity(&lich));
    let out = dibits(&bits(FSW, 20));
    let mut post: Vec<u8> = lich.into_iter().map(|bit| u8::from(bit) << 1).collect();
    post.extend(dibits(&filler(60, 61 + u32::from(functional))));
    if let Some(voice) = voice {
        for frame in voice {
            post.extend(dibits(frame));
        }
    } else {
        post.extend(dibits(&filler(288, 67 + u32::from(functional))));
    }
    finish_frame(out, post)
}

fn finish_frame(mut out: Vec<u8>, mut post: Vec<u8>) -> Vec<u8> {
    assert_eq!(post.len(), 182);
    let mut register = 0xE4u16;
    for dibit in &mut post {
        let pn = register & 1 != 0;
        let feedback = (register ^ (register >> 4)) & 1;
        register = register >> 1 | feedback << 8;
        if pn {
            *dibit ^= 0b10;
        }
    }
    out.extend(post);
    out
}
