//! Frame sync, de-framing and LDPC decoding of both Wenet framings.
//!
//! On air a frame is 16 bytes of 0x55, the unique word 0xABCDEF01, a 256-byte packet, its
//! CRC-16 and 65 bytes of LDPC parity (516 bits padded). Legacy Wenet sends every byte as an
//! RS232 word (start bit, LSB first, stop bit) at 115177 baud; Wenet v2 sends bits MSB first at
//! 96000 baud with the bytes after the unique word scrambled.

use sdrmm_dsp::coding::fec::crc16_msb;
use sdrmm_dsp::coding::fec::ldpc::{MinSum, ParityCheck, RepeatAccumulate, Schedule};

use super::code::ROWS;
use super::model::WenetMode;

pub(crate) const PACKET_LEN: usize = 256;
pub(crate) const CRC_LEN: usize = 2;
pub(crate) const PARITY_LEN: usize = 65;
pub(crate) const DATA_BITS: usize = (PACKET_LEN + CRC_LEN) * 8;
pub(crate) const PARITY_BITS: usize = 516;
pub(crate) const CODE_BITS: usize = DATA_BITS + PARITY_BITS;
pub(crate) const FRAME_BYTES: usize = PACKET_LEN + CRC_LEN + PARITY_LEN;
pub(crate) const PREAMBLE: u8 = 0x55;
pub(crate) const PREAMBLE_LEN: usize = 16;
pub(crate) const UNIQUE_WORD: [u8; 4] = [0xAB, 0xCD, 0xEF, 0x01];
pub(crate) const UART_BITS: usize = 10;

/// The byte-wise XOR Wenet v2 lays over everything after the unique word.
pub(crate) const SCRAMBLE: [u8; 125] = [
    0xB9, 0x97, 0x93, 0x13, 0xF7, 0xAB, 0x1E, 0x88, 0x12, 0xC4, 0x28, 0x80, 0x09, 0xF8, 0xB4, 0x92,
    0xFC, 0x32, 0xC6, 0xA6, 0xAE, 0xF7, 0x8B, 0x3A, 0xD2, 0xF1, 0xF1, 0x8A, 0x72, 0xCF, 0x3D, 0xC3,
    0x9E, 0x52, 0x6E, 0x7A, 0x7E, 0x37, 0xA2, 0x07, 0x17, 0x71, 0x2D, 0x9D, 0x1C, 0x58, 0xC1, 0xB4,
    0x65, 0xE4, 0xBE, 0x5B, 0xD1, 0x0F, 0xA0, 0x5A, 0x3C, 0x6F, 0xD9, 0x08, 0x9C, 0x6C, 0x5C, 0x6E,
    0x85, 0x94, 0xB1, 0x5D, 0xDE, 0xD4, 0xC3, 0x55, 0x20, 0x61, 0xD7, 0x6A, 0x81, 0x78, 0x52, 0x46,
    0x7C, 0x43, 0x40, 0x63, 0xF1, 0x25, 0xCB, 0xF1, 0x8C, 0xA7, 0x83, 0x5C, 0xA3, 0xBA, 0x5C, 0xA3,
    0xC5, 0xB6, 0x0F, 0x2A, 0x64, 0x5F, 0xEC, 0x98, 0xCF, 0xF5, 0xB6, 0x3D, 0x96, 0x42, 0x16, 0x07,
    0xEC, 0x20, 0x32, 0x4D, 0xC6, 0x17, 0x92, 0xA6, 0x91, 0xC1, 0x92, 0x43, 0x69,
];

/// Preamble bits matched ahead of the unique word, so random data rarely looks like sync.
const PREAMBLE_BITS: usize = 16;
const MAX_SYMBOLS: usize = FRAME_BYTES * UART_BITS;
const LDPC_ITERATIONS: usize = 30;
const MIN_SUM_SCALE: f32 = 0.75;
const LLR_GAIN: f32 = 4.0;
const LLR_LIMIT: f32 = 20.0;

/// The Wenet LDPC code.
pub(crate) fn code() -> RepeatAccumulate {
    let rows = ROWS
        .iter()
        .map(|row| {
            row.iter()
                .map(|&position| usize::from(position) - 1)
                .collect()
        })
        .collect();
    RepeatAccumulate::new(DATA_BITS, rows)
}

/// CRC-16/CCITT-FALSE, sent little-endian after the packet.
pub(crate) fn crc(packet: &[u8]) -> u16 {
    crc16_msb(0x1021, 0xFFFF, packet)
}

/// The bits on air before the packet, MSB of `pattern` first.
fn sync_word(mode: WenetMode) -> (u64, usize) {
    let mut pattern = 0u64;
    let mut bits = 0;
    let mut push = |bit: bool| {
        pattern = pattern << 1 | u64::from(bit);
        bits += 1;
    };
    for index in 0..PREAMBLE_BITS {
        push(index % 2 == 1);
    }
    for byte in UNIQUE_WORD {
        match mode {
            WenetMode::Uart => {
                push(false);
                (0..8).for_each(|bit| push(byte >> bit & 1 == 1));
                push(true);
            }
            WenetMode::I2s => (0..8).rev().for_each(|bit| push(byte >> bit & 1 == 1)),
        }
    }
    (pattern, bits)
}

