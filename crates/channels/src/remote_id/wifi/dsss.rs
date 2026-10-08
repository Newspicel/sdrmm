mod cck;
mod plcp;

use num_complex::Complex;
use sdrmm_wire::RemoteIdPhy;

use self::plcp::Header;
use super::receiver::{Burst, level_dbfs};

pub(crate) const SYMBOL: usize = 20;
pub(crate) const BARKER: [f32; 11] = [1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0];
pub(crate) const CHIP_SAMPLES: f64 = SYMBOL as f64 / 11.0;
const DETECT_SYMBOLS: usize = 8;
const DETECT_THRESHOLD: f32 = 0.2;
const MIN_POWER: f32 = 1e-9;
const TIMING_GAIN: f64 = 0.1;
const PHASE_SYMBOLS: usize = 32;
const PHASE_SEARCH: usize = 4;
const MAX_PREAMBLE_SYMBOLS: usize = 160;
const MAX_PSDU_BYTES: usize = 2_400;
const LOOKAHEAD: usize = SYMBOL * (DETECT_SYMBOLS + 2);
const MAX_HISTORY: usize = 450_000;

pub(crate) fn template() -> [f32; SYMBOL] {
    std::array::from_fn(|k| {
        BARKER
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

pub(crate) fn correlate(
    history: &[Complex<f32>],
    at: usize,
    template: &[f32; SYMBOL],
) -> Complex<f32> {
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

pub(crate) struct Dsss {
    template: [f32; SYMBOL],
    history: Vec<Complex<f32>>,
    energy: f32,
    rings: [[f32; DETECT_SYMBOLS]; SYMBOL],
    sums: [f32; SYMBOL],
    filled: usize,
    consumed: u64,
    cursor: usize,
    state: State,
    pub(crate) rejected: u32,
}

pub(crate) enum Outcome {
    Burst(Burst, usize),
    Skip(usize),
    Short,
    Failed,
}

impl Dsss {
    pub(crate) fn new() -> Self {
        Self {
            template: template(),
            history: Vec::new(),
            energy: 0.0,
            rings: [[0.0; DETECT_SYMBOLS]; SYMBOL],
            sums: [0.0; SYMBOL],
            filled: 0,
            consumed: 0,
            cursor: 0,
            state: State::Search,
            rejected: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.history.clear();
        self.consumed = 0;
        self.cursor = 0;
        self.state = State::Search;
        self.restart_detector();
    }

    fn restart_detector(&mut self) {
        self.energy = 0.0;
        self.rings = [[0.0; DETECT_SYMBOLS]; SYMBOL];
        self.sums = [0.0; SYMBOL];
        self.filled = 0;
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Burst>) {
        self.history.extend_from_slice(iq);
        loop {
            match self.state {
                State::Pending(start) => match self.demodulate(start) {
                    Outcome::Short => break,
                    Outcome::Burst(burst, end) => {
                        out.push(burst);
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

    fn demodulate(&mut self, start: usize) -> Outcome {
        let mut reader = SymbolReader::new(&self.history, start, &self.template);
        let preamble = match plcp::find_sfd(&mut reader, MAX_PREAMBLE_SYMBOLS) {
            Ok(preamble) => preamble,
            Err(outcome) => return outcome,
        };
        let header = match plcp::header(&mut reader, preamble) {
            Ok(header) => header,
            Err(outcome) => return outcome,
        };
        if header.bytes > MAX_PSDU_BYTES {
            return Outcome::Failed;
        }
        let bytes = match header.phy {
            RemoteIdPhy::Cck5m5 | RemoteIdPhy::Cck11m => cck::psdu(&mut reader, &header),
            _ => reader.psdu(&header),
        };
        let Some(bytes) = bytes else {
            return Outcome::Short;
        };
        let end = reader.position();
        if !super::fcs_ok(&bytes) {
            if super::wanted(&bytes) {
                self.rejected = self.rejected.saturating_add(1);
            }
            return Outcome::Skip(end);
        }
        Outcome::Burst(
            Burst {
                bytes,
                phy: header.phy,
                level_dbfs: level_dbfs(&self.history[start..end.min(self.history.len())]),
                sample: self.consumed + start as u64,
            },
            end,
        )
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
        if let State::Pending(start) = &mut self.state {
            *start = start.saturating_sub(drop);
        }
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

    fn descramble(&mut self, bit: bool) -> bool {
        let out = bit ^ (self.descrambler >> 3 & 1 == 1) ^ (self.descrambler >> 6 & 1 == 1);
        self.descrambler = (self.descrambler << 1 | u8::from(bit)) & 0x7F;
        out
    }

    pub(crate) fn dbpsk(&mut self) -> Option<bool> {
        let differential = self.next()?;
        Some(self.descramble(differential.re < 0.0))
    }

    pub(crate) fn dqpsk(&mut self) -> Option<[bool; 2]> {
        let differential = self.next()?;
        let dibit = quadrant(differential);
        Some([self.descramble(dibit[0]), self.descramble(dibit[1])])
    }

    pub(crate) fn descrambled(&mut self, bit: bool) -> bool {
        self.descramble(bit)
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

    fn psdu(&mut self, header: &Header) -> Option<Vec<u8>> {
        let mut bytes = Vec::with_capacity(header.bytes);
        let mut bits = Vec::with_capacity(8);
        while bytes.len() < header.bytes {
            match header.phy {
                RemoteIdPhy::Dsss2m => bits.extend(self.dqpsk()?),
                _ => bits.push(self.dbpsk()?),
            }
            while bits.len() >= 8 {
                bytes.push(
                    bits.drain(..8)
                        .enumerate()
                        .fold(0u8, |acc, (bit, value)| acc | u8::from(value) << bit),
                );
            }
        }
        Some(bytes)
    }
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn cck_codeword(phy: RemoteIdPhy, phi1: f32, label: u8) -> [Complex<f32>; cck::CHIPS] {
    cck::codeword(phi1, cck::phases(phy, label))
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
