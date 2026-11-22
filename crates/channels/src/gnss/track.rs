//! One satellite's code and carrier tracking, bit sync, subframe sync and its transmit time.

use std::f64::consts::{PI, TAU};

use num_complex::Complex;

use super::acquire::{Found, RATE};
use super::code::{CHIPS, padded_code};
use super::nav::{Ephemeris, EphemerisCollector, SUBFRAME_BITS, Subframe, decode_subframe};

pub(super) const CHIP_RATE: f64 = 1.023e6;
pub(super) const L1_HZ: f64 = 1_575.42e6;
const MS_PER_BIT: u64 = 20;
const PULL_IN_MS: u64 = 400;
const BIT_SYNC_TRANSITIONS: u16 = 40;
const BIT_SYNC_SHARE: f32 = 0.6;
const WINDOW_MS: u32 = 200;
/// Phase lock indicator below which a window counts as unlocked, about 26 dB-Hz.
const LOCKED: f64 = 0.25;
const LOST_WINDOWS: u8 = 3;
const DAMPING: f64 = std::f64::consts::FRAC_1_SQRT_2;
const AMPLITUDE_ALPHA: f32 = 0.01;

/// Noise bandwidths in hertz for pull-in and for steady tracking.
#[derive(Clone, Copy)]
struct Loops {
    frequency: f64,
    phase: f64,
    code: f64,
}

const PULL_IN: Loops = Loops {
    frequency: 10.0,
    phase: 20.0,
    code: 4.0,
};

const STEADY: Loops = Loops {
    frequency: 0.0,
    phase: 15.0,
    code: 1.0,
};

/// What one code period produced for the caller to report.
pub(super) enum Step {
    Nothing,
    Subframe(Subframe),
    Lost,
}

#[derive(Clone, Copy, Default)]
struct Moments {
    m2: f64,
    m4: f64,
    in_phase: f64,
    count: u32,
    weak: u8,
}

#[derive(Clone, Copy, Default)]
struct BitSync {
    previous_sign: bool,
    transitions: [u16; MS_PER_BIT as usize],
    total: u16,
    phase: Option<u64>,
    sum: f32,
    first_bit_ms: Option<u64>,
}

pub(super) struct Tracker {
    pub prn: u8,
    code: [f32; CHIPS + 2],
    period_start: f64,
    carrier_hz: f64,
    carrier_phase: f64,
    previous_prompt: Option<Complex<f32>>,
    amplitude: f32,
    prompt: Complex<f32>,
    ms: u64,
    pub cn0_db_hz: f32,
    moments: Moments,
    sync: BitSync,
    bits: [bool; SUBFRAME_BITS],
    bit_count: u64,
    anchor: Option<(u64, f64)>,
    collector: EphemerisCollector,
    pub ephemeris: Option<Ephemeris>,
}

impl Tracker {
    /// Starts on the first code period that begins at or after absolute sample `from`.
    pub fn new(found: Found, from: u64) -> Self {
        let mut tracker = Self {
            prn: found.prn,
            code: padded_code(found.prn),
            period_start: found.code_start,
            carrier_hz: found.doppler_hz,
            carrier_phase: 0.0,
            previous_prompt: None,
            amplitude: 0.0,
            prompt: Complex::default(),
            ms: 0,
            cn0_db_hz: found.cn0_db_hz,
            moments: Moments::default(),
            sync: BitSync::default(),
            bits: [false; SUBFRAME_BITS],
            bit_count: 0,
            anchor: None,
            collector: EphemerisCollector::default(),
            ephemeris: None,
        };
        let period = tracker.period_samples();
        let behind = (from as f64 - tracker.period_start) / period;
        tracker.period_start += behind.ceil().max(0.0) * period;
        tracker
    }

    fn chip_step(&self) -> f64 {
        CHIP_RATE * (1.0 + self.carrier_hz / L1_HZ) / RATE
    }

