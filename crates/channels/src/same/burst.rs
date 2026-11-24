use crate::same;
use crate::same::SAME_BURSTS;
use crate::same::SameMessage;
use sdrmm_dsp::SyncDetector;

use super::RATE;

pub(crate) const PREAMBLE_BYTE: u8 = 0xAB;
#[cfg(any(test, feature = "synth"))]
pub(crate) const PREAMBLE_BYTES: usize = 16;
const SYNC_BYTES: u32 = 4;
const SYNC_TOLERANCE: u32 = 2;
const WINDOW_SAMPLES: u64 = 20 * RATE as u64;
const MIN_COPIES: usize = 2;

const fn preamble_word(bytes: u32) -> u64 {
    let mut word = 0;
    let mut bit = 0;
    while bit < bytes * 8 {
        word = (word << 1) | ((PREAMBLE_BYTE >> (bit % 8)) & 1) as u64;
        bit += 1;
    }
    word
}

const HEADER_START: [u8; 2] = *b"ZN";
const MAX_BITS: usize = same::MAX_HEADER_LEN * 8;

/// A burst's header as heard: one soft value a bit, sent first first and positive for a one,
/// and its text when the bits alone already read as a header.
pub(crate) struct Burst {
    pub(crate) soft: Vec<f32>,
    pub(crate) text: Option<String>,
}

enum Step {
    More,
    Abandon,
    Done(Burst),
}

#[derive(Default)]
struct Collect {
    byte: [f32; 8],
    index: usize,
    started: bool,
    soft: Vec<f32>,
    text: Option<String>,
}

impl Collect {
    fn push(&mut self, soft: f32) -> Step {
        self.byte[self.index] = soft;
        self.index += 1;
        if self.index < 8 {
            return Step::More;
        }
        self.index = 0;
        let byte = hard_byte(&self.byte);
        if !self.started {
            if HEADER_START.iter().any(|&start| near(byte, start, 1)) {
                self.started = true;
                self.text = Some(String::new());
            } else if !near(byte, PREAMBLE_BYTE, SYNC_TOLERANCE) {
                return Step::Abandon;
            } else {
                return Step::More;
            }
        }
        self.soft.extend_from_slice(&self.byte);
        if let Some(text) = &mut self.text {
            if carries(byte, text.is_empty()) {
                text.push(char::from(byte));
            } else {
                self.text = None;
            }
        }
        if self
            .text
            .as_deref()
            .is_some_and(|text| same::parse(text).is_some())
        {
            return Step::Done(self.finish());
        }
        if self.soft.len() >= MAX_BITS {
            self.text = None;
            return Step::Done(self.finish());
        }
        Step::More
    }

    fn finish(&mut self) -> Burst {
        Burst {
            soft: std::mem::take(&mut self.soft),
            text: self.text.take(),
        }
    }

    /// The header so far, when a new sync cuts it short.
    fn cut(mut self) -> Option<Burst> {
        self.text = None;
        self.started.then(|| self.finish())
    }
}

pub(crate) struct Framer {
    sync: SyncDetector,
    collect: Option<Collect>,
}

impl Default for Framer {
    fn default() -> Self {
        Self {
            sync: SyncDetector::new(preamble_word(SYNC_BYTES), SYNC_BYTES * 8, SYNC_TOLERANCE),
            collect: None,
        }
    }
}

impl Framer {
    /// Takes one soft bit, positive for a one.
    pub(crate) fn push(&mut self, soft: f32) -> Option<Burst> {
        if self.sync.push(soft > 0.0) {
            return self
                .collect
                .replace(Collect::default())
                .and_then(Collect::cut);
        }
        let step = self.collect.as_mut()?.push(soft);
        match step {
            Step::More => None,
            Step::Abandon => {
                self.collect = None;
                None
            }
            Step::Done(burst) => {
                self.collect = None;
                Some(burst)
            }
        }
    }
}

fn hard_byte(bits: &[f32; 8]) -> u8 {
    (0..8).fold(0, |byte, bit| byte | u8::from(bits[bit] > 0.0) << bit)
}

