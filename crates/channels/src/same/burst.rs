use sdrmm_dsp::SyncDetector;
use sdrmm_wire::{SAME_BURSTS, SameMessage, same};

use super::RATE;

pub(crate) const PREAMBLE_BYTE: u8 = 0xAB;
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

#[derive(Default)]
struct Collect {
    index: u32,
    byte: u8,
    text: String,
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
    pub(crate) fn push(&mut self, bit: bool) -> Option<String> {
        let synced = self.sync.push(bit);
        let burst = self.collect(bit);
        if burst.is_none() && synced && self.collect.is_none() {
            self.collect = Some(Collect::default());
        }
        burst
    }

    fn collect(&mut self, bit: bool) -> Option<String> {
        let state = self.collect.as_mut()?;
        state.byte |= u8::from(bit) << state.index;
        state.index += 1;
        if state.index < 8 {
            return None;
        }
        let byte = std::mem::take(&mut state.byte);
        state.index = 0;
        if !carries(byte, state.text.is_empty()) {
            self.collect = None;
            return None;
        }
        state.text.push(char::from(byte));
        if same::parse(&state.text).is_some() {
            let text = std::mem::take(&mut state.text);
            self.collect = None;
            return Some(text);
        }
        if state.text.len() >= same::MAX_HEADER_LEN {
            self.collect = None;
        }
        None
    }
}

fn carries(byte: u8, first: bool) -> bool {
    if first {
        byte == b'Z' || byte == b'N'
    } else {
        byte.is_ascii_graphic() || byte == b' '
    }
}

struct Seen {
    at: u64,
    text: String,
}

#[derive(Default)]
pub(crate) struct Bursts {
    seen: Vec<Seen>,
    settled: Vec<Seen>,
    ready: Vec<SameMessage>,
    rejected: u32,
}

impl Bursts {
    pub(crate) fn received(&mut self, at: u64, text: String) {
        self.expire(at);
        if self.settled.iter().any(|seen| seen.text == text) {
            return;
        }
        self.seen.push(Seen { at, text });
        self.settle(at);
    }

    pub(crate) fn expire(&mut self, at: u64) {
        let stale = |seen: &Seen| at.saturating_sub(seen.at) > WINDOW_SAMPLES;
        self.rejected += self.seen.iter().filter(|seen| stale(seen)).count() as u32;
        self.seen.retain(|seen| !stale(seen));
        self.settled.retain(|seen| !stale(seen));
    }

    pub(crate) fn take(&mut self) -> Vec<SameMessage> {
        std::mem::take(&mut self.ready)
    }

    pub(crate) fn rejected(&self) -> u32 {
        self.rejected
    }

    fn settle(&mut self, at: u64) {
        let Some(newest) = self.seen.last().map(|seen| seen.text.clone()) else {
            return;
        };
        let copies = self.seen.iter().filter(|seen| seen.text == newest).count();
        if copies >= MIN_COPIES {
            self.emit(at, newest, copies);
            return;
        }
        if self.seen.len() < usize::from(SAME_BURSTS) {
            return;
        }
        let texts = self.seen.iter().map(|seen| seen.text.as_str());
        let voted = majority(texts).filter(|text| same::parse(text).is_some());
        self.seen.clear();
        match voted {
            Some(text) => self.emit(at, text, usize::from(SAME_BURSTS)),
            None => self.rejected += 1,
        }
    }

    fn emit(&mut self, at: u64, text: String, copies: usize) {
        let Some(body) = same::parse(&text) else {
            self.rejected += 1;
            return;
        };
        self.seen.retain(|seen| seen.text != text);
        self.ready.push(SameMessage {
            body,
            copies: u8::try_from(copies).unwrap_or(SAME_BURSTS),
            raw: text.clone(),
        });
        self.settled.push(Seen { at, text });
    }
}

fn majority<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let texts = texts.into_iter().collect::<Vec<_>>();
    let len = texts.first()?.len();
    if texts.iter().any(|text| text.len() != len) {
        return None;
    }
    (0..len)
        .map(|at| {
            let bytes = texts
                .iter()
                .filter_map(|text| text.as_bytes().get(at).copied())
                .collect::<Vec<_>>();
            bytes
                .iter()
                .copied()
                .find(|byte| bytes.iter().filter(|other| *other == byte).count() >= MIN_COPIES)
                .map(char::from)
        })
        .collect()
}

#[cfg(test)]
mod tests;
