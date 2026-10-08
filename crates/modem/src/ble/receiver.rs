use num_complex::Complex;
use sdrmm_dsp::{Decimator, fast_arg};

use super::{
    BlePhy, CRC_BYTES, HEADER_BYTES, MAX_PDU_BYTES, Packet, SPS, Sink, Whitener,
    access_address_bits,
    coded::{
        AA_BLOCKS, INDICATOR_BLOCKS, TERM_BITS, Viterbi, coded_access_address, indicator,
        s8_pattern,
    },
    crc_ok, gfsk,
    track::{
        Acquired, Blocks, Coherent, acquire, carrier_from_pilot, pseudo_symbols, smoothed_soft,
    },
};
use crate::cpm::{LaurentError, laurent_main_pulse};

const ONE_SYMBOLS: usize = 40;
const ONE_ERRORS: usize = 6;
const MIN_COHERENCE: f32 = 0.6;
const ONE_SCREEN_ERRORS: u32 = 2;
const ONE_BALANCED: std::ops::RangeInclusive<usize> = 1..=8;
const ONE_SCREEN_MASK: u64 = 0x7F;
const ONE_SCREEN: u64 = 0x2A;
const ONE_SCREEN_BACK: usize = (ONE_SYMBOLS - 1 - 8) * SPS;
const ONE_SEGMENT: usize = 8;
const CODED_ERRORS: u32 = 12;
const CODED_SEGMENT: usize = 16;
const BLOCK_SAMPLES: usize = 4 * SPS;
const AA_SYMBOLS: usize = AA_BLOCKS * 4;
const KEEP_BEHIND: usize = AA_SYMBOLS * SPS + 64;
const MAX_HISTORY: usize = 90_000;
const HEADER_BITS: usize = HEADER_BYTES * 8;
const HEADER_MARGIN_BITS: usize = 24;
const MARGIN_SAMPLES: f64 = (2 * BLOCK_SAMPLES) as f64;
const RETRY_SKIP: usize = 3;
const REFILTER_PAD: usize = 64;
const ANCHOR_SEARCH: usize = 2 * SPS;
const ONE_LONGEST: usize = ((MAX_PDU_BYTES + CRC_BYTES) * 8 + 8) * SPS;
const CODED_LONGEST: usize =
    (INDICATOR_BLOCKS + 2 * ((MAX_PDU_BYTES + CRC_BYTES) * 8 + TERM_BITS) + 4) * BLOCK_SAMPLES;
const PREAMBLE_BITS: usize = 8;

struct Patterns {
    one_symbols: Vec<bool>,
    one_known: Vec<Complex<f32>>,
    coded: u64,
    coded_known: Vec<Complex<f32>>,
    indicators: [(BlePhy, [bool; INDICATOR_BLOCKS]); 2],
}

impl Patterns {
    fn new() -> Self {
        let one_symbols: Vec<bool> = (0..PREAMBLE_BITS)
            .map(|bit| bit % 2 == 1)
            .chain(access_address_bits())
            .collect();
        let mut one_known = Vec::with_capacity(ONE_SYMBOLS);
        pseudo_symbols(one_symbols.iter().copied(), &mut one_known);
        let (coded_bits, after) = coded_access_address();
        let coded = (0..AA_BLOCKS).fold(0u64, |acc, j| {
            acc | u64::from(coded_bits[AA_BLOCKS - 1 - j]) << j
        });
        let mut coded_known = Vec::with_capacity(AA_SYMBOLS);
        pseudo_symbols(
            coded_bits.iter().flat_map(|&bit| s8_pattern(bit)),
            &mut coded_known,
        );
        Self {
            one_symbols,
            one_known,
            coded,
            coded_known,
            indicators: [
                (BlePhy::CodedS8, indicator(after, BlePhy::CodedS8)),
                (BlePhy::CodedS2, indicator(after, BlePhy::CodedS2)),
            ],
        }
    }
}

struct Scratch {
    pdu: [u8; MAX_PDU_BYTES + CRC_BYTES],
    pilots: Vec<Complex<f32>>,
    data: Vec<Complex<f32>>,
    soft: Vec<f32>,
    bits: Vec<bool>,
    viterbi: Viterbi,
}

