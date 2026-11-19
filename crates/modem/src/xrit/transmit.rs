use sdrmm_dsp::coding::fec::crc16_msb;

use super::rice::{NN, RiceParams};
use num_complex::Complex;

use crate::ccsds::{Cadu, Precoding, SEQUENCE_MODULO, Segment, coded_bits_with, space_packet};
use crate::constellation::tables;
use crate::linear::{LinearMod, LinearParams};
use crate::pulse::{Norm, root_raised_cosine};

const ID_LEN: u32 = 3;
const RAW_ID: u32 = (1 << ID_LEN) - 1;
const MAX_K: u32 = RAW_ID - 2;

/// Splits one file into the space packets that carry it, each with its CRC.
///
/// The first packet holds the transport header and the file's first `header_len` bytes; the
/// rest go `chunk` bytes at a time, or one coded line a packet when `lines` is given.
#[must_use]
pub fn file_packets(
    apid: u16,
    first_sequence: u16,
    counter: u16,
    file: &[u8],
    header_len: usize,
    chunk: usize,
) -> Vec<Vec<u8>> {
    let mut pieces = vec![first_piece(counter, file, header_len)];
    pieces.extend(
        file[header_len.min(file.len())..]
            .chunks(chunk.max(1))
            .map(<[u8]>::to_vec),
    );
    packets(apid, first_sequence, &pieces)
}

/// Like [`file_packets`] for a Rice-coded image: after the headers, one packet a coded line.
#[must_use]
pub fn rice_image_packets(
    apid: u16,
    first_sequence: u16,
    counter: u16,
    headers: &[u8],
    lines: &[Vec<u8>],
) -> Vec<Vec<u8>> {
    let mut pieces = vec![first_piece(counter, headers, headers.len())];
    pieces.extend(lines.iter().cloned());
    packets(apid, first_sequence, &pieces)
}

fn first_piece(counter: u16, file: &[u8], header_len: usize) -> Vec<u8> {
    let mut piece = counter.to_be_bytes().to_vec();
    piece.extend_from_slice(&(file.len() as u64 * 8).to_be_bytes());
    piece.extend_from_slice(&file[..header_len.min(file.len())]);
    piece
}

fn packets(apid: u16, first_sequence: u16, pieces: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let last = pieces.len() - 1;
    pieces
        .iter()
        .enumerate()
        .map(|(index, piece)| {
            let segment = match (index == 0, index == last) {
                (true, true) => Segment::Unsegmented,
                (true, false) => Segment::First,
                (false, true) => Segment::Last,
                (false, false) => Segment::Continuation,
            };
            let sequence = (u32::from(first_sequence) + index as u32) % SEQUENCE_MODULO;
            let mut data = piece.clone();
            data.extend_from_slice(&crc16_msb(0x1021, 0xFFFF, piece).to_be_bytes());
            space_packet(apid, segment, sequence as u16, &data)
        })
        .collect()
}

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    used: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, count: u32) {
        for shift in (0..count).rev() {
            if self.used == 0 {
                self.bytes.push(0);
            }
            let bit = (value >> shift & 1) as u8;
            if let Some(last) = self.bytes.last_mut() {
                *last |= bit << (7 - self.used);
            }
            self.used = (self.used + 1) % 8;
        }
    }

    fn fundamental(&mut self, value: u32) {
        for _ in 0..value {
            self.put(0, 1);
        }
        self.put(1, 1);
    }
}

/// Rice-codes one line the way [`super::RiceLine`] reads it, choosing per block the cheapest of
/// split-sample, second extension, a single zero block and raw.
#[must_use]
pub fn rice_encode(params: RiceParams, line: &[u8]) -> Vec<u8> {
    let block = params.block();
    let mut samples: Vec<u32> = line.iter().map(|&value| u32::from(value)).collect();
    samples.resize(
        params.padded(),
        u32::from(line.last().copied().unwrap_or(0)),
    );
    let raw = samples.clone();
    if params.flags & NN != 0 {
        predict(&mut samples, (1 << params.bits_per_pixel) - 1);
    }
    let mut writer = BitWriter::default();
    for (index, chunk) in samples.chunks(block).enumerate() {
        let reference = usize::from(index == 0 && params.flags & NN != 0);
        encode_block(&mut writer, params, chunk, &raw[index * block..], reference);
    }
    writer.bytes
}

