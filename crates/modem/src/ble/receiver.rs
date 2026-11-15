mod decoder;
mod patterns;
mod refilter;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, fast_arg, math::complex::mean_power, power_db};

use self::{
    decoder::{Decoder, Scratch},
    patterns::Patterns,
    refilter::Refilter,
};
use super::{
    BlePhy, CRC_BYTES, MAX_PDU_BYTES, Packet, SPS, Sink,
    coded::{AA_BLOCKS, INDICATOR_BLOCKS, S8_SYMBOLS, TERM_BITS},
    gfsk,
    track::acquire,
};
use crate::cpm::{LaurentError, laurent_main_pulse};

const ONE_SYMBOLS: usize = 40;
const ONE_ERRORS: usize = 6;
const ONE_SCREEN_ERRORS: u32 = 5;
const ONE_BALANCED: std::ops::RangeInclusive<usize> = 1..=8;
const ONE_SEGMENT: usize = 8;
const CODED_ERRORS: u32 = 12;
const CODED_SEGMENT: usize = 16;
const BLOCK_SAMPLES: usize = S8_SYMBOLS * SPS;
const AA_SYMBOLS: usize = AA_BLOCKS * S8_SYMBOLS;
const KEEP_BEHIND: usize = AA_SYMBOLS * SPS + 64;
const MAX_HISTORY: usize = 90_000;
const TRIM_SLACK: usize = 16_384;
const RETRY_SKIP: usize = 3;
const REFILTER_PAD: usize = 64;
const ANCHOR_SEARCH: usize = 2 * SPS;
const ONE_LONGEST: usize = ((MAX_PDU_BYTES + CRC_BYTES) * 8 + 8) * SPS;
const CODED_LONGEST: usize =
    (INDICATOR_BLOCKS + 2 * ((MAX_PDU_BYTES + CRC_BYTES) * 8 + TERM_BITS) + 4) * BLOCK_SAMPLES;
/// Floor of the reported packet level, so a silent window reads a number.
const LEVEL_FLOOR_DB: f32 = -200.0;

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
    ready_at: usize,
    suspect: Option<usize>,
    rejected: u32,
    patterns: Patterns,
    scratch: Scratch,
}

enum Attempt {
    Done(usize, usize, BlePhy, usize),
    Corrupt(usize),
    Wait(usize),
    Miss,
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
            ready_at: 0,
            suspect: None,
            rejected: 0,
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
        self.ready_at = 0;
        self.suspect = None;
    }

    #[must_use]
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.matched.process(iq, &mut self.filtered);
        self.extend();
        self.raw.extend_from_slice(iq);
        self.scan(sink);
    }

    fn extend(&mut self) {
        let first = self.y.len();
        self.y.extend_from_slice(&self.filtered);
        let y = &self.y;
        let warm = SPS.saturating_sub(first).min(y.len() - first);
        self.e.extend(std::iter::repeat_n(0.0, warm));
        self.angles.extend(std::iter::repeat_n(0.0, warm));
        let start = first + warm;
        let steps = y[start..]
            .iter()
            .zip(&y[start - SPS.min(start)..])
            .map(|(now, earlier)| now * earlier.conj());
        self.e.extend(steps.clone().map(|step| step.im));
        self.angles.extend(steps.map(fast_arg));
        for n in first..y.len() {
            let rising = n.checked_sub(SPS).map_or(0, |earlier| {
                self.rising[earlier] << 1 | u64::from(self.angles[n] > self.angles[earlier])
            });
            self.rising.push(rising);
            let block = n.checked_sub(BLOCK_SAMPLES).map_or(0, |earlier| {
                let e = &self.e;
                let metric = e[n - 3 * SPS] + e[n - 2 * SPS] - e[n - SPS] - e[n];
                self.blocks[earlier] << 1 | u64::from(metric > 0.0)
            });
            self.blocks.push(block);
        }
    }

    fn scan(&mut self, sink: &mut impl Sink) {
        if self.y.len() < self.ready_at {
            self.trim();
            return;
        }
        let mut at = self.cursor;
        while at < self.y.len() {
            self.settle_suspect(at);
            let patterns = &self.patterns;
            let one = ((self.rising[at] ^ patterns.one_rising) & patterns.one_turns).count_ones()
                <= ONE_SCREEN_ERRORS
                && one_sync(&self.angles, at, &patterns.one_symbols);
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
                    if self.suspect.is_some_and(|suspect| start < suspect) {
                        self.suspect = None;
                    }
                    let start = start.min(end.saturating_sub(1));
                    let window = &self.raw[start..end.min(self.raw.len())];
                    sink.packet(Packet {
                        pdu: &self.scratch.pdu[..length],
                        phy,
                        rf: self.rf,
                        level_dbfs: power_db(mean_power(window), LEVEL_FLOOR_DB),
                        sample: self.consumed + start as u64,
                    });
                    at = end.max(at + 1);
                }
                Attempt::Wait(needed) => {
                    self.ready_at = needed;
                    break;
                }
                Attempt::Miss | Attempt::Corrupt(_) => at += 1,
            }
        }
        self.cursor = at.min(self.y.len());
        self.settle_suspect(self.cursor);
        self.trim();
    }

    fn settle_suspect(&mut self, at: usize) {
        if self.suspect.is_some_and(|end| at >= end) {
            self.rejected = self.rejected.saturating_add(1);
            self.suspect = None;
        }
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
            Attempt::Wait(needed) if needed + base <= anchor + longest => {
                Attempt::Wait(needed + base)
            }
            Attempt::Corrupt(end) => {
                let end = end + base;
                self.suspect = Some(self.suspect.map_or(end, |suspect| suspect.max(end)));
                Attempt::Miss
            }
            _ => Attempt::Miss,
        }
    }

    fn trim(&mut self) {
        let behind = self.cursor.saturating_sub(KEEP_BEHIND);
        let over = self.y.len().saturating_sub(MAX_HISTORY);
        if behind < TRIM_SLACK && over == 0 {
            return;
        }
        let drop = behind.max(over);
        self.y.drain(..drop);
        self.e.drain(..drop);
        self.angles.drain(..drop);
        self.rising.drain(..drop);
        self.blocks.drain(..drop);
        self.raw.drain(..drop.min(self.raw.len()));
        self.consumed += drop as u64;
        self.cursor = self.cursor.saturating_sub(drop);
        self.ready_at = self.ready_at.saturating_sub(drop);
        self.suspect = self.suspect.map(|end| end.saturating_sub(drop));
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
