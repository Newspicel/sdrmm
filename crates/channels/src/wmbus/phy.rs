use num_complex::Complex;
use sdrmm_wire::{WMBUS_SAMPLE_RATE_HZ, WmbusFormat, WmbusMode};

use super::{
    coding::{manchester, nrz, three_of_six},
    link::{self, FIRST_BLOCK, MAX_RAW},
};

const RATE: f32 = WMBUS_SAMPLE_RATE_HZ as f32;
const FAST_CHIPS_PER_S: f32 = 100_000.0;
const SLOW_CHIPS_PER_S: f32 = 32_768.0;
const MAX_CHIPS: usize = 16 * MAX_RAW + 16;
const MAX_FILTER: usize = 16;
const DEVIATION_HZ: f32 = 50_000.0;
const FAST_AGILITY: f32 = 1.0 / 32.0;
const SLOW_AGILITY: f32 = 1.0 / 128.0;
const SYNC_CHIPS: u32 = 24;
const SYNC_MASK: u32 = (1 << SYNC_CHIPS) - 1;
const FAST_SYNC: u32 = 0x0055_543D;
const SLOW_SYNC: u32 = 0x0054_7696;
const C_HEADER: usize = 16;
const C_FORMAT_A: u8 = 0xCD;
const C_FORMAT_B: u8 = 0x3D;
const C_LEAD: u8 = 0x54;
const HUNT_GAIN: f32 = 0.5;
const TRACK_GAIN: f32 = 0.05;
const DC_ALPHA: f32 = 0.125;
const MIN_SWING: f32 = 1e-3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Fast,
    Slow,
}

pub(super) struct Burst<'a> {
    pub(super) mode: WmbusMode,
    pub(super) format: WmbusFormat,
    pub(super) raw: &'a [u8],
    pub(super) repaired: u32,
    pub(super) snr_db: f32,
}

#[derive(Clone, Copy)]
struct Shape {
    mode: WmbusMode,
    format: WmbusFormat,
    header: usize,
    per_byte: usize,
    inverted: bool,
}

#[derive(Clone, Copy, Default)]
struct Frame {
    shape: Option<Shape>,
    needed: Option<usize>,
    checked: bool,
}

#[derive(Clone, Copy)]
enum Stage {
    Hunt,
    Collect(Frame),
}

struct Boxcar {
    taps: [Complex<f32>; MAX_FILTER],
    len: usize,
    head: usize,
    sum: Complex<f32>,
}

impl Boxcar {
    fn new(len: usize) -> Self {
        Self {
            taps: [Complex::new(0.0, 0.0); MAX_FILTER],
            len: len.clamp(1, MAX_FILTER),
            head: 0,
            sum: Complex::new(0.0, 0.0),
        }
    }

    fn push(&mut self, value: Complex<f32>) -> Complex<f32> {
        self.sum += value - self.taps[self.head];
        self.taps[self.head] = value;
        self.head = (self.head + 1) % self.len;
        if self.head == 0 {
            self.sum = self.taps[..self.len].iter().sum();
        }
        self.sum
    }
}

struct Tones {
    phasor: Complex<f32>,
    step: Complex<f32>,
    upper: Boxcar,
    lower: Boxcar,
    lag: Complex<f32>,
    previous: Complex<f32>,
    derotate: Complex<f32>,
    agility: f32,
}

impl Tones {
    fn new(len: usize, agility: f32) -> Self {
        Self {
            phasor: Complex::new(1.0, 0.0),
            step: Complex::from_polar(1.0, std::f32::consts::TAU * DEVIATION_HZ / RATE),
            upper: Boxcar::new(len),
            lower: Boxcar::new(len),
            lag: Complex::new(0.0, 0.0),
            previous: Complex::new(0.0, 0.0),
            derotate: Complex::new(1.0, 0.0),
            agility,
        }
    }

    fn push(&mut self, sample: Complex<f32>, tracking: bool) -> f32 {
        if tracking {
            self.lag += (sample * self.previous.conj() - self.lag) * self.agility;
        }
        self.previous = sample;
        let lag = self.lag.norm();
        if lag > 0.0 {
            self.derotate *= self.lag.conj() / lag;
            self.derotate /= self.derotate.norm();
        }
        let centred = sample * self.derotate;
        let upper = self.upper.push(centred * self.phasor.conj()).norm();
        let lower = self.lower.push(centred * self.phasor).norm();
        self.phasor *= self.step;
        self.phasor /= self.phasor.norm();
        upper - lower
    }
}

