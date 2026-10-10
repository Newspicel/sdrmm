use num_complex::Complex;
use sdrmm_wire::LoraImplicitHeader;

use super::{
    Frame,
    align::{self, Candidate},
    chirp::{Dechirper, Slope, circular_distance, wrap},
    filter::Extractor,
    payload::{Lock, max_symbols},
    ring::Front,
    soft::{Demapper, MAX_BITS},
};

const DETECT_RUN: u32 = 4;
const MAX_MISSES: u32 = 3;
const MAX_UPCHIRPS: u32 = 256;
const MARGIN: f32 = 4.0;

#[must_use]
pub(crate) fn threshold(chips: usize) -> f32 {
    let ln = (chips as f32).ln();
    ln + ln.ln() + MARGIN
}

struct Preamble {
    bin: usize,
    previous: Complex<f32>,
    progression: Complex<f64>,
    interpolated: f64,
    upchirps: u32,
    misses: u32,
    downchirp: Option<Sighting>,
}

#[derive(Clone, Copy)]
struct Sighting {
    bins: f64,
    ratio: f32,
    window_start: u64,
}

impl Preamble {
    fn placeholder() -> Self {
        Self {
            bin: 0,
            previous: Complex::new(0.0, 0.0),
            progression: Complex::new(0.0, 0.0),
            interpolated: 0.0,
            upchirps: 0,
            misses: 0,
            downchirp: None,
        }
    }

    fn up_bins(&self) -> f64 {
        if self.progression.norm() == 0.0 {
            return self.bin as f64 + self.interpolated;
        }
        let fraction = self.progression.arg() / std::f64::consts::TAU;
        self.bin as f64 + 0.5 + wrap(fraction - 0.5, 1.0)
    }
}

enum Stage {
    Search { run: u32, bin: usize },
    Preamble(Preamble),
    Align(Candidate),
    Payload(Box<Lock>),
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
    pub extractor: Extractor,
    pub demapper: Demapper,
    pub samples: Vec<Complex<f32>>,
    pub llrs: Vec<f32>,
}

