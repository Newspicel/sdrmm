use num_complex::Complex;
use sdrmm_wire::IsmModulation;

use super::{RATE, fsk::Fsk};

pub(super) const MAX_PULSES: usize = 1_200;
const ENV_ALPHA: f32 = 0.3;
const START_RATIO: f32 = 2.5;
const FLOOR_RATIO: f32 = 1.6;
const HYSTERESIS: f32 = 1.12;
const HIGH_ATTACK: f32 = 0.25;
const HIGH_DECAY: f32 = 1.0 - 1.0 / (RATE as f32 * 0.025);
const NOISE_ALPHA: f32 = 1.0 / 2_048.0;
const WARMUP_ALPHA: f32 = 1.0 / 32.0;
const WARMUP_SAMPLES: u32 = 2_048;
const MIN_RUN: u32 = 8;
const NOISE_SETTLE: u32 = 32;
const GAP_RATIO: u32 = 10;
const MIN_END_GAP: u32 = (RATE * 0.01) as u32;
const MAX_END_GAP: u32 = (RATE * 0.1) as u32;
pub(super) const MAX_PULSE: u32 = (RATE * 0.1) as u32;
const NOISE_FLOOR: f32 = 1e-9;

pub(super) struct Package<'a> {
    pub(super) pulses: &'a [u32],
    pub(super) gaps: &'a [u32],
    pub(super) modulation: IsmModulation,
    pub(super) snr_db: f32,
}

impl Package<'_> {
    pub(super) fn len(&self) -> usize {
        self.pulses.len()
    }
}

pub(super) struct Detector {
    env: f32,
    noise: f32,
    high: f32,
    peak: f32,
    warm: u32,
    level: bool,
    run: u32,
    pending: u32,
    active: bool,
    pulses: Box<[u32; MAX_PULSES]>,
    gaps: Box<[u32; MAX_PULSES]>,
    count: usize,
    longest: u32,
    previous: Complex<f32>,
    fsk: Fsk,
}

impl Detector {
    pub(super) fn new() -> Self {
        Self {
            env: 0.0,
            noise: NOISE_FLOOR,
            high: 0.0,
            peak: 0.0,
            warm: 0,
            level: false,
            run: 0,
            pending: 0,
            active: false,
            pulses: Box::new([0; MAX_PULSES]),
            gaps: Box::new([0; MAX_PULSES]),
            count: 0,
            longest: 0,
            previous: Complex::new(0.0, 0.0),
            fsk: Fsk::new(),
        }
    }

    pub(super) fn reset(&mut self) {
        self.warm = 0;
        self.level = false;
        self.run = 0;
        self.pending = 0;
        self.active = false;
        self.count = 0;
        self.fsk.clear();
    }

    pub(super) fn process(&mut self, iq: &[Complex<f32>], sink: &mut dyn FnMut(&Package<'_>)) {
        for &sample in iq {
            self.sample(sample, sink);
        }
    }

    fn sample(&mut self, sample: Complex<f32>, sink: &mut dyn FnMut(&Package<'_>)) {
        self.env += (sample.norm() - self.env) * ENV_ALPHA;
        let lag = sample * self.previous.conj();
        self.previous = sample;
        if self.warm < WARMUP_SAMPLES {
            self.warm += 1;
            self.noise += (self.env - self.noise) * WARMUP_ALPHA;
            return;
        }
        self.high *= HIGH_DECAY;
        let above = self.above();
        self.step(above, sink);
        if self.level {
            self.fsk.push(lag);
            self.track_high();
        } else if self.pending == 0 && self.run > NOISE_SETTLE {
            self.noise = (self.noise + (self.env - self.noise) * NOISE_ALPHA).max(NOISE_FLOOR);
        }
    }

    fn above(&self) -> bool {
        if !self.active {
            return self.env > self.noise * START_RATIO;
        }
        let threshold = ((self.noise + self.high) * 0.5).max(self.noise * FLOOR_RATIO);
        if self.level {
            self.env > threshold / HYSTERESIS
        } else {
            self.env > threshold * HYSTERESIS
        }
    }

    fn track_high(&mut self) {
        if self.pending == 0 && self.env > self.high {
            self.high += (self.env - self.high) * HIGH_ATTACK;
            self.peak = self.peak.max(self.high);
        }
    }

    fn step(&mut self, above: bool, sink: &mut dyn FnMut(&Package<'_>)) {
        self.run = self.run.saturating_add(1);
        if above == self.level {
            self.pending = 0;
        } else {
            self.pending += 1;
            if self.pending >= MIN_RUN {
                let done = self.run - self.pending;
                self.level = above;
                self.run = self.pending;
                self.pending = 0;
                if above {
                    self.rise(done);
                } else {
                    self.fall(done, sink);
                }
            }
        }
        if self.active && !self.level && self.ended() {
            self.gaps[self.count - 1] = self.run;
            self.finish(sink);
        } else if self.level && self.run > MAX_PULSE {
            self.carrier(sink);
        }
    }

    fn ended(&self) -> bool {
        self.count > 0
            && (self.run > MAX_END_GAP
                || (self.run > GAP_RATIO * self.longest && self.run > MIN_END_GAP))
    }

    fn rise(&mut self, gap: u32) {
        if self.active {
            if let Some(last) = self.count.checked_sub(1) {
                self.gaps[last] = gap;
            }
        } else {
            self.active = true;
            self.count = 0;
            self.longest = 0;
            self.high = self.env;
            self.peak = self.env;
        }
        self.fsk.clear();
    }

    fn fall(&mut self, pulse: u32, sink: &mut dyn FnMut(&Package<'_>)) {
        if !self.active {
            return;
        }
        self.pulses[self.count] = pulse;
        self.gaps[self.count] = 0;
        self.count += 1;
        self.longest = self.longest.max(pulse);
        self.analyse_fsk(sink);
        if self.count == MAX_PULSES {
            self.finish(sink);
        }
    }

    fn carrier(&mut self, sink: &mut dyn FnMut(&Package<'_>)) {
        if self.active {
            self.pulses[self.count] = self.run;
            self.gaps[self.count] = 0;
            self.count += 1;
            self.analyse_fsk(sink);
            self.finish(sink);
        }
        self.noise = self.env.max(NOISE_FLOOR);
        self.level = false;
        self.run = 0;
        self.pending = 0;
    }

    fn snr_db(&self) -> f32 {
        20.0 * (self.peak.max(NOISE_FLOOR) / self.noise).log10()
    }

    fn analyse_fsk(&mut self, sink: &mut dyn FnMut(&Package<'_>)) {
        let snr_db = self.snr_db();
        if let Some((pulses, gaps)) = self.fsk.analyse() {
            sink(&Package {
                pulses,
                gaps,
                modulation: IsmModulation::Fsk,
                snr_db,
            });
        }
        self.fsk.clear();
    }

    fn finish(&mut self, sink: &mut dyn FnMut(&Package<'_>)) {
        let snr_db = self.snr_db();
        sink(&Package {
            pulses: &self.pulses[..self.count],
            gaps: &self.gaps[..self.count],
            modulation: IsmModulation::Ook,
            snr_db,
        });
        self.active = false;
        self.count = 0;
    }
}