impl Scratch {
    fn new() -> Self {
        let coded = 2 * ((MAX_PDU_BYTES + CRC_BYTES) * 8 + TERM_BITS) + INDICATOR_BLOCKS;
        Self {
            pdu: [0; MAX_PDU_BYTES + CRC_BYTES],
            pilots: Vec::with_capacity(coded),
            data: Vec::with_capacity(coded),
            soft: Vec::with_capacity(coded),
            bits: Vec::with_capacity(coded),
            viterbi: Viterbi::new(),
        }
    }
}

enum Attempt {
    Done(usize, usize, BlePhy, usize),
    Wait,
    Miss,
}

pub struct Lane {
    rf: Option<u8>,
    channel_index: u8,
    matched: Decimator,
    filtered: Vec<Complex<f32>>,
    y: Vec<Complex<f32>>,
    e: Vec<f32>,
    angles: Vec<f32>,
    rising: Vec<u64>,
    blocks: Vec<u64>,
    raw: Vec<Complex<f32>>,
    refilter: Refilter,
    consumed: u64,
    cursor: usize,
    patterns: Patterns,
    scratch: Scratch,
}

struct Refilter {
    filter: Decimator,
    rotated: Vec<Complex<f32>>,
    out: Vec<Complex<f32>>,
}

impl Refilter {
    fn new(pulse: &[f32]) -> Self {
        Self {
            filter: Decimator::new(pulse, 1),
            rotated: Vec::with_capacity(MAX_HISTORY),
            out: Vec::with_capacity(MAX_HISTORY),
        }
    }

    fn run(&mut self, raw: &[Complex<f32>], omega: f32, origin: usize) {
        self.rotated.clear();
        let step = Complex::from_polar(1.0, -omega);
        let mut turn = Complex::from_polar(1.0, omega * origin as f32);
        for &sample in raw {
            self.rotated.push(sample * turn);
            turn *= step;
        }
        self.filter.reset();
        self.filter.process(&self.rotated, &mut self.out);
    }
}

#[derive(Clone, Copy)]
enum Kind {
    One,
    Coded,
}

impl Lane {
    pub fn new(rf: Option<u8>, channel_index: u8) -> Result<Self, LaurentError> {
        let pulse = laurent_main_pulse(&gfsk(SPS as f64))?;
        Ok(Self {
            rf,
            channel_index,
            matched: Decimator::new(&pulse, 1),
            refilter: Refilter::new(&pulse),
            filtered: Vec::new(),
            y: Vec::with_capacity(MAX_HISTORY),
            e: Vec::with_capacity(MAX_HISTORY),
            angles: Vec::with_capacity(MAX_HISTORY),
            rising: Vec::with_capacity(MAX_HISTORY),
            blocks: Vec::with_capacity(MAX_HISTORY),
            raw: Vec::with_capacity(MAX_HISTORY),
            consumed: 0,
            cursor: 0,
            patterns: Patterns::new(),
            scratch: Scratch::new(),
        })
    }

    pub fn reset(&mut self) {
        self.matched.reset();
        self.y.clear();
        self.e.clear();
        self.angles.clear();
        self.rising.clear();
        self.blocks.clear();
        self.raw.clear();
        self.consumed = 0;
        self.cursor = 0;
    }