    fn period_samples(&self) -> f64 {
        CHIPS as f64 / self.chip_step()
    }

    pub fn doppler_hz(&self) -> f64 {
        self.carrier_hz
    }

    /// Code phase in chips at absolute sample `at`.
    pub fn code_phase_chips(&self, at: f64) -> f64 {
        ((at - self.period_start) * self.chip_step()).rem_euclid(CHIPS as f64)
    }

    /// GPS seconds of week the signal arriving at absolute sample `at` left the satellite.
    pub fn transmit_s(&self, at: f64) -> Option<f64> {
        const WEEK_S: f64 = 604_800.0;
        let (anchor_ms, anchor_s) = self.anchor?;
        let periods = self.ms as f64 - anchor_ms as f64;
        let ahead = (self.period_start - at) * self.chip_step() / CHIP_RATE;
        Some((anchor_s + periods * 1e-3 - ahead).rem_euclid(WEEK_S))
    }

    /// The next period's end in absolute samples, the sample it needs last.
    pub fn needs_until(&self) -> f64 {
        self.period_start + self.period_samples() + 1.0
    }

    /// Integrates one code period from `history`, whose first sample is absolute `origin`.
    pub fn step(&mut self, history: &[Complex<f32>], origin: u64) -> Step {
        let first = self.period_start.ceil();
        let offset = (first - origin as f64).max(0.0) as usize;
        let phase = (first - self.period_start) * self.chip_step();
        let count = ((CHIPS as f64 - phase) / self.chip_step()).ceil() as usize;
        let Some(samples) = history.get(offset..offset + count) else {
            return Step::Lost;
        };
        let lead = TAU * self.carrier_hz * (first - self.period_start) / RATE;
        let [early, prompt, late] = self.correlate(samples, phase, self.carrier_phase + lead);
        self.hold_prompt(prompt);
        let period = self.period_samples();
        let loops = if self.ms < PULL_IN_MS {
            PULL_IN
        } else {
            STEADY
        };
        self.steer_carrier(prompt, period, loops);
        self.steer_code(early, late, loops);
        self.period_start += period;
        self.ms += 1;
        if self.measure(prompt) {
            return Step::Lost;
        }
        self.demodulate(prompt)
    }

    /// The last prompt over its running amplitude: the BPSK nav bits once the PLL holds.
    pub fn prompt(&self) -> Complex<f32> {
        self.prompt
    }

    fn hold_prompt(&mut self, prompt: Complex<f32>) {
        let size = prompt.norm();
        self.amplitude = if self.amplitude > 0.0 {
            self.amplitude + AMPLITUDE_ALPHA * (size - self.amplitude)
        } else {
            size
        };
        self.prompt = prompt / self.amplitude.max(f32::MIN_POSITIVE);
    }

    fn correlate(&self, samples: &[Complex<f32>], phase: f64, carrier: f64) -> [Complex<f32>; 3] {
        let step = self.chip_step();
        let turn = Complex::from_polar(1.0f32, (-TAU * self.carrier_hz / RATE) as f32);
        let mut rotor = Complex::from_polar(1.0f32, -carrier as f32);
        let mut sums = [Complex::<f32>::default(); 3];
        for (k, &sample) in samples.iter().enumerate() {
            let chip = phase + k as f64 * step;
            let base = sample * rotor;
            rotor *= turn;
            sums[0] += base * self.code[(chip + 1.5) as usize];
            sums[1] += base * self.code[(chip + 1.0) as usize];
            sums[2] += base * self.code[(chip + 0.5) as usize];
        }
        sums
    }

