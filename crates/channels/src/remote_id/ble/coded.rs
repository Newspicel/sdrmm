use sdrmm_dsp::fec::conv_soft::SoftViterbi;
use sdrmm_wire::RemoteIdPhy;

use super::{
    CRC_BYTES, HEADER_BYTES, Whitener,
    gfsk::{Level, Read, Reader, SPS, access_address_bits},
};

const PREAMBLE_PERIOD: [bool; 8] = [false, false, true, true, true, true, false, false];
const PREAMBLE_CHECK: usize = 16;
const PREAMBLE_ERRORS: usize = 1;
const S8_SYMBOLS: usize = 4;
const AA_SYMBOLS: usize = 32 * 2 * S8_SYMBOLS;
pub(crate) const SYNC_SYMBOLS: usize = PREAMBLE_CHECK + AA_SYMBOLS;
pub(crate) const SYNC_SAMPLES: usize = SYNC_SYMBOLS * SPS;
const AA_ERRORS: usize = 48;
const EARLY_SYMBOLS: usize = 32;
const EARLY_ERRORS: usize = 8;
const TERM_BITS: usize = 3;
const CI_BITS: usize = 2;
const HEADER_MARGIN_BITS: usize = 24;
const MIN_SWING: f32 = 0.25;
const G0: u32 = 0b1111;
const G1: u32 = 0b1011;
const K: u32 = 4;

pub(crate) const SCREEN_END: usize = (PREAMBLE_CHECK - 2) * SPS;
const SCREEN_MASK: u64 = 1 << 12 | 1 << 8 | 1 << 4 | 1;
const SCREEN_EDGES: u64 = 1 << 12 | 1 << 4;