fn near(byte: u8, to: u8, tolerance: u32) -> bool {
    (byte ^ to).count_ones() <= tolerance
}

fn carries(byte: u8, first: bool) -> bool {
    if first {
        HEADER_START.contains(&byte)
    } else {
        byte.is_ascii_graphic() || byte == b' '
    }
}

/// The header the soft bits read as, byte by byte until it parses.
fn read(soft: &[f32]) -> Option<String> {
    let mut text = String::new();
    for bits in soft.as_chunks::<8>().0 {
        let byte = hard_byte(bits);
        if !carries(byte, text.is_empty()) {
            return None;
        }
        text.push(char::from(byte));
        if same::parse(&text).is_some() {
            return Some(text);
        }
    }
    None
}

/// The bursts' soft bits summed place by place, a short burst adding nothing past its end.
fn combine<'a>(bursts: impl Iterator<Item = &'a [f32]> + Clone) -> Vec<f32> {
    let len = bursts.clone().map(<[f32]>::len).max().unwrap_or(0);
    let mut sum = vec![0.0; len];
    for soft in bursts {
        for (total, value) in sum.iter_mut().zip(soft) {
            *total += value;
        }
    }
    sum
}

struct Seen {
    at: u64,
    burst: Burst,
}

struct Settled {
    at: u64,
    text: String,
}

#[derive(Default)]
pub(crate) struct Bursts {
    seen: Vec<Seen>,
    settled: Vec<Settled>,
    ready: Vec<SameMessage>,
    rejected: u32,
}

impl Bursts {
    pub(crate) fn received(&mut self, at: u64, burst: Burst) {
        self.expire(at);
        let settled = |text: &String| self.settled.iter().any(|seen| &seen.text == text);
        if burst.text.as_ref().is_some_and(settled) {
            return;
        }
        self.seen.push(Seen { at, burst });
        self.settle(at);
    }

    /// Forgets bursts older than the window; one that read as a header alone counts as rejected.
    pub(crate) fn expire(&mut self, at: u64) {
        let stale = |at_seen: u64| at.saturating_sub(at_seen) > WINDOW_SAMPLES;
        self.rejected += self
            .seen
            .iter()
            .filter(|seen| stale(seen.at) && seen.burst.text.is_some())
            .count() as u32;
        self.seen.retain(|seen| !stale(seen.at));
        self.settled.retain(|seen| !stale(seen.at));
    }

    pub(crate) fn take(&mut self) -> Vec<SameMessage> {
        std::mem::take(&mut self.ready)
    }

    pub(crate) fn rejected(&self) -> u32 {
        self.rejected
    }

    /// Two bursts that read the same settle the header; otherwise all three bursts' soft bits
    /// are summed and read once.
    fn settle(&mut self, at: u64) {
        if let Some(text) = self.seen.last().and_then(|seen| seen.burst.text.clone()) {
            let copies = self
                .seen
                .iter()
                .filter(|seen| seen.burst.text.as_ref() == Some(&text))
                .count();
            if copies >= MIN_COPIES {
                self.emit(at, text, copies);
                return;
            }
        }
        if self.seen.len() < usize::from(SAME_BURSTS) {
            return;
        }
        let combined = combine(self.seen.iter().map(|seen| seen.burst.soft.as_slice()));
        self.seen.clear();
        match read(&combined) {
            Some(text) => self.emit(at, text, usize::from(SAME_BURSTS)),
            None => self.rejected += 1,
        }
    }

    fn emit(&mut self, at: u64, text: String, copies: usize) {
        let Some(body) = same::parse(&text) else {
            self.rejected += 1;
            return;
        };
        self.seen
            .retain(|seen| seen.burst.text.as_ref() != Some(&text));
        self.ready.push(SameMessage {
            body,
            copies: u8::try_from(copies).unwrap_or(SAME_BURSTS),
            raw: text.clone(),
        });
        self.settled.push(Settled { at, text });
    }
}

#[cfg(test)]
mod tests;