    /// Costas PLL with an FLL assist while pulling in, both data-insensitive.
    fn steer_carrier(&mut self, prompt: Complex<f32>, period: f64, loops: Loops) {
        let dt = period / RATE;
        let phase_error = wrap_half(f64::from(prompt.im).atan2(f64::from(prompt.re)));
        let natural = loops.phase / 0.53;
        let mut frequency = natural * natural * dt / TAU * phase_error;
        if let Some(previous) = self.previous_prompt
            && loops.frequency > 0.0
        {
            let cross = f64::from(previous.re * prompt.im - prompt.re * previous.im);
            let dot = f64::from(previous.re * prompt.re + previous.im * prompt.im);
            if dot != 0.0 {
                frequency += 4.0 * loops.frequency * dt * (cross / dot).atan() / (TAU * dt);
            }
        }
        self.previous_prompt = Some(prompt);
        self.carrier_phase = wrap(
            self.carrier_phase
                + TAU * self.carrier_hz * dt
                + 2.0 * DAMPING * natural * dt * phase_error,
        );
        self.carrier_hz += frequency;
    }

    /// First-order early-minus-late envelope DLL, carrier aided through `chip_step`.
    fn steer_code(&mut self, early: Complex<f32>, late: Complex<f32>, loops: Loops) {
        let (early, late) = (f64::from(early.norm()), f64::from(late.norm()));
        if early + late <= 0.0 {
            return;
        }
        let error_chips = 0.5 * (early - late) / (early + late);
        let gain = 4.0 * loops.code * 1e-3;
        self.period_start -= gain * error_chips / self.chip_step();
    }

    /// Updates C/N0 by the M2M4 moments estimator and reports a lost lock when the phase lock
    /// indicator, the in-phase share of prompt power, stays low.
    fn measure(&mut self, prompt: Complex<f32>) -> bool {
        let power = f64::from(prompt.norm_sqr());
        let m = &mut self.moments;
        m.m2 += power;
        m.m4 += power * power;
        m.in_phase += f64::from(prompt.re * prompt.re - prompt.im * prompt.im);
        m.count += 1;
        if m.count < WINDOW_MS {
            return false;
        }
        let m2 = m.m2 / f64::from(m.count);
        let m4 = m.m4 / f64::from(m.count);
        let signal = (2.0 * m2 * m2 - m4).max(0.0).sqrt();
        let noise = (m2 - signal).max(f64::MIN_POSITIVE);
        self.cn0_db_hz = (10.0 * (signal / noise * 1_000.0).max(1.0).log10()) as f32;
        let lock = m.in_phase / m.m2.max(f64::MIN_POSITIVE);
        let weak = if lock < LOCKED { m.weak + 1 } else { 0 };
        *m = Moments {
            weak,
            ..Moments::default()
        };
        weak >= LOST_WINDOWS
    }

    fn demodulate(&mut self, prompt: Complex<f32>) -> Step {
        let index = self.ms - 1;
        let sign = prompt.re > 0.0;
        let sync = &mut self.sync;
        let Some(phase) = sync.phase else {
            if index > PULL_IN_MS && sign != sync.previous_sign {
                sync.transitions[(index % MS_PER_BIT) as usize] += 1;
                sync.total += 1;
            }
            sync.previous_sign = sign;
            if sync.total >= BIT_SYNC_TRANSITIONS {
                self.lock_bits();
            }
            return Step::Nothing;
        };
        if index % MS_PER_BIT == phase {
            sync.first_bit_ms.get_or_insert(index);
            sync.sum = 0.0;
        }
        if sync.first_bit_ms.is_none() {
            return Step::Nothing;
        }
        sync.sum += prompt.re;
        if (index + 1) % MS_PER_BIT != phase {
            return Step::Nothing;
        }
        let bit = sync.sum > 0.0;
        self.push_bit(bit)
    }

    fn lock_bits(&mut self) {
        let sync = &mut self.sync;
        let (best, &count) = sync
            .transitions
            .iter()
            .enumerate()
            .max_by_key(|(_, count)| **count)
            .unwrap_or((0, &0));
        if f32::from(count) >= BIT_SYNC_SHARE * f32::from(sync.total) {
            sync.phase = Some(best as u64);
        } else {
            *sync = BitSync::default();
        }
    }

