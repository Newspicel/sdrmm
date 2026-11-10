mod cck;
mod detect;
mod plcp;

use num_complex::Complex;

use self::{
    cck::Cck,
    detect::{DETECT_SPAN, DETECT_SYMBOLS, Matched, first_peak},
    plcp::Header,
};
use super::{Frame, MAX_MPDU_BYTES, Sink, WifiPhy, byte, fcs_ok, level_dbfs};
use crate::{
    linear::PhaseTracker,
    spread::{PnError, PnSequence},
};

pub use self::cck::codeword as cck_codeword;
pub use self::plcp::{crc16 as header_crc, psdu_bytes};

pub const SYMBOL: usize = 20;
pub const BARKER_CHIPS: usize = 11;
pub const CHIP_SAMPLES: f64 = SYMBOL as f64 / BARKER_CHIPS as f64;
const TIMING_GAIN: f64 = 0.1;
const PHASE_GAIN: f32 = 0.05;
const FREQ_GAIN: f32 = 0.002;
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

pub(crate) struct SymbolReader<'a> {
    history: &'a [Complex<f32>],
    matched: &'a [Complex<f32>],
    position: f64,
    previous: Complex<f32>,
    rotation: Complex<f32>,
    tracker: PhaseTracker,
    point: Complex<f32>,
    descrambler: u8,
}

impl<'a> SymbolReader<'a> {
    fn new(history: &'a [Complex<f32>], matched: &'a [Complex<f32>], start: usize) -> Self {
        let mut reader = Self {
            history,
            matched,
            position: start as f64,
            previous: Complex::new(0.0, 0.0),
            rotation: Complex::new(1.0, 0.0),
            tracker: PhaseTracker::new(PHASE_GAIN, FREQ_GAIN),
            point: Complex::new(1.0, 0.0),
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
                .map(|c| c.norm_sqr().sqrt())
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
        (at >= 1 && at + 1 < self.matched.len()).then(|| self.matched[at])
    }

    fn at(&self, position: f64) -> Option<Complex<f32>> {
        let index = position as usize;
        let weight = (position - index as f64) as f32;
        Some(self.correlation(index)? * (1.0 - weight) + self.correlation(index + 1)? * weight)
    }

    fn next(&mut self) -> Option<Complex<f32>> {
        let centre = self.locate()?;
        let differential = centre * self.previous.conj() * self.rotation;
        self.previous = centre;
        Some(differential)
    }

    /// The correlation at the tracked symbol start, then the start moved on by one symbol and
    /// nudged toward the correlation peak.
    fn locate(&mut self) -> Option<Complex<f32>> {
        let centre = self.at(self.position)?;
        let nearest = self.position.round() as usize;
        let early = self.correlation(nearest.checked_sub(1)?)?.norm_sqr().sqrt();
        let middle = self.correlation(nearest)?.norm_sqr().sqrt();
        let late = self.correlation(nearest + 1)?.norm_sqr().sqrt();
        let curvature = early - 2.0 * middle + late;
        if curvature < 0.0 {
            let peak = nearest as f64 + f64::from(0.5 * (early - late) / curvature);
            let error = (peak - self.position).clamp(-1.0, 1.0);
            self.position += TIMING_GAIN * error;
        }
        self.position += SYMBOL as f64;
        Some(centre)
    }

    /// The next symbol against the tracked carrier, and the point decided for it among
    /// `points`. The tracker then steers by the decision.
    fn coherent(&mut self, points: Points) -> Option<Complex<f32>> {
        let centre = self.locate()?;
        self.tracker.advance(SYMBOL as f32);
        let derotated = centre * self.tracker.derotator(0.0);
        let decided = points.nearest(derotated);
        self.tracker.steer(derotated, decided, SYMBOL as f32);
        let step = decided * self.point.conj();
        self.point = decided;
        self.previous = centre;
        Some(step)
    }

    /// Estimates the carrier from the training differentials and starts the tracker on the
    /// last symbol read.
    pub(crate) fn train(&mut self, differentials: &[Complex<f32>]) {
        let squared: Complex<f32> = differentials.iter().map(|d| d * d).sum();
        let angle = squared.arg() / 2.0;
        self.rotation = Complex::from_polar(1.0, -angle);
        self.tracker
            .start(self.previous.arg(), angle / SYMBOL as f32);
        self.point = Complex::new(1.0, 0.0);
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
        let step = self.coherent(Points::Binary)?;
        Some(self.descrambled(step.re < 0.0))
    }

    pub(crate) fn dqpsk(&mut self) -> Option<[bool; 2]> {
        let step = self.coherent(Points::Quadrature)?;
        let dibit = quadrant(step);
        Some([self.descrambled(dibit[0]), self.descrambled(dibit[1])])
    }

    pub(crate) fn chip_start(&self) -> f64 {
        self.position
    }

    /// The carrier tracker and the point last decided, both at the centre of the last symbol.
    pub(crate) fn carrier(&self) -> (PhaseTracker, Complex<f32>) {
        (self.tracker, self.point)
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

#[derive(Clone, Copy)]
enum Points {
    Binary,
    Quadrature,
}

impl Points {
    fn nearest(self, z: Complex<f32>) -> Complex<f32> {
        match self {
            Self::Binary => Complex::new(if z.re < 0.0 { -1.0 } else { 1.0 }, 0.0),
            Self::Quadrature => nearest_quadrant(z),
        }
    }
}

pub(crate) fn nearest_quadrant(z: Complex<f32>) -> Complex<f32> {
    if z.re.abs() >= z.im.abs() {
        Complex::new(z.re.signum(), 0.0)
    } else {
        Complex::new(0.0, z.im.signum())
    }
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