/// What one frame decoded to; a decoded packet is [`Deframer::packet`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The CRC holds after LDPC repaired `repaired` code bits.
    Decoded {
        repaired: u32,
    },
    Failed,
}

pub(crate) struct Deframer {
    mode: WenetMode,
    pattern: u64,
    mask: u64,
    tolerance: u32,
    register: u64,
    inverted: bool,
    collecting: Option<usize>,
    symbols: Box<[f32; MAX_SYMBOLS]>,
    llrs: Box<[f32; CODE_BITS]>,
    decoder: MinSum,
    bytes: [u8; PACKET_LEN + CRC_LEN],
}

impl Deframer {
    pub(crate) fn new(mode: WenetMode, check: ParityCheck) -> Self {
        let (pattern, bits) = sync_word(mode);
        Self {
            mode,
            pattern,
            mask: (1u64 << bits) - 1,
            tolerance: (bits / 9) as u32,
            register: 0,
            inverted: false,
            collecting: None,
            symbols: Box::new([0.0; MAX_SYMBOLS]),
            llrs: Box::new([0.0; CODE_BITS]),
            decoder: MinSum::new(check, Schedule::Layered, MIN_SUM_SCALE),
            bytes: [0; PACKET_LEN + CRC_LEN],
        }
    }

    /// The packet of the last frame that decoded.
    pub(crate) fn packet(&self) -> &[u8; PACKET_LEN] {
        self.bytes
            .first_chunk::<PACKET_LEN>()
            .unwrap_or(&[0; PACKET_LEN])
    }

    pub(crate) fn reset(&mut self) {
        self.register = 0;
        self.collecting = None;
    }

    const fn frame_symbols(&self) -> usize {
        match self.mode {
            WenetMode::Uart => FRAME_BYTES * UART_BITS,
            WenetMode::I2s => FRAME_BYTES * 8,
        }
    }

    /// Takes one soft symbol, positive for a 1; returns a frame when one completes.
    pub(crate) fn push(&mut self, symbol: f32) -> Option<Outcome> {
        if let Some(count) = self.collecting {
            self.symbols[count] = if self.inverted { -symbol } else { symbol };
            if count + 1 < self.frame_symbols() {
                self.collecting = Some(count + 1);
                return None;
            }
            self.collecting = None;
            self.register = 0;
            return Some(self.decode());
        }
        self.register = (self.register << 1 | u64::from(symbol > 0.0)) & self.mask;
        let errors = (self.register ^ self.pattern).count_ones();
        let inverted_errors = (!self.register & self.mask ^ self.pattern).count_ones();
        if errors <= self.tolerance || inverted_errors <= self.tolerance {
            self.inverted = inverted_errors < errors;
            self.collecting = Some(0);
        }
        None
    }

    fn decode(&mut self) -> Outcome {
        self.fill_llrs();
        let converged = self
            .decoder
            .decode(&self.llrs[..], LDPC_ITERATIONS)
            .is_some();
        let hard = self.decoder.hard();
        let mut bytes = [0u8; PACKET_LEN + CRC_LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (0..8).fold(0u8, |acc, bit| acc << 1 | u8::from(hard[index * 8 + bit]));
        }
        let (packet, sent) = bytes.split_at(PACKET_LEN);
        if crc(packet) != u16::from_le_bytes([sent[0], sent[1]]) {
            return Outcome::Failed;
        }
        let repaired = if converged {
            hard.iter()
                .zip(self.llrs.iter())
                .filter(|&(&bit, &llr)| bit != (llr < 0.0))
                .count() as u32
        } else {
            0
        };
        self.bytes = bytes;
        Outcome::Decoded { repaired }
    }

    fn fill_llrs(&mut self) {
        let used = self.frame_symbols();
        let level = self.symbols[..used].iter().map(|s| s.abs()).sum::<f32>() / used as f32;
        let gain = if level > 0.0 { LLR_GAIN / level } else { 0.0 };
        for (index, llr) in self.llrs.iter_mut().enumerate() {
            let (byte, bit) = (index / 8, index % 8);
            let soft = match self.mode {
                WenetMode::Uart => self.symbols[byte * UART_BITS + 8 - bit],
                WenetMode::I2s => {
                    let flip = SCRAMBLE[byte % SCRAMBLE.len()] >> (7 - bit) & 1 == 1;
                    let soft = self.symbols[index];
                    if flip { -soft } else { soft }
                }
            };
            *llr = (-soft * gain).clamp(-LLR_LIMIT, LLR_LIMIT);
        }
    }
}

