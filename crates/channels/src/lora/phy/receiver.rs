use crate::lora::{LoraImplicitHeader, LoraIntegrity};
use num_complex::Complex;

use super::{
    Frame,
    align::{self, Candidate},
    chirp::{Dechirper, Slope, wrap},
    detect::{Accumulator, DETECT_WINDOWS, DownHistory, Levels, STEADY_WINDOWS, Sighting},
    filter::Extractor,
    payload::{Lock, MAX_COLUMNS, max_symbols},
    ring::Front,
    soft::{Demapper, MAX_BITS},
};

const MAX_UPCHIRPS: u32 = 256;
/// Windows without preamble energy after which the SFD must have passed: the partial window,
/// both sync symbols and the two and a quarter downchirps.
const QUIET_WINDOWS: u32 = 5;
/// Quiet windows after which the preamble cannot resume: both sync symbols have passed, so the
/// bin lighting up again is a payload symbol or noise.
const PREAMBLE_OVER: u32 = 2;
const MARGIN: f32 = 4.0;

#[must_use]
pub(crate) fn threshold(chips: usize) -> f32 {
    let ln = (chips as f32).ln();
    ln + ln.ln() + MARGIN
}

struct Preamble {
    bin: usize,
    previous: Complex<f32>,
    previous_start: u64,
    progression: Complex<f64>,
    interpolated: f64,
    upchirps: u32,
    quiet: u32,
    downchirp: Option<Sighting>,
}

impl Preamble {
    fn up_bins(&self) -> f64 {
        if self.progression.norm() == 0.0 {
            return self.bin as f64 + self.interpolated;
        }
        let fraction = self.progression.arg() / std::f64::consts::TAU;
        self.bin as f64 + 0.5 + wrap(fraction - 0.5, 1.0)
    }

    /// The strongest pair of adjacent bins at or next to the preamble's, and its level. A sender
    /// whose chip clock is off walks the preamble across bins: at SF12 and 100 ppm by a bin every
    /// two and a half upchirps.
    fn pair_level(&self, dechirper: &Dechirper) -> (usize, f32) {
        let energies = dechirper.energies();
        let n = energies.len();
        let mean = dechirper.mean();
        let pair = |bin: usize| energies[bin] + energies[(bin + 1) % n];
        let best = [(self.bin + n - 1) % n, (self.bin + 1) % n]
            .into_iter()
            .fold(
                self.bin,
                |best, bin| if pair(bin) > pair(best) { bin } else { best },
            );
        let level = if mean > 0.0 { pair(best) / mean } else { 0.0 };
        (best, level)
    }
}

enum Stage {
    Search,
    Preamble(Preamble),
    Align(Candidate),
    Payload { lock: Box<Lock>, rewind: u64 },
}

pub(crate) struct Settings {
    pub spreading_factor: u8,
    pub bandwidth_hz: f64,
    pub carrier_hz: f64,
    pub inverted: bool,
    pub implicit_header: Option<LoraImplicitHeader>,
}

pub(crate) struct Tools {
    pub dechirper: Dechirper,
    pub accumulator: Accumulator,
    pub downs: DownHistory,
    pub extractor: Extractor,
    pub demapper: Demapper,
    pub samples: Vec<Complex<f32>>,
    pub llrs: Vec<f32>,
    pub tail: Vec<f32>,
}

pub(crate) struct Receiver {
    settings: Settings,
    levels: Levels,
    tools: Tools,
    cursor: u64,
    stage: Stage,
}

enum Step {
    Advanced,
    Blocked,
}

impl Receiver {
    #[must_use]
    pub(crate) fn new(settings: Settings) -> Self {
        let chips = 1usize << settings.spreading_factor;
        Self {
            levels: Levels::new(chips),
            tools: Tools {
                dechirper: Dechirper::new(chips),
                accumulator: Accumulator::new(chips),
                downs: DownHistory::new(chips),
                extractor: Extractor::new(chips),
                demapper: Demapper::new(chips),
                samples: vec![Complex::new(0.0, 0.0); chips],
                llrs: vec![0.0; max_symbols(settings.spreading_factor) * MAX_BITS],
                tail: vec![0.0; MAX_COLUMNS * chips],
            },
            settings,
            cursor: 0,
            stage: Stage::Search,
        }
    }

    fn chips(&self) -> usize {
        1 << self.settings.spreading_factor
    }

    pub(crate) fn retune(&mut self, carrier_hz: f64) {
        self.settings.carrier_hz = carrier_hz;
    }

    pub(crate) fn run(&mut self, front: &Front, frames: &mut Vec<Frame>) {
        while let Step::Advanced = self.step(front, frames) {}
    }

    fn step(&mut self, front: &Front, frames: &mut Vec<Frame>) -> Step {
        match std::mem::replace(&mut self.stage, Stage::Search) {
            Stage::Search => self.search(front),
            Stage::Preamble(preamble) => self.preamble(front, preamble),
            Stage::Align(candidate) => self.align(front, candidate),
            Stage::Payload { lock, rewind } => self.payload(front, lock, rewind, frames),
        }
    }