fn predict(samples: &mut [u32], max: u32) {
    let mut last = samples[0];
    for slot in &mut samples[1..] {
        let value = *slot;
        let theta = last.min(max - last);
        *slot = if value >= last {
            let delta = value - last;
            if delta <= theta {
                2 * delta
            } else {
                theta + delta
            }
        } else {
            let delta = last - value;
            if delta <= theta {
                2 * delta - 1
            } else {
                theta + delta
            }
        };
        last = value;
    }
}

fn encode_block(
    writer: &mut BitWriter,
    params: RiceParams,
    chunk: &[u32],
    raw: &[u32],
    reference: usize,
) {
    let bits = u32::from(params.bits_per_pixel);
    let coded = &chunk[reference..];
    let split_cost = |k: u32| -> u64 {
        coded
            .iter()
            .map(|&value| u64::from(value >> k) + 1 + u64::from(k))
            .sum()
    };
    let (best_k, best) = (0..=MAX_K)
        .map(|k| (k, split_cost(k)))
        .min_by_key(|&(_, cost)| cost)
        .unwrap_or((0, u64::MAX));
    let second = second_extension_words(coded, reference);
    let second_cost = second.as_ref().map_or(u64::MAX, |words| {
        words.iter().map(|&m| u64::from(m) + 1).sum::<u64>() + 1
    });
    let raw_cost = u64::from(bits) * chunk.len() as u64;
    let reference_value = (reference == 1).then(|| raw[0]);
    if coded.iter().all(|&value| value == 0) && reference == 0 {
        writer.put(0, ID_LEN);
        writer.put(0, 1);
        writer.fundamental(0);
    } else if second_cost < best && second_cost < raw_cost {
        writer.put(0, ID_LEN);
        writer.put(1, 1);
        if let Some(value) = reference_value {
            writer.put(value, bits);
        }
        for m in second.unwrap_or_default() {
            writer.fundamental(m);
        }
    } else if best < raw_cost {
        writer.put(best_k + 1, ID_LEN);
        if let Some(value) = reference_value {
            writer.put(value, bits);
        }
        for &value in coded {
            writer.fundamental(value >> best_k);
        }
        for &value in coded {
            writer.put(value & ((1 << best_k) - 1), best_k);
        }
    } else {
        writer.put(RAW_ID, ID_LEN);
        if let Some(value) = reference_value {
            writer.put(value, bits);
        }
        for &value in coded {
            writer.put(value, bits);
        }
    }
}

fn second_extension_words(coded: &[u32], reference: usize) -> Option<Vec<u32>> {
    let word = |a: u32, b: u32| {
        let beta = a + b;
        (beta <= 12).then(|| beta * (beta + 1) / 2 + b)
    };
    let mut words = Vec::new();
    let mut rest = coded;
    if reference == 1 {
        let (&first, tail) = rest.split_first()?;
        words.push(word(0, first)?);
        rest = tail;
    }
    for pair in rest.chunks(2) {
        let second = pair.get(1).copied().unwrap_or(0);
        words.push(word(pair[0], second)?);
    }
    Some(words)
}

/// BPSK at `sps` samples a symbol, RRC-shaped, carrying the coded bits of `cadus`.
#[must_use]
pub fn modulate(
    cadus: &[Cadu],
    precoding: Precoding,
    sps: usize,
    roll_off: f64,
) -> Vec<Complex<f32>> {
    let labels: Vec<u32> = coded_bits_with(cadus, precoding)
        .into_iter()
        .map(u32::from)
        .collect();
    let pulse = root_raised_cosine(sps as f64, roll_off, 8, Norm::Energy);
    match LinearParams::new(tables::bpsk(), pulse, sps) {
        Ok(params) => LinearMod::transmission(&params, &labels),
        Err(_) => Vec::new(),
    }
}
