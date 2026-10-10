mod cck;
mod plcp;

use num_complex::Complex;

use self::{cck::Cck, plcp::Header};
use super::{Frame, MAX_MPDU_BYTES, Sink, WifiPhy, byte, fcs_ok, level_dbfs};
use crate::spread::{PnError, PnSequence};

pub use self::cck::codeword as cck_codeword;
pub use self::plcp::{crc16 as header_crc, psdu_bytes};

pub const SYMBOL: usize = 20;
pub const BARKER_CHIPS: usize = 11;
pub const CHIP_SAMPLES: f64 = SYMBOL as f64 / BARKER_CHIPS as f64;
const DETECT_SYMBOLS: usize = 8;
const DETECT_THRESHOLD: f32 = 0.2;
const MIN_POWER: f32 = 1e-9;
const TIMING_GAIN: f64 = 0.1;
const PHASE_SYMBOLS: usize = 32;
const PHASE_SEARCH: usize = 4;
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

fn correlate(history: &[Complex<f32>], at: usize, template: &[f32; SYMBOL]) -> Complex<f32> {
    history[at..at + SYMBOL]
        .iter()
        .zip(template)
        .map(|(&x, &b)| x * b)
        .sum()
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
    template: [f32; SYMBOL],
    cck: Cck,
    history: Vec<Complex<f32>>,
    bytes: Vec<u8>,
    energy: f32,
    rings: [[f32; DETECT_SYMBOLS]; SYMBOL],
    sums: [f32; SYMBOL],
    filled: usize,
    consumed: u64,
    cursor: usize,
    ready_at: usize,
    state: State,
    rejected: u32,
}

impl Dsss {
    pub fn new() -> Result<Self, PnError> {
        Ok(Self {
            template: template(&barker()?),
            cck: Cck::new(),
            history: Vec::with_capacity(MAX_HISTORY),
            bytes: Vec::with_capacity(MAX_MPDU_BYTES),
            energy: 0.0,
            rings: [[0.0; DETECT_SYMBOLS]; SYMBOL],
            sums: [0.0; SYMBOL],
            filled: 0,
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
        self.consumed = 0;
        self.cursor = 0;
        self.ready_at = 0;
        self.state = State::Search;
        self.restart_detector();
    }

    fn restart_detector(&mut self) {
        self.energy = 0.0;
        self.rings = [[0.0; DETECT_SYMBOLS]; SYMBOL];
        self.sums = [0.0; SYMBOL];
        self.filled = 0;
    }

    pub fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.history.extend_from_slice(iq);
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
        self.restart_detector();
    }

    fn search(&mut self) -> bool {
        while self.cursor + LOOKAHEAD <= self.history.len() {
            let at = self.cursor;
            if self.filled == 0 {
                self.energy = self.history[at..at + SYMBOL]
                    .iter()
                    .map(|x| x.norm_sqr())
                    .sum();
            } else {
                self.energy +=
                    self.history[at + SYMBOL - 1].norm_sqr() - self.history[at - 1].norm_sqr();
            }
            let rho = if self.energy > MIN_POWER {
                correlate(&self.history, at, &self.template).norm_sqr()
                    / (self.energy * SYMBOL as f32)
            } else {
                0.0
            };
            let phase = at % SYMBOL;
            let slot = (at / SYMBOL) % DETECT_SYMBOLS;
            self.sums[phase] += rho - self.rings[phase][slot];
            self.rings[phase][slot] = rho;
            self.filled += 1;
            self.cursor += 1;
            if self.filled >= SYMBOL * DETECT_SYMBOLS
                && self.sums[phase] / DETECT_SYMBOLS as f32 > DETECT_THRESHOLD
                && self.is_peak(phase)
            {
                self.state = State::Pending(at);
                return true;
            }
        }
        false
    }

    fn is_peak(&self, phase: usize) -> bool {
        let next = self.sums[(phase + 1) % SYMBOL];
        let previous = self.sums[(phase + SYMBOL - 1) % SYMBOL];
        self.sums[phase] >= previous && self.sums[phase] >= next
    }

    fn demodulate(&mut self, start: usize, sink: &impl Sink) -> Outcome {
        let Self {
            history,
            template,
            cck,
            bytes,
            rejected,
            ..
        } = self;
        let mut reader = SymbolReader::new(history, start, template);
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
            State::Search => self.cursor.saturating_sub(1),
        };
        let drop = keep.max(self.history.len().saturating_sub(MAX_HISTORY));
        if drop == 0 {
            return;
        }
        self.history.drain(..drop);
        self.consumed += drop as u64;
        self.cursor = self.cursor.saturating_sub(drop);
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

pub(crate) struct SymbolReader<'a> {
    history: &'a [Complex<f32>],
    template: &'a [f32; SYMBOL],
    position: f64,
    previous: Complex<f32>,
    rotation: Complex<f32>,
    descrambler: u8,
}

impl<'a> SymbolReader<'a> {
    fn new(history: &'a [Complex<f32>], start: usize, template: &'a [f32; SYMBOL]) -> Self {
        let mut reader = Self {
            history,
            template,
            position: start as f64,
            previous: Complex::new(0.0, 0.0),
            rotation: Complex::new(1.0, 0.0),
            descrambler: 0,
        };
        reader.position = reader.best_phase(start) as f64;
        reader.previous = reader.at(reader.position).unwrap_or_default();
        reader.position += SYMBOL as f64;
        reader
    }

