mod cck;
mod detect;
mod plcp;
mod reader;

use num_complex::Complex;

use self::{
    cck::Cck,
    detect::{DETECT_SPAN, DETECT_SYMBOLS, Matched, first_peak},
    plcp::Header,
    reader::SymbolReader,
};
use super::{Frame, MAX_MPDU_BYTES, Sink, WifiPhy, byte, fcs_ok, level_dbfs};
use crate::spread::{PnError, PnSequence};

pub use self::cck::codeword as cck_codeword;
pub(crate) use self::plcp::{LENGTH_EXTENSION, LONG_SFD, SHORT_SFD, signal_code};
pub use self::plcp::{crc16 as header_crc, psdu_bytes};
pub(crate) use self::reader::{nearest_quadrant, quadrant};

pub const SYMBOL: usize = 20;
pub const BARKER_CHIPS: usize = 11;
pub const CHIP_SAMPLES: f64 = SYMBOL as f64 / BARKER_CHIPS as f64;
const MAX_PREAMBLE_SYMBOLS: usize = 160;
const LOOKAHEAD: usize = SYMBOL * (DETECT_SYMBOLS + 2);
const MAX_HISTORY: usize = 450_000;
const MARGIN: usize = SYMBOL + 2;

pub fn barker() -> Result<[f32; BARKER_CHIPS], PnError> {
    let sequence = PnSequence::barker(BARKER_CHIPS)?;
    let mut chips = [0.0; BARKER_CHIPS];
    chips.copy_from_slice(sequence.chips());
    Ok(chips)
}

#[must_use]
pub fn template(barker: &[f32; BARKER_CHIPS]) -> [f32; SYMBOL] {
    std::array::from_fn(|k| {
        barker
            .iter()
            .enumerate()
            .map(|(chip, &value)| {
                let start = chip as f64 * CHIP_SAMPLES;
                let overlap = (start + CHIP_SAMPLES).min(k as f64 + 1.0) - start.max(k as f64);
                value * overlap.max(0.0) as f32
            })
            .sum()
    })
}

#[derive(Clone, Copy)]
enum State {
    Search,
    Pending(usize),
}

enum Outcome {
    Frame(Header, usize),
    Skip(usize),
    Short,
    Wait(usize),
    Failed,
}

pub struct Dsss {
    matched: Matched,
    cck: Cck,
    history: Vec<Complex<f32>>,
    bytes: Vec<u8>,
    restart: usize,
    consumed: u64,
    cursor: usize,
    ready_at: usize,
    state: State,
    rejected: u32,
}

impl Dsss {
    pub fn new() -> Result<Self, PnError> {
        Ok(Self {
            matched: Matched::new(&template(&barker()?), MAX_HISTORY),
            cck: Cck::new(),
            history: Vec::with_capacity(MAX_HISTORY),
            bytes: Vec::with_capacity(MAX_MPDU_BYTES),
            restart: 0,
            consumed: 0,
            cursor: 0,
            ready_at: 0,
            state: State::Search,
            rejected: 0,
        })
    }

    #[must_use]
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.matched.reset();
        self.consumed = 0;
        self.cursor = 0;
        self.ready_at = 0;
        self.state = State::Search;
        self.restart = 0;
    }

    pub fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.history.extend_from_slice(iq);
        self.matched.push(iq);
        loop {
            match self.state {
                State::Pending(_) if self.history.len() < self.ready_at => break,
                State::Pending(start) => match self.demodulate(start, sink) {
                    Outcome::Short => break,
                    Outcome::Wait(needed) => {
                        self.ready_at = needed;
                        break;
                    }
                    Outcome::Frame(header, end) => {
                        sink.frame(Frame {
                            mpdu: &self.bytes,
                            phy: header.phy,
                            level_dbfs: level_dbfs(&self.history[start..end]),
                            sample: self.consumed + start as u64,
                        });
                        self.resume(end);
                    }
                    Outcome::Skip(end) => self.resume(end),
                    Outcome::Failed => self.resume(start + SYMBOL),
                },
                State::Search => {
                    if !self.search() {
                        break;
                    }
                }
            }
        }
        self.trim();
    }

    fn resume(&mut self, at: usize) {
        self.ready_at = 0;
        self.cursor = at.max(self.cursor);
        self.state = State::Search;
        self.restart = self.cursor;
    }

    fn search(&mut self) -> bool {
        let to = (self.history.len() + 1).saturating_sub(LOOKAHEAD);
        let from = self.cursor.max(self.restart + DETECT_SPAN - 1);
        if let Some(at) = first_peak(&self.matched.rho, from, to) {
            self.cursor = at + 1;
            self.state = State::Pending(at);
            return true;
        }
        self.cursor = self.cursor.max(to);
        false
    }

    fn demodulate(&mut self, start: usize, sink: &impl Sink) -> Outcome {
        let Self {
            history,
            matched,
            cck,
            bytes,
            rejected,
            ..
        } = self;
        let mut reader = SymbolReader::new(history, &matched.values, start);
        let preamble = match plcp::find_sfd(&mut reader, MAX_PREAMBLE_SYMBOLS) {
            Ok(preamble) => preamble,
            Err(outcome) => return outcome,
        };
        let header = match plcp::header(&mut reader, preamble) {
            Ok(header) => header,
            Err(outcome) => return outcome,
        };
        if header.bytes > MAX_MPDU_BYTES || header.bytes == 0 {
            return Outcome::Failed;
        }
        let end = reader.position() + duration(&header) + MARGIN;
        if end > history.len() {
            return Outcome::Wait(end);
        }
        bytes.clear();
        let complete = match header.phy {
            WifiPhy::Cck5m5 | WifiPhy::Cck11m => cck.psdu(&mut reader, &header, bytes, sink),
            _ => reader.psdu(&header, bytes, sink),
        };
        let end = reader.position().min(history.len());
        if !complete {
            return Outcome::Skip(end);
        }
        if !fcs_ok(bytes) {
            *rejected = rejected.saturating_add(1);
            return Outcome::Skip(end);
        }
        Outcome::Frame(header, end)
    }

    fn trim(&mut self) {
        let keep = match self.state {
            State::Pending(start) => start,
            State::Search => self
                .cursor
                .min(self.restart.max(self.cursor.saturating_sub(DETECT_SPAN))),
        };
        let drop = keep
            .max(self.history.len().saturating_sub(MAX_HISTORY))
            .min(self.matched.len());
        if drop == 0 {
            return;
        }
        self.history.drain(..drop);
        self.matched.drain(drop);
        self.consumed += drop as u64;
        self.cursor = self.cursor.saturating_sub(drop);
        self.restart = self.restart.saturating_sub(drop);
        self.ready_at = self.ready_at.saturating_sub(drop);
        if let State::Pending(start) = &mut self.state {
            *start = start.saturating_sub(drop);
        }
    }
}

fn duration(header: &Header) -> usize {
    let bits = header.bytes * 8;
    match header.phy {
        WifiPhy::Dsss2m => bits.div_ceil(2) * SYMBOL,
        WifiPhy::Cck5m5 | WifiPhy::Cck11m => cck::duration(header.phy, header.bytes),
        _ => bits * SYMBOL,
    }
}

#[cfg(test)]
mod tests;