    pub fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.matched.process(iq, &mut self.filtered);
        for index in 0..self.filtered.len() {
            self.push(self.filtered[index]);
        }
        self.raw.extend_from_slice(iq);
        self.scan(sink);
    }

    fn push(&mut self, value: Complex<f32>) {
        let n = self.y.len();
        self.y.push(value);
        let step = n
            .checked_sub(SPS)
            .map_or(Complex::new(0.0, 0.0), |earlier| {
                value * self.y[earlier].conj()
            });
        let e = step.im;
        self.e.push(e);
        let angle = fast_arg(step);
        self.angles.push(angle);
        let rising = n.checked_sub(SPS).map_or(0, |earlier| {
            self.rising[earlier] << 1 | u64::from(angle > self.angles[earlier])
        });
        self.rising.push(rising);
        let block = n.checked_sub(BLOCK_SAMPLES).map_or(0, |earlier| {
            let metric = self.e[n - 3 * SPS] + self.e[n - 2 * SPS] - self.e[n - SPS] - e;
            self.blocks[earlier] << 1 | u64::from(metric > 0.0)
        });
        self.blocks.push(block);
    }

    fn scan(&mut self, sink: &mut impl Sink) {
        let mut at = self.cursor;
        while at < self.y.len() {
            let one = at.checked_sub(ONE_SCREEN_BACK).is_some_and(|back| {
                let screen = ((self.rising[back] ^ ONE_SCREEN) & ONE_SCREEN_MASK).count_ones()
                    <= ONE_SCREEN_ERRORS;
                screen && one_sync(&self.angles, at, &self.patterns.one_symbols)
            });
            let coded = (self.blocks[at] ^ self.patterns.coded).count_ones() <= CODED_ERRORS;
            if !one && !coded {
                at += 1;
                continue;
            }
            let mut attempt = if one {
                self.attempt(Kind::One, at)
            } else {
                Attempt::Miss
            };
            if coded && matches!(attempt, Attempt::Miss) {
                let Some(best) = self.best_coded_anchor(at) else {
                    break;
                };
                attempt = self.attempt(Kind::Coded, best);
                if matches!(attempt, Attempt::Miss) {
                    at = best + RETRY_SKIP;
                    continue;
                }
            }
            match attempt {
                Attempt::Done(start, end, phy, length) => {
                    let start = start.min(end.saturating_sub(1));
                    let window = &self.raw[start..end.min(self.raw.len())];
                    let mean = window.iter().map(Complex::norm_sqr).sum::<f32>()
                        / window.len().max(1) as f32;
                    sink.packet(Packet {
                        pdu: &self.scratch.pdu[..length],
                        phy,
                        rf: self.rf,
                        level_dbfs: 10.0 * mean.max(1e-20).log10(),
                        sample: self.consumed + start as u64,
                    });
                    at = end.max(at + 1);
                }
                Attempt::Wait => break,
                Attempt::Miss => at += 1,
            }
        }
        self.cursor = at.min(self.y.len());
        self.trim();
    }

    fn best_coded_anchor(&self, at: usize) -> Option<usize> {
        let last = at + ANCHOR_SEARCH;
        if last >= self.blocks.len() {
            return None;
        }
        (at..=last).min_by_key(|&index| (self.blocks[index] ^ self.patterns.coded).count_ones())
    }

    fn attempt(&mut self, kind: Kind, anchor: usize) -> Attempt {
        let (known, segment, longest) = match kind {
            Kind::One => (&self.patterns.one_known, ONE_SEGMENT, ONE_LONGEST),
            Kind::Coded => (&self.patterns.coded_known, CODED_SEGMENT, CODED_LONGEST),
        };
        let Some(coarse) = acquire(&self.y, anchor, known, segment) else {
            return Attempt::Miss;
        };
        let base = anchor.saturating_sub(known.len() * SPS + REFILTER_PAD);
        let end = self.raw.len().min(self.y.len()).min(anchor + longest);
        let raw = &self.raw[base..end];
        self.refilter.run(raw, coarse.omega, anchor - base);
        let mut decoder = Decoder {
            y: &self.refilter.out,
            channel_index: self.channel_index,
            patterns: &self.patterns,
            scratch: &mut self.scratch,
        };
        let attempt = match kind {
            Kind::One => decoder.one(anchor - base),
            Kind::Coded => decoder.coded(anchor - base),
        };
        match attempt {
            Attempt::Done(start, end, phy, length) => {
                Attempt::Done(start + base, end + base, phy, length)
            }
            other => other,
        }
    }

    fn trim(&mut self) {
        let drop = self
            .cursor
            .saturating_sub(KEEP_BEHIND)
            .max(self.y.len().saturating_sub(MAX_HISTORY));
        if drop == 0 {
            return;
        }
        self.y.drain(..drop);
        self.e.drain(..drop);
        self.angles.drain(..drop);
        self.rising.drain(..drop);
        self.blocks.drain(..drop);
        self.raw.drain(..drop.min(self.raw.len()));
        self.consumed += drop as u64;
        self.cursor = self.cursor.saturating_sub(drop);
    }
}

struct Decoder<'a> {
    y: &'a [Complex<f32>],
    channel_index: u8,
    patterns: &'a Patterns,
    scratch: &'a mut Scratch,
}