pub(crate) struct Receiver {
    settings: Settings,
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
            tools: Tools {
                dechirper: Dechirper::new(chips),
                extractor: Extractor::new(),
                demapper: Demapper::new(chips),
                samples: vec![Complex::new(0.0, 0.0); chips],
                llrs: vec![0.0; max_symbols(settings.spreading_factor) * MAX_BITS],
            },
            settings,
            cursor: 0,
            stage: Stage::Search { run: 0, bin: 0 },
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
        match std::mem::replace(&mut self.stage, Stage::Search { run: 0, bin: 0 }) {
            Stage::Search { run, bin } => self.search(front, run, bin),
            Stage::Preamble(preamble) => self.preamble(front, preamble),
            Stage::Align(candidate) => self.align(front, candidate),
            Stage::Payload(lock) => self.payload(front, lock, frames),
        }
    }

    fn load_window(&mut self, front: &Front) -> bool {
        let narrow = front.narrow();
        let chips = self.chips() as u64;
        self.cursor = self.cursor.max(narrow.start());
        if self.cursor + chips > narrow.end() {
            return false;
        }
        narrow.copy(self.cursor, &mut self.tools.samples);
        if self.settings.inverted {
            for sample in &mut self.tools.samples {
                *sample = sample.conj();
            }
        }
        true
    }

    fn search(&mut self, front: &Front, run: u32, bin: usize) -> Step {
        if !self.load_window(front) {
            self.stage = Stage::Search { run, bin };
            return Step::Blocked;
        }
        let chips = self.chips();
        self.tools
            .dechirper
            .transform(&self.tools.samples, Slope::Up);
        let peak = self.tools.dechirper.pair_peak();
        let run = if 2.0 * peak.ratio() < threshold(chips) {
            0
        } else if run > 0 && circular_distance(peak.bin, bin, chips) <= 1 {
            run + 1
        } else {
            1
        };
        self.cursor += chips as u64;
        self.stage = if run >= DETECT_RUN {
            Stage::Preamble(Preamble {
                bin: peak.bin,
                previous: self.tools.dechirper.bins()[peak.bin],
                progression: Complex::new(0.0, 0.0),
                interpolated: self.tools.dechirper.fraction(peak.bin),
                upchirps: run,
                misses: 0,
                downchirp: None,
            })
        } else {
            Stage::Search { run, bin: peak.bin }
        };
        Step::Advanced
    }

    fn preamble(&mut self, front: &Front, mut preamble: Preamble) -> Step {
        if !self.load_window(front) {
            self.stage = Stage::Preamble(preamble);
            return Step::Blocked;
        }
        let chips = self.chips();
        let start = self.cursor;
        self.cursor += chips as u64;
        self.tools
            .dechirper
            .transform(&self.tools.samples, Slope::Up);
        let up = self.tools.dechirper.pair_peak();
        let still_up = 2.0 * up.ratio() >= threshold(chips)
            && circular_distance(up.bin, preamble.bin, chips) <= 1;
        if still_up && preamble.misses == 0 {
            let current = self.tools.dechirper.bins()[preamble.bin];
            let step = current * preamble.previous.conj();
            preamble.progression += Complex::new(f64::from(step.re), f64::from(step.im));
            preamble.previous = current;
            preamble.interpolated = self.tools.dechirper.fraction(preamble.bin);
            preamble.upchirps += 1;
            self.stage = if preamble.upchirps > MAX_UPCHIRPS {
                Stage::Search { run: 0, bin: 0 }
            } else {
                Stage::Preamble(preamble)
            };
            return Step::Advanced;
        }
        self.tools
            .dechirper
            .transform(&self.tools.samples, Slope::Down);
        let down = self.tools.dechirper.peak();
        let sighting = (down.ratio() >= threshold(chips)).then(|| Sighting {
            bins: down.bin as f64 + self.tools.dechirper.fraction(down.bin),
            ratio: down.ratio(),
            window_start: start,
        });
        if let Some(stage) = Self::settle_downchirp(&mut preamble, sighting) {
            self.stage = stage;
            return Step::Advanced;
        }
        preamble.misses += 1;
        self.stage = if preamble.misses > MAX_MISSES {
            Stage::Search { run: 0, bin: 0 }
        } else {
            Stage::Preamble(preamble)
        };
        Step::Advanced
    }

    fn settle_downchirp(preamble: &mut Preamble, sighting: Option<Sighting>) -> Option<Stage> {
        let best = match (preamble.downchirp, sighting) {
            (None, Some(first)) => {
                preamble.downchirp = Some(first);
                return Some(Stage::Preamble(std::mem::replace(
                    preamble,
                    Preamble::placeholder(),
                )));
            }
            (Some(first), Some(second)) if second.ratio > first.ratio => second,
            (Some(first), _) => first,
            (None, None) => return None,
        };
        Some(Stage::Align(Candidate {
            up_bins: preamble.up_bins(),
            down_bins: best.bins,
            window_start: best.window_start,
        }))
    }

    fn align(&mut self, front: &Front, candidate: Candidate) -> Step {
        let chips = self.chips();
        match align::lock(&candidate, &self.settings, &mut self.tools, front.wide()) {
            align::Outcome::Waiting => {
                self.stage = Stage::Align(candidate);
                Step::Blocked
            }
            align::Outcome::Rejected => {
                self.cursor = candidate.window_start + chips as u64;
                self.stage = Stage::Search { run: 0, bin: 0 };
                Step::Advanced
            }
            align::Outcome::Locked(lock) => {
                self.stage = Stage::Payload(lock);
                Step::Advanced
            }
        }
    }

    fn payload(&mut self, front: &Front, mut lock: Box<Lock>, frames: &mut Vec<Frame>) -> Step {
        match lock.advance(&self.settings, &mut self.tools, front.wide()) {
            None => {
                self.stage = Stage::Payload(lock);
                Step::Blocked
            }
            Some(None) => {
                self.stage = Stage::Payload(lock);
                Step::Advanced
            }
            Some(Some(finished)) => {
                self.cursor = finished.resume_chip;
                frames.push(finished.frame);
                self.stage = Stage::Search { run: 0, bin: 0 };
                Step::Advanced
            }
        }
    }
}