pub(super) struct Lane {
    family: Family,
    period: f32,
    tones: Tones,
    countdown: f32,
    last: f32,
    dc: f32,
    swing: f32,
    shift: u32,
    pattern: [f32; SYNC_CHIPS as usize],
    stage: Stage,
    chips: Box<[f32]>,
    count: usize,
    bytes: Box<[u8; MAX_RAW]>,
    repaired: u32,
}

impl Lane {
    pub(super) fn new(family: Family) -> Self {
        let period = RATE
            / match family {
                Family::Fast => FAST_CHIPS_PER_S,
                Family::Slow => SLOW_CHIPS_PER_S,
            };
        Self {
            family,
            period,
            tones: Tones::new(
                period.round() as usize,
                match family {
                    Family::Fast => FAST_AGILITY,
                    Family::Slow => SLOW_AGILITY,
                },
            ),
            countdown: period,
            last: 0.0,
            dc: 0.0,
            swing: 1.0,
            shift: 0,
            pattern: [0.0; SYNC_CHIPS as usize],
            stage: Stage::Hunt,
            chips: vec![0.0; MAX_CHIPS].into_boxed_slice(),
            count: 0,
            bytes: Box::new([0; MAX_RAW]),
            repaired: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.stage = Stage::Hunt;
        self.shift = 0;
        self.dc = 0.0;
    }

    pub(super) fn push(&mut self, sample: Complex<f32>, sink: &mut dyn FnMut(Burst<'_>)) {
        let level = self.tones.push(sample, matches!(self.stage, Stage::Hunt));
        let previous = self.last - self.dc;
        let current = level - self.dc;
        if (current >= 0.0) != (previous >= 0.0) {
            let fraction = previous / (previous - current);
            let error = wrap(self.period * 0.5 + fraction - self.countdown, self.period);
            let gain = match self.stage {
                Stage::Hunt => HUNT_GAIN,
                _ => TRACK_GAIN,
            };
            self.countdown += gain * error;
        }
        self.countdown -= 1.0;
        if self.countdown <= 0.0 {
            let chip = self.last + (1.0 + self.countdown) * (level - self.last);
            self.countdown += self.period;
            self.chip(chip, sink);
        }
        self.last = level;
    }

    fn chip(&mut self, value: f32, sink: &mut dyn FnMut(Burst<'_>)) {
        let Stage::Collect(frame) = self.stage else {
            self.hunt(value);
            return;
        };
        self.chips[self.count] = (value - self.dc) / self.swing;
        self.count += 1;
        match self.progress(frame) {
            Some(frame) if frame.needed == Some(self.count) => self.finish(frame, sink),
            Some(frame) => self.stage = Stage::Collect(frame),
            None => self.stage = Stage::Hunt,
        }
    }

    fn hunt(&mut self, value: f32) {
        self.dc += DC_ALPHA * (value - self.dc);
        self.pattern.copy_within(1.., 0);
        self.pattern[SYNC_CHIPS as usize - 1] = value;
        self.shift = (self.shift << 1 | u32::from(value > self.dc)) & SYNC_MASK;
        let sync = match self.family {
            Family::Fast => FAST_SYNC,
            Family::Slow => SLOW_SYNC,
        };
        if (self.shift ^ sync).count_ones() <= 1 {
            self.lock(sync);
        }
    }

    fn lock(&mut self, sync: u32) {
        let (mut high, mut highs, mut low, mut lows) = (0.0f32, 0u32, 0.0f32, 0u32);
        for (k, &value) in self.pattern.iter().enumerate() {
            if sync >> (SYNC_CHIPS as usize - 1 - k) & 1 == 1 {
                high += value;
                highs += 1;
            } else {
                low += value;
                lows += 1;
            }
        }
        let (high, low) = (high / highs as f32, low / lows as f32);
        self.dc = (high + low) * 0.5;
        self.swing = ((high - low) * 0.5).max(MIN_SWING);
        self.count = 0;
        self.repaired = 0;
        self.shift = 0;
        let shape = (self.family == Family::Slow).then_some(Shape {
            mode: WmbusMode::S,
            format: WmbusFormat::A,
            header: 0,
            per_byte: 16,
            inverted: false,
        });
        self.stage = Stage::Collect(Frame {
            shape,
            ..Frame::default()
        });
    }

    fn progress(&mut self, mut frame: Frame) -> Option<Frame> {
        if frame.shape.is_none() {
            if self.count < C_HEADER {
                return Some(frame);
            }
            frame.shape = Some(self.fast_shape());
        }
        let shape = frame.shape?;
        if frame.needed.is_none() {
            if self.count < shape.header + shape.per_byte {
                return Some(frame);
            }
            frame.needed = Some(self.needed(shape, false)?);
        }
        let first = shape.header + shape.per_byte * FIRST_BLOCK;
        if shape.format == WmbusFormat::A && !frame.checked && self.count >= first {
            let shape = self.first_block(shape)?;
            frame.shape = Some(shape);
            frame.needed = Some(self.needed(shape, true)?);
            frame.checked = true;
        }
        Some(frame)
    }

    fn fast_shape(&self) -> Shape {
        let format = match (nrz(&self.chips[..8]), nrz(&self.chips[8..C_HEADER])) {
            (C_LEAD, C_FORMAT_A) => Some(WmbusFormat::A),
            (C_LEAD, C_FORMAT_B) => Some(WmbusFormat::B),
            _ => None,
        };
        match format {
            Some(format) => Shape {
                mode: WmbusMode::C,
                format,
                header: C_HEADER,
                per_byte: 8,
                inverted: false,
            },
            None => Shape {
                mode: WmbusMode::T,
                format: WmbusFormat::A,
                header: 0,
                per_byte: 12,
                inverted: false,
            },
        }
    }

    fn needed(&mut self, shape: Shape, settled: bool) -> Option<usize> {
        let length = self.byte(shape, 0);
        let total = match shape.mode {
            WmbusMode::S if !settled => {
                link::raw_len(shape.format, length).max(link::raw_len(shape.format, !length))
            }
            WmbusMode::S | WmbusMode::T | WmbusMode::C => link::raw_len(shape.format, length),
        }?;
        Some(shape.header + shape.per_byte * total.max(FIRST_BLOCK))
    }

    fn first_block(&mut self, shape: Shape) -> Option<Shape> {
        let flips: &[bool] = match shape.mode {
            WmbusMode::S => &[false, true],
            WmbusMode::T | WmbusMode::C => &[false],
        };
        flips.iter().find_map(|&inverted| {
            let shape = Shape { inverted, ..shape };
            for index in 0..FIRST_BLOCK {
                self.bytes[index] = self.byte(shape, index);
            }
            link::first_block_holds(&self.bytes[..FIRST_BLOCK]).then_some(shape)
        })
    }

    fn byte(&mut self, shape: Shape, index: usize) -> u8 {
        let start = shape.header + index * shape.per_byte;
        let chips = &self.chips[start..start + shape.per_byte];
        let byte = match shape.mode {
            WmbusMode::T => {
                let high = three_of_six(&chips[..6]);
                let low = three_of_six(&chips[6..]);
                self.repaired += u32::from(high.repaired) + u32::from(low.repaired);
                high.value << 4 | low.value
            }
            WmbusMode::C => nrz(chips),
            WmbusMode::S => manchester(chips),
        };
        if shape.inverted { !byte } else { byte }
    }

    fn finish(&mut self, frame: Frame, sink: &mut dyn FnMut(Burst<'_>)) {
        self.stage = Stage::Hunt;
        let (Some(shape), Some(needed)) = (frame.shape, frame.needed) else {
            return;
        };
        let total = (needed - shape.header) / shape.per_byte;
        self.repaired = 0;
        for index in 0..total {
            self.bytes[index] = self.byte(shape, index);
        }
        let snr_db = snr_db(&self.chips[shape.header..needed]);
        sink(Burst {
            mode: shape.mode,
            format: shape.format,
            raw: &self.bytes[..total],
            repaired: self.repaired,
            snr_db,
        });
    }
}

fn snr_db(chips: &[f32]) -> f32 {
    let len = chips.len().max(1) as f32;
    let mean = chips.iter().map(|chip| chip.abs()).sum::<f32>() / len;
    let spread = chips
        .iter()
        .map(|chip| (chip.abs() - mean).powi(2))
        .sum::<f32>()
        / len;
    10.0 * (mean * mean / spread.max(f32::EPSILON)).log10()
}

fn wrap(error: f32, period: f32) -> f32 {
    (error + period * 0.5).rem_euclid(period) - period * 0.5
}