    fn load_window(&mut self, front: &Front) -> bool {
        let narrow = front.narrow();
        let chips = self.chips() as u64;
        self.cursor = self.cursor.max(narrow.start());
        if self.cursor + chips > narrow.end() {
            return false;
        }
        narrow.copy(self.cursor as i64, &mut self.tools.samples);
        if self.settings.inverted {
            for sample in &mut self.tools.samples {
                *sample = sample.conj();
            }
        }
        true
    }

    fn search(&mut self, front: &Front) -> Step {
        if !self.load_window(front) {
            self.stage = Stage::Search;
            return Step::Blocked;
        }
        let start = self.cursor;
        self.cursor += self.chips() as u64;
        let dechirper = &mut self.tools.dechirper;
        dechirper.transform(&self.tools.samples, Slope::Up);
        let detection = self.tools.accumulator.add(
            start,
            dechirper.energies(),
            dechirper.mean(),
            self.levels.steady,
        );
        self.stage = match detection {
            Some(found) if found.level >= self.levels.detect && found.steady >= STEADY_WINDOWS => {
                self.tools.accumulator.reset();
                Stage::Preamble(Preamble {
                    bin: found.bin,
                    previous: dechirper.bins()[found.bin],
                    previous_start: start,
                    progression: Complex::new(0.0, 0.0),
                    interpolated: dechirper.fraction(found.bin),
                    upchirps: DETECT_WINDOWS as u32,
                    quiet: 0,
                    downchirp: None,
                })
            }
            _ => Stage::Search,
        };
        Step::Advanced
    }

    fn preamble(&mut self, front: &Front, mut preamble: Preamble) -> Step {
        if !self.load_window(front) {
            self.stage = Stage::Preamble(preamble);
            return Step::Blocked;
        }
        let chips = self.chips() as u64;
        let start = self.cursor;
        self.cursor += chips;
        self.tools
            .dechirper
            .transform(&self.tools.samples, Slope::Up);
        let (bin, level) = preamble.pair_level(&self.tools.dechirper);
        if level >= self.levels.carry_on && preamble.quiet < PREAMBLE_OVER {
            self.carry_on(&mut preamble, bin, start);
            self.stage = if preamble.upchirps > MAX_UPCHIRPS {
                Stage::Search
            } else {
                Stage::Preamble(preamble)
            };
            return Step::Advanced;
        }
        self.tools
            .dechirper
            .transform(&self.tools.samples, Slope::Down);
        let sighting = self.tools.downs.sighting(start, &self.tools.dechirper);
        if preamble
            .downchirp
            .is_none_or(|best| sighting.level > best.level)
        {
            preamble.downchirp = Some(sighting);
        }
        preamble.quiet += 1;
        self.stage = match preamble.downchirp {
            Some(best) if preamble.quiet >= QUIET_WINDOWS => Stage::Align(Candidate {
                up_bins: preamble.up_bins(),
                down_bins: best.bins,
                window_start: best.window_start,
            }),
            _ => Stage::Preamble(preamble),
        };
        Step::Advanced
    }

    /// One more upchirp on the preamble's bin: its phase step from the last one refines the
    /// fractional bin. A step across two bins turns by more than the tone did, so it is left out.
    fn carry_on(&self, preamble: &mut Preamble, bin: usize, start: u64) {
        let dechirper = &self.tools.dechirper;
        let current = dechirper.bins()[bin];
        if preamble.previous_start + self.chips() as u64 == start && bin == preamble.bin {
            let step = current * preamble.previous.conj();
            preamble.progression += Complex::new(f64::from(step.re), f64::from(step.im));
        }
        preamble.bin = bin;
        preamble.previous = current;
        preamble.previous_start = start;
        preamble.interpolated = dechirper.fraction(preamble.bin);
        preamble.upchirps += 1;
        preamble.quiet = 0;
    }

    fn align(&mut self, front: &Front, candidate: Candidate) -> Step {
        let chips = self.chips();
        match align::lock(&candidate, &self.settings, &mut self.tools, front) {
            align::Outcome::Waiting => {
                self.stage = Stage::Align(candidate);
                Step::Blocked
            }
            align::Outcome::Rejected => {
                self.cursor = candidate.window_start + chips as u64;
                self.stage = Stage::Search;
                Step::Advanced
            }
            align::Outcome::Locked(lock) => {
                self.stage = Stage::Payload {
                    lock,
                    rewind: candidate.window_start + chips as u64,
                };
                Step::Advanced
            }
        }
    }

    /// A lock that was never confident and whose header fails was likely no frame, so the search
    /// goes back to just after its SFD rather than losing the symbols the header took.
    fn payload(
        &mut self,
        front: &Front,
        mut lock: Box<Lock>,
        rewind: u64,
        frames: &mut Vec<Frame>,
    ) -> Step {
        match lock.advance(&self.settings, &mut self.tools, front.wide()) {
            None => {
                self.stage = Stage::Payload { lock, rewind };
                Step::Blocked
            }
            Some(None) => {
                self.stage = Stage::Payload { lock, rewind };
                Step::Advanced
            }
            Some(Some(finished)) => {
                let integrity = finished.frame.integrity;
                self.cursor = if !lock.confident() && integrity == LoraIntegrity::HeaderFailed {
                    rewind
                } else {
                    finished.resume_chip
                };
                if lock.confident() || integrity == LoraIntegrity::CrcOk {
                    frames.push(finished.frame);
                }
                self.stage = Stage::Search;
                Step::Advanced
            }
        }
    }
}