    fn best_phase(&self, start: usize) -> usize {
        let energy = |at: usize| -> f32 {
            (0..PHASE_SYMBOLS)
                .filter_map(|symbol| self.correlation(at + symbol * SYMBOL))
                .map(|c| c.norm())
                .sum()
        };
        (start.saturating_sub(PHASE_SEARCH)..=start + PHASE_SEARCH)
            .max_by(|&a, &b| energy(a).total_cmp(&energy(b)))
            .unwrap_or(start)
    }

    pub(crate) fn position(&self) -> usize {
        self.position.round() as usize
    }

    pub(crate) fn history(&self) -> &'a [Complex<f32>] {
        self.history
    }

    fn correlation(&self, at: usize) -> Option<Complex<f32>> {
        (at >= 1 && at + SYMBOL < self.history.len())
            .then(|| correlate(self.history, at, self.template))
    }

    fn at(&self, position: f64) -> Option<Complex<f32>> {
        let index = position.floor();
        let weight = (position - index) as f32;
        let index = index as usize;
        Some(self.correlation(index)? * (1.0 - weight) + self.correlation(index + 1)? * weight)
    }

    fn next(&mut self) -> Option<Complex<f32>> {
        let centre = self.at(self.position)?;
        let nearest = self.position.round() as usize;
        let early = self.correlation(nearest.checked_sub(1)?)?.norm();
        let middle = self.correlation(nearest)?.norm();
        let late = self.correlation(nearest + 1)?.norm();
        let curvature = early - 2.0 * middle + late;
        if curvature < 0.0 {
            let peak = nearest as f64 + f64::from(0.5 * (early - late) / curvature);
            let error = (peak - self.position).clamp(-1.0, 1.0);
            self.position += TIMING_GAIN * error;
        }
        let differential = centre * self.previous.conj() * self.rotation;
        self.previous = centre;
        self.position += SYMBOL as f64;
        Some(differential)
    }

    pub(crate) fn train(&mut self, differentials: &[Complex<f32>]) {
        let squared: Complex<f32> = differentials.iter().map(|d| d * d).sum();
        let angle = squared.arg() / 2.0;
        self.rotation = Complex::from_polar(1.0, -angle);
    }

    pub(crate) fn raw(&mut self) -> Option<Complex<f32>> {
        self.next()
    }

    pub(crate) fn descrambled(&mut self, bit: bool) -> bool {
        let out = bit ^ (self.descrambler >> 3 & 1 == 1) ^ (self.descrambler >> 6 & 1 == 1);
        self.descrambler = (self.descrambler << 1 | u8::from(bit)) & 0x7F;
        out
    }

    pub(crate) fn dbpsk(&mut self) -> Option<bool> {
        let differential = self.next()?;
        Some(self.descrambled(differential.re < 0.0))
    }

    pub(crate) fn dqpsk(&mut self) -> Option<[bool; 2]> {
        let differential = self.next()?;
        let dibit = quadrant(differential);
        Some([self.descrambled(dibit[0]), self.descrambled(dibit[1])])
    }

    pub(crate) fn chip_start(&self) -> f64 {
        self.position
    }

    pub(crate) fn last_phase(&self) -> Complex<f32> {
        self.previous
    }

    pub(crate) fn rotation(&self) -> Complex<f32> {
        self.rotation
    }

    pub(crate) fn advance_to(&mut self, position: f64) {
        self.position = position;
    }

    fn psdu(&mut self, header: &Header, bytes: &mut Vec<u8>, sink: &impl Sink) -> bool {
        let mut bits = [false; 8];
        let mut filled = 0;
        while bytes.len() < header.bytes {
            let decided = match header.phy {
                WifiPhy::Dsss2m => self.dqpsk().map(|pair| (pair, 2)),
                _ => self.dbpsk().map(|bit| ([bit, false], 1)),
            };
            let Some((pair, count)) = decided else {
                return false;
            };
            for &bit in &pair[..count] {
                bits[filled] = bit;
                filled += 1;
                if filled == 8 {
                    bytes.push(byte(&bits));
                    filled = 0;
                    if bytes.len() == 1 && !sink.accepts(bytes[0]) {
                        self.advance_to(self.position + skipped(header, bytes.len()));
                        return false;
                    }
                }
            }
        }
        true
    }
}

fn skipped(header: &Header, read: usize) -> f64 {
    let bits_left = header.bytes.saturating_sub(read) * 8;
    let per_symbol = if header.phy == WifiPhy::Dsss2m { 2 } else { 1 };
    (bits_left.div_ceil(per_symbol) * SYMBOL) as f64
}

pub(crate) fn quadrant(differential: Complex<f32>) -> [bool; 2] {
    let rotated = differential * Complex::from_polar(1.0, std::f32::consts::FRAC_PI_4);
    match (rotated.re >= 0.0, rotated.im >= 0.0) {
        (true, true) => [false, false],
        (false, true) => [false, true],
        (false, false) => [true, true],
        (true, false) => [true, false],
    }
}

#[cfg(test)]
mod tests;