impl Decoder<'_> {
    fn one(&mut self, anchor: usize) -> Attempt {
        let Some(acquired) = acquire(self.y, anchor, &self.patterns.one_known, ONE_SEGMENT)
            .filter(|acquired| acquired.coherence >= MIN_COHERENCE)
        else {
            return Attempt::Miss;
        };
        let start = anchor.saturating_sub(ONE_SYMBOLS * SPS);
        let view = acquired.view(self.y);
        let mut detector = Coherent::new(view, acquired.instant, acquired.carrier, acquired.last);
        if !self.available(detector.position(), HEADER_BITS) {
            return Attempt::Wait;
        }
        let mut whitener = Whitener::new(self.channel_index);
        for slot in 0..HEADER_BYTES {
            let Some(byte) = read_byte(&mut detector) else {
                return Attempt::Wait;
            };
            self.scratch.pdu[slot] = whitener.byte(byte);
        }
        let total = HEADER_BYTES + usize::from(self.scratch.pdu[1]) + CRC_BYTES;
        if !self.available(detector.position(), (total - HEADER_BYTES) * 8) {
            return Attempt::Wait;
        }
        for slot in HEADER_BYTES..total {
            let Some(byte) = read_byte(&mut detector) else {
                return Attempt::Wait;
            };
            self.scratch.pdu[slot] = whitener.byte(byte);
        }
        if crc_ok(&self.scratch.pdu[..total]) {
            Attempt::Done(
                start,
                detector.position() as usize,
                BlePhy::Le1m,
                total - CRC_BYTES,
            )
        } else {
            Attempt::Miss
        }
    }

    fn available(&self, position: f64, symbols: usize) -> bool {
        position + (symbols * SPS) as f64 + MARGIN_SAMPLES < self.y.len() as f64
    }

    fn coded(&mut self, anchor: usize) -> Attempt {
        let Some(acquired) = acquire(self.y, anchor, &self.patterns.coded_known, CODED_SEGMENT)
            .filter(|acquired| acquired.coherence >= MIN_COHERENCE)
        else {
            return Attempt::Miss;
        };
        let header_coded = 2 * (HEADER_BITS + HEADER_MARGIN_BITS);
        if !self.available(acquired.instant, (INDICATOR_BLOCKS + header_coded) * 4) {
            return Attempt::Wait;
        }
        let view = acquired.view(self.y);
        let mut blocks = Blocks::new(
            view,
            acquired.instant,
            acquired.carrier * acquired.last * 2.0,
        );
        self.scratch.pilots.clear();
        self.scratch.data.clear();
        if !read_blocks(&mut blocks, INDICATOR_BLOCKS, self.scratch) {
            return Attempt::Wait;
        }
        let phy = self.indicator();
        let start = anchor.saturating_sub(AA_SYMBOLS * SPS);
        let outcome = match phy {
            BlePhy::CodedS8 => self.s8(&mut blocks, header_coded),
            _ => self.s2(&acquired, &blocks, header_coded),
        };
        match outcome {
            Some(Some((end, length))) => Attempt::Done(start, end, phy, length),
            Some(None) => Attempt::Miss,
            None => Attempt::Wait,
        }
    }

    fn indicator(&mut self) -> BlePhy {
        self.scratch.soft.clear();
        smoothed_soft(
            &self.scratch.pilots,
            &self.scratch.data,
            &mut self.scratch.soft,
        );
        self.patterns
            .indicators
            .iter()
            .map(|(phy, expected)| {
                let score: f32 = self
                    .scratch
                    .soft
                    .iter()
                    .zip(expected)
                    .map(|(&soft, &bit)| if bit { soft } else { -soft })
                    .sum();
                (*phy, score)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(BlePhy::CodedS8, |(phy, _)| phy)
    }

    fn s8(
        &mut self,
        blocks: &mut Blocks<'_>,
        header_coded: usize,
    ) -> Option<Option<(usize, usize)>> {
        if !read_blocks(blocks, header_coded, self.scratch) {
            return None;
        }
        let length = self.coded_length(true)?;
        let coded = 2 * ((HEADER_BYTES + length + CRC_BYTES) * 8 + TERM_BITS);
        let remaining = coded.saturating_sub(header_coded);
        if !self.available(blocks.position(), remaining * 4) {
            return None;
        }
        if !read_blocks(blocks, remaining, self.scratch) {
            return None;
        }
        self.scratch.soft.clear();
        smoothed_soft(
            &self.scratch.pilots,
            &self.scratch.data,
            &mut self.scratch.soft,
        );
        Some(self.finish(INDICATOR_BLOCKS, coded, length, blocks.position()))
    }

    fn s2(
        &mut self,
        acquired: &Acquired,
        blocks: &Blocks<'_>,
        header_coded: usize,
    ) -> Option<Option<(usize, usize)>> {
        let carrier = carrier_from_pilot(blocks.reference(), acquired.last);
        let view = acquired.view(self.y);
        let mut detector = Coherent::new(view, blocks.position(), carrier, acquired.last);
        self.scratch.soft.clear();
        for _ in 0..header_coded {
            let soft = detector.symbol()?;
            self.scratch.soft.push(soft);
        }
        let length = self.coded_length(false)?;
        let coded = 2 * ((HEADER_BYTES + length + CRC_BYTES) * 8 + TERM_BITS);
        let remaining = coded.saturating_sub(header_coded);
        if !self.available(detector.position(), remaining) {
            return None;
        }
        for _ in 0..remaining {
            let soft = detector.symbol()?;
            self.scratch.soft.push(soft);
        }
        Some(self.finish(0, coded, length, detector.position()))
    }

    fn coded_length(&mut self, smooth: bool) -> Option<usize> {
        let first = if smooth {
            self.scratch.soft.clear();
            smoothed_soft(
                &self.scratch.pilots,
                &self.scratch.data,
                &mut self.scratch.soft,
            );
            INDICATOR_BLOCKS
        } else {
            0
        };
        let scratch = &mut self.scratch;
        let header_coded = 2 * (HEADER_BITS + HEADER_MARGIN_BITS);
        scratch.bits.clear();
        scratch.viterbi.decode(
            scratch.soft.get(first..first + header_coded)?,
            false,
            &mut scratch.bits,
        );
        let mut whitener = Whitener::new(self.channel_index);
        whitener.byte(byte_of(&scratch.bits[..8]));
        Some(usize::from(whitener.byte(byte_of(&scratch.bits[8..16]))))
    }

    fn finish(
        &mut self,
        first: usize,
        coded: usize,
        length: usize,
        end: f64,
    ) -> Option<(usize, usize)> {
        let scratch = &mut self.scratch;
        let soft = scratch.soft.get(first..first + coded)?;
        scratch.bits.clear();
        scratch.viterbi.decode(soft, true, &mut scratch.bits);
        let total = HEADER_BYTES + length + CRC_BYTES;
        let mut whitener = Whitener::new(self.channel_index);
        for (slot, bits) in scratch.pdu[..total]
            .iter_mut()
            .zip(scratch.bits.as_chunks::<8>().0)
        {
            *slot = whitener.byte(byte_of(bits));
        }
        crc_ok(&scratch.pdu[..total]).then_some((end as usize, total - CRC_BYTES))
    }
}

fn one_sync(angles: &[f32], anchor: usize, symbols: &[bool]) -> bool {
    let Some(first) = anchor.checked_sub((ONE_SYMBOLS - 1) * SPS) else {
        return false;
    };
    let angle = |k: usize| angles[first + k * SPS];
    let mean = ONE_BALANCED.map(angle).sum::<f32>() / ONE_BALANCED.count() as f32;
    let mut errors = 0;
    let start = *ONE_BALANCED.start();
    for (k, &symbol) in symbols.iter().enumerate().take(ONE_SYMBOLS).skip(start) {
        errors += usize::from((angle(k) > mean) != symbol);
        if errors > ONE_ERRORS {
            return false;
        }
    }
    true
}

fn read_blocks(blocks: &mut Blocks<'_>, count: usize, scratch: &mut Scratch) -> bool {
    for _ in 0..count {
        let Some((pilot, data)) = blocks.block() else {
            return false;
        };
        scratch.pilots.push(pilot);
        scratch.data.push(data);
    }
    true
}

fn read_byte(detector: &mut Coherent<'_>) -> Option<u8> {
    (0..8).try_fold(0u8, |acc, bit| {
        Some(acc | u8::from(detector.symbol()? > 0.0) << bit)
    })
}

fn byte_of(bits: &[bool]) -> u8 {
    bits.iter()
        .enumerate()
        .fold(0u8, |acc, (bit, &value)| acc | u8::from(value) << bit)
}
