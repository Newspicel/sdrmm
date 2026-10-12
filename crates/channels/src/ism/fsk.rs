use num_complex::Complex;
use sdrmm_dsp::fastmath::fast_arg;

use super::pulse::{MAX_PULSE, MAX_PULSES};

const CAPACITY: usize = MAX_PULSE as usize;
const MIN_SAMPLES: usize = 500;
const MIN_PULSES: usize = 16;
const SMOOTH: usize = 4;
const MIN_SEPARATION: f32 = 0.08;
const MIN_SHARE: f32 = 0.1;
const MAX_MIDDLE_SHARE: f32 = 0.3;
const HYSTERESIS: f32 = 0.1;
const MIN_RUN: u32 = 3;
const ROUNDS: usize = 4;

pub(super) struct Fsk {
    lags: Box<[Complex<f32>]>,
    len: usize,
    freq: Box<[f32]>,
    scratch: Box<[f32]>,
    pulses: Box<[u32; MAX_PULSES]>,
    gaps: Box<[u32; MAX_PULSES]>,
}

struct Levels {
    low: f32,
    high: f32,
}

impl Levels {
    fn mid(&self) -> f32 {
        (self.low + self.high) * 0.5
    }

    fn separation(&self) -> f32 {
        self.high - self.low
    }
}

impl Fsk {
    pub(super) fn new() -> Self {
        Self {
            lags: vec![Complex::new(0.0, 0.0); CAPACITY].into_boxed_slice(),
            len: 0,
            freq: vec![0.0; CAPACITY].into_boxed_slice(),
            scratch: vec![0.0; CAPACITY].into_boxed_slice(),
            pulses: Box::new([0; MAX_PULSES]),
            gaps: Box::new([0; MAX_PULSES]),
        }
    }

    pub(super) fn clear(&mut self) {
        self.len = 0;
    }

    pub(super) fn push(&mut self, lag: Complex<f32>) {
        if let Some(slot) = self.lags.get_mut(self.len) {
            *slot = lag;
            self.len += 1;
        }
    }

    pub(super) fn analyse(&mut self) -> Option<(&[u32], &[u32])> {
        if self.len < MIN_SAMPLES {
            return None;
        }
        self.discriminate();
        let levels = self.levels()?;
        let count = self.runs(&levels);
        (count >= MIN_PULSES).then(|| (&self.pulses[..count], &self.gaps[..count]))
    }

    fn discriminate(&mut self) {
        let mut sum = Complex::new(0.0f32, 0.0);
        for k in 0..self.len {
            sum += self.lags[k];
            if k >= SMOOTH {
                sum -= self.lags[k - SMOOTH];
            }
            self.freq[k] = fast_arg(sum);
        }
    }

    fn levels(&mut self) -> Option<Levels> {
        let len = self.len;
        let scratch = &mut self.scratch[..len];
        scratch.copy_from_slice(&self.freq[..len]);
        let (_, low, _) = scratch.select_nth_unstable_by(len / 10, f32::total_cmp);
        let low = *low;
        let (_, high, _) = scratch.select_nth_unstable_by(len * 9 / 10, f32::total_cmp);
        let mut levels = Levels { low, high: *high };
        for _ in 0..ROUNDS {
            levels = self.refine(&levels)?;
        }
        self.distinct(&levels).then_some(levels)
    }

    fn refine(&self, levels: &Levels) -> Option<Levels> {
        let mid = levels.mid();
        let (mut low, mut lows, mut high, mut highs) = (0.0f32, 0usize, 0.0f32, 0usize);
        for &f in &self.freq[..self.len] {
            if f < mid {
                low += f;
                lows += 1;
            } else {
                high += f;
                highs += 1;
            }
        }
        (lows > 0 && highs > 0).then(|| Levels {
            low: low / lows as f32,
            high: high / highs as f32,
        })
    }

    fn distinct(&self, levels: &Levels) -> bool {
        let separation = levels.separation();
        if separation < MIN_SEPARATION {
            return false;
        }
        let (mid, band) = (levels.mid(), separation * 0.25);
        let freq = &self.freq[..self.len];
        let middle = freq.iter().filter(|&&f| (f - mid).abs() < band).count();
        let highs = freq.iter().filter(|&&f| f >= mid).count();
        let len = self.len as f32;
        let share = highs as f32 / len;
        middle as f32 / len < MAX_MIDDLE_SHARE && share > MIN_SHARE && share < 1.0 - MIN_SHARE
    }

    fn runs(&mut self, levels: &Levels) -> usize {
        let (mid, band) = (levels.mid(), levels.separation() * HYSTERESIS);
        let mut level = self.freq[0] >= mid;
        if !level {
            self.pulses[0] = 0;
        }
        let (mut run, mut pending, mut count) = (0u32, 0u32, 0usize);
        for k in 0..self.len {
            let f = self.freq[k];
            let side = if level {
                f > mid - band
            } else {
                f >= mid + band
            };
            run += 1;
            if side == level {
                pending = 0;
                continue;
            }
            pending += 1;
            if pending < MIN_RUN {
                continue;
            }
            let done = run - pending;
            if !self.store(level, done, &mut count) {
                return count;
            }
            level = !level;
            run = pending;
            pending = 0;
        }
        if level {
            self.store(true, run, &mut count);
            self.store(false, 0, &mut count);
        } else {
            self.store(false, run, &mut count);
        }
        count
    }

    fn store(&mut self, high: bool, run: u32, count: &mut usize) -> bool {
        if *count >= MAX_PULSES {
            return false;
        }
        if high {
            self.pulses[*count] = run;
        } else {
            self.gaps[*count] = run;
            *count += 1;
        }
        true
    }
}