    fn push_bit(&mut self, bit: bool) -> Step {
        let slot = (self.bit_count % SUBFRAME_BITS as u64) as usize;
        self.bits[slot] = bit;
        self.bit_count += 1;
        if self.bit_count < SUBFRAME_BITS as u64 {
            return Step::Nothing;
        }
        let oldest = (self.bit_count % SUBFRAME_BITS as u64) as usize;
        let mut ordered = [false; SUBFRAME_BITS];
        let (newer, older) = self.bits.split_at(oldest);
        ordered[..older.len()].copy_from_slice(older);
        ordered[older.len()..].copy_from_slice(newer);
        let Some(subframe) = decode_subframe(&ordered) else {
            return Step::Nothing;
        };
        let first_bit = self.bit_count - SUBFRAME_BITS as u64;
        let first_ms = self.sync.first_bit_ms.unwrap_or(0) + first_bit * MS_PER_BIT;
        self.anchor = Some((first_ms, f64::from(subframe.start_tow_s())));
        if let Some(ephemeris) = self.collector.offer(&subframe) {
            self.ephemeris = Some(ephemeris);
        }
        Step::Subframe(subframe)
    }
}

fn wrap(phase: f64) -> f64 {
    (phase + PI).rem_euclid(TAU) - PI
}

fn wrap_half(phase: f64) -> f64 {
    (phase + PI / 2.0).rem_euclid(PI) - PI / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gnss::acquire::SAMPLES_PER_MS;

    fn track(tracker: &mut Tracker, iq: &[Complex<f32>]) {
        let mut origin = 0;
        let window = 3 * SAMPLES_PER_MS;
        while origin + window <= iq.len() {
            while tracker.needs_until() < (origin + window) as f64 {
                assert!(!matches!(
                    tracker.step(&iq[origin..origin + window], origin as u64),
                    Step::Lost
                ));
            }
            origin += SAMPLES_PER_MS;
        }
    }

    #[test]
    fn pulls_in_from_acquisition_errors_and_follows_code_and_carrier() {
        let iq = crate::gnss::synth::satellite(13, 2_345.0, 100.3, 1_500);
        let mut tracker = Tracker::new(
            Found {
                prn: 13,
                doppler_hz: 2_345.0 - 90.0,
                code_start: 100.3 + 0.4,
                cn0_db_hz: 50.0,
            },
            0,
        );
        track(&mut tracker, &iq);
        assert!(
            (tracker.doppler_hz() - 2_345.0).abs() < 1.0,
            "{}",
            tracker.doppler_hz()
        );
        let at = 1_400.0 * SAMPLES_PER_MS as f64;
        let chip_rate = CHIP_RATE * (1.0 + 2_345.0 / L1_HZ);
        let truth = ((at - 100.3) / RATE * chip_rate).rem_euclid(CHIPS as f64);
        let error = tracker.code_phase_chips(at) - truth;
        assert!(error.abs() < 0.02, "code off by {error} chips");
        assert!(tracker.cn0_db_hz > 50.0, "{}", tracker.cn0_db_hz);
    }

    #[test]
    fn a_vanished_signal_is_reported_lost() {
        let noise = sdrmm_decoder_test_support::complex_noise(4, 0.5, 800 * SAMPLES_PER_MS);
        let mut tracker = Tracker::new(
            Found {
                prn: 3,
                doppler_hz: 0.0,
                code_start: 0.0,
                cn0_db_hz: 45.0,
            },
            0,
        );
        let mut lost = false;
        let mut origin = 0;
        while !lost && origin + 3 * SAMPLES_PER_MS <= noise.len() {
            let history = &noise[origin..origin + 3 * SAMPLES_PER_MS];
            while tracker.needs_until() < (origin + 3 * SAMPLES_PER_MS) as f64 {
                lost |= matches!(tracker.step(history, origin as u64), Step::Lost);
            }
            origin += SAMPLES_PER_MS;
        }
        assert!(lost);
    }
}