/// Builds the on-air bits of one frame carrying `packet`, preamble first.
#[cfg(any(test, feature = "synth"))]
pub(crate) fn frame_bits(
    mode: WenetMode,
    code: &RepeatAccumulate,
    packet: &[u8; PACKET_LEN],
) -> Vec<bool> {
    let mut body = packet.to_vec();
    body.extend_from_slice(&crc(packet).to_le_bytes());
    let data: Vec<bool> = body
        .iter()
        .flat_map(|&byte| (0..8).rev().map(move |bit| byte >> bit & 1 == 1))
        .collect();
    let mut parity = vec![false; PARITY_BITS];
    code.encode(&data, &mut parity);
    parity.resize(PARITY_LEN * 8, false);
    for chunk in parity.chunks(8) {
        body.push(chunk.iter().fold(0u8, |acc, &bit| acc << 1 | u8::from(bit)));
    }
    if mode == WenetMode::I2s {
        for (index, byte) in body.iter_mut().enumerate() {
            *byte ^= SCRAMBLE[index % SCRAMBLE.len()];
        }
    }
    let mut bytes = vec![PREAMBLE; PREAMBLE_LEN];
    bytes.extend_from_slice(&UNIQUE_WORD);
    bytes.extend_from_slice(&body);
    let mut bits = Vec::with_capacity(bytes.len() * UART_BITS);
    for byte in bytes {
        match mode {
            WenetMode::Uart => {
                bits.push(false);
                bits.extend((0..8).map(|bit| byte >> bit & 1 == 1));
                bits.push(true);
            }
            WenetMode::I2s => bits.extend((0..8).rev().map(|bit| byte >> bit & 1 == 1)),
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(seed: u8) -> [u8; PACKET_LEN] {
        std::array::from_fn(|index| (index as u8).wrapping_mul(31).wrapping_add(seed))
    }

    fn soft(bits: &[bool], flips: &[usize]) -> Vec<f32> {
        bits.iter()
            .enumerate()
            .map(|(index, &bit)| {
                let value = if bit { 1.0 } else { -1.0 };
                if flips.contains(&index) {
                    -0.4 * value
                } else {
                    value
                }
            })
            .collect()
    }

    fn on_air(mode: WenetMode, code_bit: usize) -> usize {
        let head = PREAMBLE_LEN + UNIQUE_WORD.len();
        match mode {
            WenetMode::Uart => (head + code_bit / 8) * UART_BITS + 8 - code_bit % 8,
            WenetMode::I2s => head * 8 + code_bit,
        }
    }

    fn run(deframer: &mut Deframer, symbols: &[f32]) -> Vec<Outcome> {
        symbols.iter().filter_map(|&s| deframer.push(s)).collect()
    }

    fn decoded(deframer: &mut Deframer, symbols: &[f32]) -> Option<([u8; PACKET_LEN], u32)> {
        match run(deframer, symbols).as_slice() {
            [Outcome::Decoded { repaired }] => Some((*deframer.packet(), *repaired)),
            _ => None,
        }
    }

    #[test]
    fn the_crc_is_ccitt_false() {
        assert_eq!(crc(b"123456789"), 0x29B1);
    }

    #[test]
    fn the_sync_words_end_in_the_unique_word() {
        let (uart, bits) = sync_word(WenetMode::Uart);
        assert_eq!(bits, 56);
        assert_eq!(uart & 0x3FF, 0b01_0000_0001);
        assert_eq!(uart >> 40, 0x5555);
        let (i2s, bits) = sync_word(WenetMode::I2s);
        assert_eq!(bits, 48);
        assert_eq!(i2s & 0xFFFF_FFFF, 0xABCD_EF01);
        assert_eq!(i2s >> 32, 0x5555);
    }

    #[test]
    fn both_framings_round_trip_with_errors_the_ldpc_repairs() {
        let code = code();
        for mode in [WenetMode::Uart, WenetMode::I2s] {
            let bits = frame_bits(mode, &code, &packet(mode as u8));
            let flips: Vec<usize> = (0..12).map(|k| on_air(mode, 17 + k * 211)).collect();
            let mut deframer = Deframer::new(mode, code.parity_check());
            let (got, repaired) =
                decoded(&mut deframer, &soft(&bits, &flips)).expect("one decoded frame");
            assert_eq!(got, packet(mode as u8));
            assert!(repaired >= 12, "{mode:?} repaired {repaired}");
        }
    }

    #[test]
    fn an_inverted_signal_still_decodes() {
        let code = code();
        let bits = frame_bits(WenetMode::Uart, &code, &packet(3));
        let inverted: Vec<f32> = soft(&bits, &[]).iter().map(|s| -s).collect();
        let mut deframer = Deframer::new(WenetMode::Uart, code.parity_check());
        let (got, _) = decoded(&mut deframer, &inverted).expect("one decoded frame");
        assert_eq!(got, packet(3));
    }

    #[test]
    fn a_ruined_frame_fails_its_crc() {
        let code = code();
        let bits = frame_bits(WenetMode::I2s, &code, &packet(9));
        let start = (PREAMBLE_LEN + UNIQUE_WORD.len()) * 8;
        let flips: Vec<usize> = (start..start + 400).step_by(2).collect();
        let mut deframer = Deframer::new(WenetMode::I2s, code.parity_check());
        assert_eq!(
            run(&mut deframer, &soft(&bits, &flips)),
            vec![Outcome::Failed]
        );
    }
}
