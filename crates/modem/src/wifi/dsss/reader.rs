use num_complex::Complex;
use sdrmm_dsp::Descrambler;

use super::{SYMBOL, WifiPhy, byte, offset_hz, plcp::Header};
use crate::{
    linear::PhaseTracker,
    wifi::{Sink, points::Cloud, scrambler::SCRAMBLER_TAPS},
};

const TIMING_GAIN: f64 = 0.1;
const PHASE_GAIN: f32 = 0.05;
const FREQ_GAIN: f32 = 0.002;
const PHASE_SYMBOLS: usize = 32;
const PHASE_SEARCH: usize = 4;
/// The furthest a symbol start moves toward the correlation peak in one step, in samples.
const MAX_TIMING_ERROR: f64 = 1.0;

pub(crate) struct SymbolReader<'a> {
    history: &'a [Complex<f32>],
    matched: &'a [Complex<f32>],
    position: f64,
    previous: Complex<f32>,
    rotation: Complex<f32>,
    tracker: PhaseTracker,
    point: Complex<f32>,
    derotated: Complex<f32>,
    descrambler: Descrambler,
}

impl<'a> SymbolReader<'a> {
    pub(super) fn new(
        history: &'a [Complex<f32>],
        matched: &'a [Complex<f32>],
        start: usize,
    ) -> Self {
        let mut reader = Self {
            history,
            matched,
            position: start as f64,
            previous: Complex::new(0.0, 0.0),
            rotation: Complex::new(1.0, 0.0),
            tracker: PhaseTracker::new(PHASE_GAIN, FREQ_GAIN),
            point: Complex::new(1.0, 0.0),
            derotated: Complex::new(0.0, 0.0),
            descrambler: Descrambler::new(&SCRAMBLER_TAPS),
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
            let error = (peak - self.position).clamp(-MAX_TIMING_ERROR, MAX_TIMING_ERROR);
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
        self.derotated = derotated;
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
        self.descrambler.push(bit)
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

    pub(super) fn psdu(
        &mut self,
        header: &Header,
        bytes: &mut Vec<u8>,
        cloud: &mut Cloud,
        sink: &impl Sink,
    ) -> bool {
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
            cloud.push(self.derotated);
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
        cloud.heard_at(offset_hz(&self.tracker));
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