pub(crate) fn screen(rising: u64) -> bool {
    ((rising ^ SCREEN_EDGES) & SCREEN_MASK).count_ones() as usize <= PREAMBLE_ERRORS
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Encoder(u8);

impl Encoder {
    pub(crate) fn push(&mut self, bit: bool) -> [bool; 2] {
        let register = u32::from(bit) << 3 | u32::from(self.0);
        self.0 = (self.0 >> 1) | u8::from(bit) << 2;
        [
            (register & G0).count_ones() & 1 == 1,
            (register & G1).count_ones() & 1 == 1,
        ]
    }
}

pub(crate) fn s8_pattern(coded: bool) -> [bool; S8_SYMBOLS] {
    if coded {
        [true, true, false, false]
    } else {
        [false, false, true, true]
    }
}

pub(crate) fn encode_s8(encoder: &mut Encoder, bits: impl Iterator<Item = bool>) -> Vec<bool> {
    bits.flat_map(|bit| encoder.push(bit))
        .flat_map(s8_pattern)
        .collect()
}

pub(crate) fn ci_bits(phy: RemoteIdPhy) -> [bool; CI_BITS] {
    [phy == RemoteIdPhy::LeCodedS2, false]
}

fn known_symbols() -> (Vec<bool>, Encoder) {
    let mut encoder = Encoder::default();
    let mut symbols: Vec<bool> = (0..PREAMBLE_CHECK)
        .map(|k| PREAMBLE_PERIOD[k % 8])
        .collect();
    symbols.extend(encode_s8(&mut encoder, access_address_bits()));
    (symbols, encoder)
}

pub(crate) struct SyncCoded {
    pattern: Vec<bool>,
    indicators: [(RemoteIdPhy, Vec<bool>); 2],
    viterbi: SoftViterbi,
}

impl SyncCoded {
    pub(crate) fn new() -> Self {
        let (pattern, after_access_address) = known_symbols();
        let indicator = |phy| {
            let mut encoder = after_access_address;
            let bits = ci_bits(phy)
                .into_iter()
                .chain(std::iter::repeat_n(false, TERM_BITS));
            (phy, encode_s8(&mut encoder, bits))
        };
        Self {
            indicators: [
                indicator(RemoteIdPhy::LeCodedS8),
                indicator(RemoteIdPhy::LeCodedS2),
            ],
            pattern,
            viterbi: SoftViterbi::new(K, G0, G1),
        }
    }

    pub(crate) fn check(&self, history: &[f32], at: usize) -> Option<Level> {
        let sample = |symbol: usize| history[at + symbol * SPS];
        let broken_edges = (1..PREAMBLE_CHECK - 1)
            .step_by(4)
            .filter(|&symbol| (sample(symbol) < sample(symbol + 1)) != self.pattern[symbol + 1])
            .count();
        if broken_edges > PREAMBLE_ERRORS {
            return None;
        }
        let mean = (0..PREAMBLE_CHECK).map(sample).sum::<f32>() / PREAMBLE_CHECK as f32;
        let preamble_errors = (0..PREAMBLE_CHECK)
            .filter(|&symbol| (sample(symbol) > mean) != self.pattern[symbol])
            .count();
        if preamble_errors > PREAMBLE_ERRORS {
            return None;
        }
        let mut errors = 0;
        for symbol in PREAMBLE_CHECK..SYNC_SYMBOLS {
            errors += usize::from((sample(symbol) > mean) != self.pattern[symbol]);
            let checked = symbol + 1 - PREAMBLE_CHECK;
            if errors > AA_ERRORS || (checked == EARLY_SYMBOLS && errors > EARLY_ERRORS) {
                return None;
            }
        }
        let level =
            Level::fit((0..SYNC_SYMBOLS).map(|symbol| (sample(symbol), self.pattern[symbol])))?;
        (level.swing > MIN_SWING).then_some(level)
    }

    pub(crate) fn score(&self, history: &[f32], at: usize, level: Level) -> f32 {
        self.pattern
            .iter()
            .enumerate()
            .map(|(symbol, &bit)| {
                let value = history[at + symbol * SPS] - level.dc;
                if bit { value } else { -value }
            })
            .sum()
    }

    pub(crate) fn read(
        &self,
        reader: &mut Reader<'_>,
        channel_index: u8,
    ) -> Result<(RemoteIdPhy, Vec<u8>), Read> {
        let span = self.indicators[0].1.len();
        if reader.remaining() < span {
            return Err(Read::Short);
        }
        let mut values = Vec::with_capacity(span);
        for _ in 0..span {
            values.push(reader.symbol().ok_or(Read::Short)?);
        }
        let phy = self
            .indicators
            .iter()
            .map(|(phy, expected)| (*phy, correlate(&values, expected)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(RemoteIdPhy::LeCodedS8, |(phy, _)| phy);
        let mut llrs = Vec::new();
        let header_bits = HEADER_BYTES * 8 + HEADER_MARGIN_BITS;
        collect(reader, phy, header_bits, &mut llrs)?;
        let header = bytes(&self.viterbi.decode(&llrs), HEADER_BYTES);
        let mut whitener = Whitener::new(channel_index);
        whitener.byte(header[0]);
        let length = usize::from(whitener.byte(header[1]));
        let total = HEADER_BYTES + length + CRC_BYTES;
        let bits = total * 8 + TERM_BITS;
        collect(reader, phy, bits.saturating_sub(header_bits), &mut llrs)?;
        let decoded = self.viterbi.decode(&llrs);
        let mut whitener = Whitener::new(channel_index);
        let mut pdu: Vec<u8> = bytes(&decoded, total)
            .into_iter()
            .map(|byte| whitener.byte(byte))
            .collect();
        if super::crc_ok(&pdu) {
            pdu.truncate(total - CRC_BYTES);
            Ok((phy, pdu))
        } else {
            Err(Read::Bad)
        }
    }
}

fn correlate(values: &[f32], expected: &[bool]) -> f32 {
    values
        .iter()
        .zip(expected)
        .map(|(&value, &bit)| if bit { value } else { -value })
        .sum()
}

fn collect(
    reader: &mut Reader<'_>,
    phy: RemoteIdPhy,
    bits: usize,
    llrs: &mut Vec<f32>,
) -> Result<(), Read> {
    let per_coded = if phy == RemoteIdPhy::LeCodedS8 {
        S8_SYMBOLS
    } else {
        1
    };
    if reader.remaining() < bits * 2 * per_coded {
        return Err(Read::Short);
    }
    for _ in 0..bits * 2 {
        let llr = if per_coded == 1 {
            reader.symbol().ok_or(Read::Short)?
        } else {
            let mut chips = [0.0f32; S8_SYMBOLS];
            for chip in &mut chips {
                *chip = reader.symbol().ok_or(Read::Short)?;
            }
            (chips[0] + chips[1] - chips[2] - chips[3]) * 0.5
        };
        llrs.push(llr);
    }
    Ok(())
}

fn bytes(bits: &[u8], count: usize) -> Vec<u8> {
    bits.chunks(8)
        .take(count)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (bit, &value)| acc | (value & 1) << bit)
        })
        .collect()
}
