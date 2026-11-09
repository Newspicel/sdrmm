use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{
    Decimator, FmDemod, Nco, RealDecimator, SymbolSync, ToneCorrelator, design_lowpass,
    one_pole_coeff,
};

use super::params::{CpmParams, Mapping};

pub const TIMING_BW_BURST: f64 = 0.015;

pub const TIMING_BW_CONTINUOUS: f64 = 0.003;

const CENTRE_SYMBOLS_FLOOR: f32 = 150.0;

const CENTRE_POWER_SYMBOLS: f32 = 30.0;

const PEAK_SYMBOLS: f32 = 60.0;

const PEAK_HOLD_SYMBOLS: f32 = 4.0 * PEAK_SYMBOLS;

const PEAK_ATTACK: f32 = 0.125;

const ENVELOPE_TAU_SYMBOLS: f64 = 0.5;

const FLOOR_TAU_SYMBOLS: f64 = 96.0;

const FLOOR_SETTLE: f64 = 4.0;

const CARRIER_RISE: f32 = 4.0;

const STEADY_SPREAD: f32 = 0.25;

/// How long the envelope must stay under the carrier threshold before the gate closes. Near
/// the noise floor a burst's envelope dips under it for a sample or two; without a hangover each
/// dip restarted the settle and cut the burst into pieces too short to time.
const HANGOVER_SYMBOLS: f64 = 2.0;

/// How far past the outer symbol level the discriminator must swing to count as wild. A carrier
/// stays inside its deviation; noise of any strength throws the phase around and lands out here
/// far more often, which power alone cannot tell from a weak carrier.
const WILD_LEVELS: f32 = 2.0;
/// The share of noise's own wild rate at which an open gate is taken to hold noise, and its floor
/// follows so a noise floor that rose since it was learned closes the gate again. A carrier
/// a few dB over the noise runs at under half of it.
const NOISE_LIKENESS: f32 = 0.75;
/// How far under the level it was keyed at the envelope must fall for a carrier to have ended.
/// The gate then only keys again on something clearly over what is left, however much higher
/// that is than the floor learned before.
const CARRIER_DROP: f32 = 16.0;
/// How much faster than usual the floor rises once an open gate is known to hold noise.
const NOISE_FLOOR_SPEED: f32 = 8.0;
/// How quickly an open gate's wild rate follows the input.
const WILD_TAU_SYMBOLS: f64 = 16.0;
/// How quickly the centre wild swings are measured from follows the discriminator, so a
/// carrier offset the loops have not caught up with does not read as wild.
const SWING_TAU_SYMBOLS: f64 = 4.0;

const OFFSET_SYMBOLS: f32 = 64.0;
const OFFSET_TRUST: f32 = 0.5;
const INNER_RUN_SYMBOLS: usize = 16;

const IMAGE_TAPS: usize = 127;
const OFFSET_LOOP_GAIN: f64 = 0.02;
const OFFSET_LIMIT_SYMBOLS: f64 = 1.0;
const STEP_REFRESH: usize = 16;
/// Symbols each burst's opening timing estimate spans: half a DMR burst.
const ACQUIRE_SYMBOLS: usize = 64;
const COARSE_SYMBOLS: f64 = 1024.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RealDetector {
    Discriminator { centre_hz: f64 },
    ToneFilterbank { plus_hz: f64, minus_hz: f64 },
}

pub struct IqFront(IqKind);

enum IqKind {
    Quadrature(FmDemod),
    Tones(Box<ToneFront>),
}

struct ToneFront {
    plus: ToneCorrelator<Complex<f32>>,
    minus: ToneCorrelator<Complex<f32>>,
    tracker: OffsetTracker,
}

pub struct RealFront(RealKind);

enum RealKind {
    Analytic(Box<Analytic>),
    Filterbank {
        plus: ToneCorrelator,
        minus: ToneCorrelator,
    },
}

struct Analytic {
    mixer: Nco,
    image: Decimator,
    demod: FmDemod,
    mixed: Vec<Complex<f32>>,
    baseband: Vec<Complex<f32>>,
}

#[derive(Clone, Debug)]
struct OffsetTracker {
    gain: f64,
    tone: Complex<f64>,
    frequency: f64,
    limit: f64,
    rotor: Complex<f64>,
    step: Complex<f64>,
    since_step: usize,
    previous: [Complex<f64>; 2],
    power: f64,
    alpha: f64,
    lag: Complex<f64>,
    last: Complex<f64>,
    coarse_alpha: f64,
}

impl OffsetTracker {
    fn new(tone: f64, gain: f64, limit: f64, alpha: f64, coarse_alpha: f64) -> Self {
        Self {
            gain,
            tone: Complex::from_polar(1.0, -TAU * tone),
            frequency: 0.0,
            limit: TAU * limit,
            rotor: Complex::new(1.0, 0.0),
            step: Complex::new(1.0, 0.0),
            since_step: 0,
            previous: [Complex::new(0.0, 0.0); 2],
            power: 0.0,
            alpha,
            lag: Complex::new(0.0, 0.0),
            last: Complex::new(0.0, 0.0),
            coarse_alpha,
        }
    }

    fn centre(&mut self, sample: Complex<f32>) -> Complex<f32> {
        if !(sample.re.is_finite() && sample.im.is_finite()) {
            return Complex::new(0.0, 0.0);
        }
        let raw = Complex::new(f64::from(sample.re), f64::from(sample.im));
        self.lag += self.coarse_alpha * (raw * self.last.conj() - self.lag);
        self.last = raw;
        let turned = raw * self.rotor;
        self.rotor *= self.step;
        self.since_step += 1;
        if self.since_step == STEP_REFRESH {
            self.since_step = 0;
            self.step = Complex::from_polar(1.0, -(self.lag.arg() + self.frequency));
            self.rotor /= self.rotor.norm();
        }
        Complex::new(turned.re as f32, turned.im as f32)
    }

    fn follow(&mut self, plus: Complex<f64>, minus: Complex<f64>) {
        let (winner, index, reference) = if plus.norm_sqr() >= minus.norm_sqr() {
            (plus, 0, self.tone)
        } else {
            (minus, 1, self.tone.conj())
        };
        let power = winner.norm_sqr();
        self.power += self.alpha * (power - self.power);
        let rotation = winner * self.previous[index].conj() * reference;
        self.previous = [plus, minus];
        if self.power > 0.0 {
            let error = rotation.im / self.power;
            self.frequency = (self.frequency + self.gain * error).clamp(-self.limit, self.limit);
        }
    }
}

#[derive(Clone, Copy)]
struct Gate {
    envelope: f32,
    floor: f32,
    steady_level: f32,
    mean: f32,
    mean_square: f32,
    envelope_alpha: f32,
    floor_alpha: f32,
    settling: usize,
    settle_samples: usize,
    keyed: usize,
    support: usize,
    quiet: usize,
    hangover: usize,
    carrier_level: f32,
    wild_rate: f32,
    noise_wild: f32,
    noise_heard: usize,
    wild_alpha: f32,
    always_keyed: bool,
}

impl Gate {
    /// `wild` says whether the discriminator swung past the signal's deviation on this sample,
    /// when the front has a discriminator to ask.
    #[inline]
    fn step(&mut self, power: f32, wild: Option<bool>) -> (bool, bool) {
        if self.always_keyed {
            self.keyed = (self.keyed + 1).min(self.support);
            return (true, self.keyed == self.support);
        }
        self.envelope += self.envelope_alpha * (power - self.envelope);
        self.mean += self.floor_alpha * (power - self.mean);
        self.mean_square += self.floor_alpha * (power * power - self.mean_square);
        self.settling = self.settling.saturating_sub(1);
        let spread = self.mean_square - self.mean * self.mean;
        let steady = spread < STEADY_SPREAD * self.mean * self.mean;
        let carrier = self.settling == 0 && (steady || self.envelope > self.floor * CARRIER_RISE);
        self.quiet = if carrier { 0 } else { self.quiet + 1 };
        let keyed = carrier || (self.keyed > 0 && self.quiet <= self.hangover);
        let noisy = wild.is_some_and(|wild| self.hears_noise(wild, keyed));
        if steady {
            self.steady_level = self.mean;
        }
        self.carrier_level = if keyed && !noisy {
            self.envelope
                .max(self.carrier_level * (1.0 - self.floor_alpha))
        } else {
            0.0
        };
        if self.steady_level > 0.0 && self.mean * CARRIER_RISE < self.steady_level {
            self.floor = self.mean;
            self.steady_level = 0.0;
        } else if self.carrier_level > 0.0 && self.envelope * CARRIER_DROP < self.carrier_level {
            self.floor = self.floor.max(self.envelope / CARRIER_RISE);
            self.carrier_level = 0.0;
        } else if noisy {
            self.floor += NOISE_FLOOR_SPEED * self.floor_alpha * (self.envelope - self.floor);
        } else if !keyed && !steady {
            self.floor += self.floor_alpha * (self.envelope - self.floor);
        }
        self.keyed = if keyed {
            (self.keyed + 1).min(self.support)
        } else {
            0
        };
        (keyed, self.keyed == self.support && !noisy)
    }

    /// Whether an open gate's input swings like noise, learning noise's wild rate while closed.
    fn hears_noise(&mut self, wild: bool, keyed: bool) -> bool {
        let wild = f32::from(u8::from(wild));
        if !keyed {
            self.noise_wild += self.floor_alpha * (wild - self.noise_wild);
            self.noise_heard = self.noise_heard.saturating_add(1);
            self.wild_rate = 0.0;
            return false;
        }
        self.wild_rate += self.wild_alpha * (wild - self.wild_rate);
        self.noise_heard >= self.settle_samples && self.wild_rate > NOISE_LIKENESS * self.noise_wild
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
        self.floor = 0.0;
        self.steady_level = 0.0;
        self.mean = 0.0;
        self.mean_square = 0.0;
        self.settling = self.settle_samples;
        self.keyed = 0;
        self.quiet = 0;
        self.carrier_level = 0.0;
        self.wild_rate = 0.0;
        self.noise_wild = 0.0;
        self.noise_heard = 0;
    }
}

pub struct CpmDemod<F = IqFront> {
    front: F,
    matched: RealDecimator,
    sync: SymbolSync,
    level_max: f32,
    swing_centre: f32,
    swing_alpha: f32,
    centre: f32,
    centre_scale: f64,
    peak: f32,
    idle_peak: f32,
    centre_alpha: f32,
    peak_decay: f32,
    peak_hold_decay: f32,
    outer_region: f32,
    levels: Option<Mapping>,
    offset: f32,
    since_outer: usize,
    gate: Gate,
    demod_buf: Vec<f32>,
    filtered: Vec<f32>,
    carrier_run: Vec<bool>,
    settled_run: Vec<bool>,
    centred: Vec<Complex<f32>>,
    retimed: Vec<Complex<f32>>,
    retimed_carrier: Vec<bool>,
    retimed_settled: Vec<bool>,
}

impl CpmDemod<IqFront> {
    #[must_use]
    pub fn new(params: &CpmParams, receive_filter: &[f32], timing_bw: f64) -> Self {
        let front = IqFront(IqKind::Quadrature(FmDemod::new(
            params.sps(),
            params.h() / 2.0,
        )));
        Self::build(params, receive_filter, timing_bw, front, 0)
    }

    #[must_use]
    pub fn tones(params: &CpmParams, receive_filter: &[f32], timing_bw: f64) -> Self {
        assert_eq!(
            params.mapping().m(),
            2,
            "a two-tone filterbank detects two levels"
        );
        let sps = params.sps();
        let window = (sps.round() as usize).max(2);
        let shift = params.h() / 2.0;
        let front = IqFront(IqKind::Tones(Box::new(ToneFront {
            plus: ToneCorrelator::complex(sps, shift, window),
            minus: ToneCorrelator::complex(sps, -shift, window),
            tracker: OffsetTracker::new(
                shift / sps,
                OFFSET_LOOP_GAIN / sps,
                OFFSET_LIMIT_SYMBOLS / sps,
                1.0 / sps,
                1.0 / (COARSE_SYMBOLS * sps),
            ),
        })));
        Self::build(params, receive_filter, timing_bw, front, window)
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<f32>) {
        self.carrier_run.clear();
        self.settled_run.clear();
        let quadrature = matches!(self.front.0, IqKind::Quadrature(_));
        if !quadrature {
            self.gate_run(iq.iter().map(|s| (s.norm_sqr(), None)));
        }
        match &mut self.front.0 {
            IqKind::Quadrature(demod) => demod.process(iq, &mut self.demod_buf),
            IqKind::Tones(tones) => {
                let ToneFront {
                    plus,
                    minus,
                    tracker,
                } = tones.as_mut();
                self.demod_buf.clear();
                self.demod_buf.extend(iq.iter().map(|&s| {
                    let centred = tracker.centre(s);
                    let difference = plus.push(centred) - minus.push(centred);
                    tracker.follow(plus.correlation(), minus.correlation());
                    difference
                }));
            }
        }
        if quadrature {
            let bound = WILD_LEVELS * self.level_max;
            let (mut swing, alpha) = (self.swing_centre, self.swing_alpha);
            let demod_buf = std::mem::take(&mut self.demod_buf);
            self.gate_run(iq.iter().zip(&demod_buf).map(|(s, &f)| {
                let wild = (f - swing).abs() > bound;
                swing += alpha * (f.clamp(swing - bound, swing + bound) - swing);
                (s.norm_sqr(), Some(wild))
            }));
            self.swing_centre = swing;
            self.demod_buf = demod_buf;
        }
        self.finish(out);
    }

    pub fn reset(&mut self) {
        self.clear();
    }
}

impl CpmDemod<RealFront> {
    #[must_use]
    pub fn real(
        params: &CpmParams,
        receive_filter: &[f32],
        timing_bw: f64,
        sample_rate: f64,
        detector: RealDetector,
    ) -> Self {
        assert!(
            sample_rate.is_finite() && sample_rate > 0.0,
            "sample rate must be positive"
        );
        let baud = sample_rate / params.sps();
        let (front, extra) = match detector {
            RealDetector::Discriminator { centre_hz } => {
                assert!(
                    centre_hz > 0.0 && centre_hz < sample_rate / 2.0,
                    "subcarrier centre must lie inside the Nyquist band"
                );
                let taps = design_lowpass(IMAGE_TAPS, centre_hz / sample_rate);
                (
                    RealKind::Analytic(Box::new(Analytic {
                        mixer: Nco::new(-centre_hz as f32, sample_rate as f32),
                        image: Decimator::new(&taps, 1),
                        demod: FmDemod::new(sample_rate, params.h() * baud / 2.0),
                        mixed: Vec::new(),
                        baseband: Vec::new(),
                    })),
                    IMAGE_TAPS,
                )
            }
            RealDetector::ToneFilterbank { plus_hz, minus_hz } => {
                assert_eq!(
                    params.mapping().m(),
                    2,
                    "a two-tone filterbank detects two levels"
                );
                assert!(plus_hz != minus_hz, "filterbank tones must be distinct");
                let window = (sample_rate / (plus_hz - minus_hz).abs()).round() as usize;
                (
                    RealKind::Filterbank {
                        plus: ToneCorrelator::new(sample_rate, plus_hz, window),
                        minus: ToneCorrelator::new(sample_rate, minus_hz, window),
                    },
                    window,
                )
            }
        };
        Self::build(params, receive_filter, timing_bw, RealFront(front), extra)
    }

    pub fn process_real(&mut self, audio: &[f32], out: &mut Vec<f32>) {
        self.carrier_run.clear();
        self.settled_run.clear();
        self.gate_run(audio.iter().map(|&sample| (sample * sample, None)));
        match &mut self.front.0 {
            RealKind::Analytic(analytic) => {
                let Analytic {
                    mixer,
                    image,
                    demod,
                    mixed,
                    baseband,
                } = analytic.as_mut();
                mixed.clear();
                mixed.extend(
                    audio
                        .iter()
                        .map(|&s| Complex::new(s, 0.0) * mixer.next_sample()),
                );
                image.process(mixed, baseband);
                demod.process(baseband, &mut self.demod_buf);
            }
            RealKind::Filterbank { plus, minus } => {
                self.demod_buf.clear();
                self.demod_buf
                    .extend(audio.iter().map(|&s| plus.push(s) - minus.push(s)));
            }
        }
        self.finish(out);
    }

    pub fn reset(&mut self) {
        if let RealKind::Filterbank { plus, minus } = &mut self.front.0 {
            plus.reset();
            minus.reset();
        }
        self.clear();
    }
}

impl<F> CpmDemod<F> {
    fn build(
        params: &CpmParams,
        receive_filter: &[f32],
        timing_bw: f64,
        front: F,
        front_support: usize,
    ) -> Self {
        assert!(!receive_filter.is_empty(), "receive filter must have taps");
        let area: f64 = receive_filter.iter().map(|&t| f64::from(t)).sum();
        assert!(
            (area - 1.0).abs() < 1e-3,
            "receive filter must be unit-area (pulse::Norm::Area), got Σ = {area}"
        );
        let sps = params.sps();
        let level_max = params.mapping().max_level();
        let settle = (FLOOR_SETTLE * FLOOR_TAU_SYMBOLS * sps) as usize;
        let mean_sq = params
            .mapping()
            .levels()
            .iter()
            .map(|&l| l * l)
            .sum::<f32>()
            / params.mapping().m() as f32;
        let half_spacing = params.mapping().min_spacing() / 2.0;
        let centre_symbols = (CENTRE_POWER_SYMBOLS * mean_sq / (half_spacing * half_spacing))
            .max(CENTRE_SYMBOLS_FLOOR);
        Self {
            front,
            matched: RealDecimator::new(receive_filter, 1),
            sync: timing_loop(params, sps, timing_bw),
            swing_centre: 0.0,
            swing_alpha: one_pole_coeff(sps, SWING_TAU_SYMBOLS),
            level_max,
            centre: 0.0,
            centre_scale: params.h() / (2.0 * sps),
            peak: level_max,
            idle_peak: level_max,
            centre_alpha: 1.0 / (centre_symbols * sps as f32),
            peak_decay: 1.0 - 1.0 / PEAK_SYMBOLS,
            peak_hold_decay: 1.0 - 1.0 / PEAK_HOLD_SYMBOLS,
            outer_region: (level_max - params.mapping().min_spacing() / 2.0).max(0.0) / level_max,
            levels: (params.mapping().m() > 2).then(|| params.mapping().clone()),
            offset: 0.0,
            since_outer: 0,
            gate: Gate {
                envelope: 0.0,
                floor: 0.0,
                steady_level: 0.0,
                mean: 0.0,
                mean_square: 0.0,
                envelope_alpha: one_pole_coeff(sps, ENVELOPE_TAU_SYMBOLS),
                floor_alpha: one_pole_coeff(sps, FLOOR_TAU_SYMBOLS),
                settling: settle,
                settle_samples: settle,
                keyed: 0,
                support: receive_filter.len() + front_support,
                quiet: 0,
                hangover: (HANGOVER_SYMBOLS * sps).round() as usize,
                carrier_level: 0.0,
                wild_rate: 0.0,
                noise_wild: 0.0,
                noise_heard: 0,
                wild_alpha: one_pole_coeff(sps, WILD_TAU_SYMBOLS),
                always_keyed: false,
            },
            demod_buf: Vec::new(),
            filtered: Vec::new(),
            carrier_run: Vec::new(),
            settled_run: Vec::new(),
            centred: Vec::new(),
            retimed: Vec::new(),
            retimed_carrier: Vec::new(),
            retimed_settled: Vec::new(),
        }
    }

    #[must_use]
    pub fn continuous(mut self) -> Self {
        self.gate.always_keyed = true;
        self
    }

    #[must_use]
    pub fn settled(&self) -> &[bool] {
        &self.retimed_settled
    }

    #[must_use]
    pub fn frequency_error_cycles_per_sample(&self) -> f64 {
        f64::from(self.centre + self.offset) * self.centre_scale
    }

    fn gate_run(&mut self, powers: impl Iterator<Item = (f32, Option<bool>)>) {
        let mut gate = self.gate;
        for (power, wild) in powers {
            let (keyed, settled) = gate.step(power, wild);
            self.carrier_run.push(keyed);
            self.settled_run.push(settled);
        }
        self.gate = gate;
    }

    fn finish(&mut self, out: &mut Vec<f32>) {
        self.matched.process(&self.demod_buf, &mut self.filtered);
        self.centred.clear();
        for (&sample, &settled) in self.filtered.iter().zip(&self.settled_run) {
            if settled {
                self.centre += self.centre_alpha * (sample - self.centre);
            }
            self.centred.push(Complex::new(sample - self.centre, 0.0));
        }

        self.retimed.clear();
        self.retimed_carrier.clear();
        self.retimed_settled.clear();
        let mut start = 0;
        while start < self.centred.len() {
            let (carrier, settled) = (self.carrier_run[start], self.settled_run[start]);
            let mut end = start + 1;
            while end < self.centred.len()
                && self.carrier_run[end] == carrier
                && self.settled_run[end] == settled
            {
                end += 1;
            }
            let run = &self.centred[start..end];
            if settled {
                self.sync.process(run, &mut self.retimed);
            } else {
                self.sync.process_held(run, &mut self.retimed);
            }
            self.retimed_carrier.resize(self.retimed.len(), carrier);
            self.retimed_settled.resize(self.retimed.len(), settled);
            start = end;
        }

        let (mut peak, mut idle) = (self.peak, self.idle_peak);
        let carriers = self.retimed_carrier.iter().zip(&self.retimed_settled);
        for (symbol, (&carrier, &settled)) in self.retimed.iter().zip(carriers) {
            let value = symbol.re - self.offset;
            let magnitude = value.abs();
            if carrier {
                if settled && (self.levels.is_none() || magnitude <= WILD_LEVELS * self.level_max) {
                    self.since_outer += 1;
                    if magnitude > peak {
                        peak += PEAK_ATTACK * (magnitude - peak);
                        self.since_outer = 0;
                    } else if magnitude > peak * self.outer_region {
                        peak += (magnitude - peak) / PEAK_SYMBOLS;
                        self.since_outer = 0;
                    } else if let Some(scaled) = self.inner_scale(magnitude, peak) {
                        peak += (scaled - peak) / PEAK_SYMBOLS;
                    } else {
                        peak *= self.peak_hold_decay;
                    }
                }
            } else {
                if magnitude > idle {
                    idle += PEAK_ATTACK * (magnitude - idle);
                } else {
                    idle *= self.peak_decay;
                }
            }
            let level = if carrier { &peak } else { &idle };
            let unit = *level / self.level_max;
            let normalised = if unit > 1e-6 { value / unit } else { 0.0 };
            if carrier
                && settled
                && let Some(levels) = &self.levels
            {
                let miss = normalised - levels.level(levels.slice(normalised));
                if miss.abs() < OFFSET_TRUST {
                    self.offset = (self.offset + miss * unit / OFFSET_SYMBOLS).clamp(-unit, unit);
                }
            }
            out.push(normalised);
        }
        (self.peak, self.idle_peak) = (peak, idle);
    }

    fn inner_scale(&self, magnitude: f32, peak: f32) -> Option<f32> {
        let levels = self.levels.as_ref()?;
        let unit = peak / self.level_max;
        if self.since_outer < INNER_RUN_SYMBOLS || unit <= 1e-6 {
            return None;
        }
        let decision = levels.level(levels.slice(magnitude / unit)).abs();
        let reliable = (magnitude / unit - decision).abs() < OFFSET_TRUST;
        (reliable && decision > 0.0 && decision < self.level_max)
            .then(|| magnitude * self.level_max / decision)
    }

    fn clear(&mut self) {
        self.sync.reset();
        self.centre = 0.0;
        self.swing_centre = 0.0;
        self.offset = 0.0;
        self.since_outer = 0;
        self.peak = self.level_max;
        self.idle_peak = self.level_max;
        self.gate.reset();
        self.demod_buf.clear();
        self.filtered.clear();
        self.carrier_run.clear();
        self.settled_run.clear();
        self.centred.clear();
        self.retimed.clear();
        self.retimed_carrier.clear();
        self.retimed_settled.clear();
    }
}

fn timing_loop(params: &CpmParams, sps: f64, timing_bw: f64) -> SymbolSync {
    let sync = SymbolSync::new(sps, timing_bw).with_acquisition(ACQUIRE_SYMBOLS);
    if params.mapping().m() > 2 {
        sync.with_levels(params.mapping().levels())
    } else {
        sync
    }
}

#[cfg(test)]
mod tests {

    use sdrmm_modem_test_support::ber::rng::Rng;

    use super::{
        super::{levels::KnownSymbols, modulator::CpmMod, params::Mapping},
        *,
    };
    use crate::pulse::{self, Norm};

    const RATE: f64 = 48_000.0;
    const BAUD: f64 = 4_800.0;
    const SPS: f64 = 10.0;

    fn dibit_mapping() -> Mapping {
        Mapping::new(vec![1.0, 3.0, -1.0, -3.0])
    }

    fn four_level(deviation_hz: f64) -> CpmParams {
        CpmParams::from_deviation(
            dibit_mapping(),
            deviation_hz,
            BAUD,
            pulse::root_raised_cosine(SPS, 0.2, 8, Norm::Area),
            SPS,
        )
    }

    fn rx_rrc() -> Vec<f32> {
        pulse::root_raised_cosine(SPS, 0.2, 8, Norm::Area)
    }

    fn symbols(len: usize, seed: u32, m: u8) -> Vec<u8> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as u8) & (m - 1)
            })
            .collect()
    }

    fn transmit(params: &CpmParams, syms: &[u8]) -> Vec<Complex<f32>> {
        let mut m = CpmMod::new(params.clone());
        let mut out = Vec::new();
        m.modulate(syms, &mut out);
        m.flush(&mut out);
        out
    }

    const NOISE: f32 = 0.01;

    fn noise(seed: u64, len: usize) -> Vec<Complex<f32>> {
        let mut rng = Rng::new(seed);
        (0..len)
            .map(|_| {
                let re = (rng.uniform() as f32 * 2.0 - 1.0) * NOISE;
                let im = (rng.uniform() as f32 * 2.0 - 1.0) * NOISE;
                Complex::new(re, im)
            })
            .collect()
    }

    fn real_noise(seed: u64, len: usize) -> Vec<f32> {
        let mut rng = Rng::new(seed);
        (0..len)
            .map(|_| (rng.uniform() as f32 * 2.0 - 1.0) * NOISE)
            .collect()
    }

    fn listening(demod: &mut CpmDemod, seed: u64) {
        let len = demod.gate.settle_samples + 4 * SPS as usize * 100;
        let quiet = noise(seed, len);
        let mut discard = Vec::new();
        demod.process(&quiet, &mut discard);
    }

    #[test]
    fn a_carrier_that_never_stopped_keys_the_gate_from_a_cold_start() {
        let params = four_level(1_944.0);
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        let iq = transmit(&params, &symbols(4_000, 0x5eed, 4));
        let mut out = Vec::new();

        demod.process(&iq, &mut out);

        let opened = demod
            .settled()
            .iter()
            .position(|&settled| settled)
            .expect("the gate never opened on a carrier that was already running");
        assert!(
            opened < 600,
            "the gate took {opened} symbols to open on a live carrier"
        );
    }

    #[test]
    fn a_dip_inside_a_burst_does_not_restart_the_gate() {
        let params = four_level(1_944.0);
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let threshold = demod.gate.floor * CARRIER_RISE;
        let carrier = 3.0 * threshold;
        for _ in 0..demod.gate.support * 2 {
            demod.gate.step(carrier, None);
        }
        assert!(
            demod.gate.step(carrier, None).1,
            "a steady carrier settles the gate"
        );
        let mut dipped = 0;
        while demod.gate.envelope >= threshold {
            demod.gate.step(0.0, None);
            dipped += 1;
        }
        let mut settled = true;
        for _ in 0..2 {
            settled &= demod.gate.step(0.0, None).1;
        }
        for _ in 0..20 {
            settled &= demod.gate.step(carrier, None).1;
        }
        assert!(settled, "a {}-sample dip restarted the gate", dipped + 2);
    }

    #[test]
    fn noise_alone_never_keys_the_gate() {
        let params = four_level(1_944.0);
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        let mut out = Vec::new();

        demod.process(&noise(0x1157, 96_000), &mut out);

        assert!(
            demod.settled().iter().all(|&settled| !settled),
            "noise on its own was taken for a carrier"
        );
    }

    #[test]
    fn a_carrier_that_stops_shuts_the_gate_and_reopens_it_when_it_returns() {
        let params = four_level(1_944.0);
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        let iq = transmit(&params, &symbols(4_000, 0x5eed, 4));
        let mut out = Vec::new();

        demod.process(&iq, &mut out);
        assert!(demod.settled().iter().any(|&settled| settled));

        demod.process(&noise(0x2468, 96_000), &mut out);
        demod.process(&noise(0x1359, 96_000), &mut out);
        assert!(
            demod.settled().iter().all(|&settled| !settled),
            "the gate stayed open after the carrier went away"
        );

        demod.process(&iq, &mut out);
        assert!(
            demod.settled().iter().any(|&settled| settled),
            "the gate never reopened when the carrier came back"
        );
    }

    fn symbol_errors(got: &[u8], sent: &[u8], skip: usize) -> (usize, usize) {
        let (delay, errors) = (0..48)
            .map(|delay| {
                let errors = got
                    .iter()
                    .enumerate()
                    .skip(skip)
                    .filter(|&(i, s)| sent.get(i.wrapping_sub(delay)).is_none_or(|w| w != s))
                    .count();
                (delay, errors)
            })
            .min_by_key(|&(_, errors)| errors)
            .unwrap();
        assert!((0..40).contains(&delay), "implausible alignment {delay}");
        (errors, got.len() - skip)
    }

    #[test]
    fn the_centre_tracker_reads_back_the_carrier_offset_it_was_given() {
        let sent = symbols(20_000, 23, 4);
        let params = four_level(1_944.0);
        let clean = transmit(&params, &sent);
        let measure = |offset_hz: f64| {
            let shifted: Vec<Complex<f32>> = clean
                .iter()
                .enumerate()
                .map(|(i, &x)| {
                    let phase = TAU * offset_hz * i as f64 / RATE;
                    x * Complex::new(phase.cos() as f32, phase.sin() as f32)
                })
                .collect();
            let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
            listening(&mut demod, 0x2211);
            let mut soft = Vec::new();
            demod.process(&shifted, &mut soft);
            demod.frequency_error_cycles_per_sample() * RATE
        };

        let base = measure(0.0);
        for offset_hz in [-600.0f64, -200.0, 200.0, 600.0] {
            let moved = measure(offset_hz) - base;
            assert!(
                (moved - offset_hz).abs() < 20.0,
                "a {offset_hz} Hz shift moved the tracker by {moved} Hz"
            );
        }
    }

    #[test]
    fn recovers_four_level_symbols_at_an_unexpected_deviation() {
        for deviation in [1_944.0, 1_400.0, 2_600.0] {
            let sent = symbols(400, 17, 4);
            let iq = transmit(&four_level(deviation), &sent);
            let nominal = four_level(1_944.0);
            let mut demod = CpmDemod::new(&nominal, &rx_rrc(), TIMING_BW_BURST);
            listening(&mut demod, 0x1157);
            let mut soft = Vec::new();
            demod.process(&iq, &mut soft);
            let got: Vec<u8> = soft.iter().map(|&s| nominal.mapping().slice(s)).collect();
            let (errors, total) = symbol_errors(&got, &sent, 100);
            assert!(total > 280, "only {total} symbols at {deviation} Hz");
            assert_eq!(errors, 0, "symbol errors at {deviation} Hz deviation");
        }
    }

    #[test]
    fn a_noisy_four_level_burst_locks_within_a_few_hundred_symbols() {
        let sent = symbols(600, 0x5eed, 4);
        let params = four_level(1_944.0);
        let clean = transmit(&params, &sent);
        let mut errors = 0;
        for delay in 0..SPS as usize {
            let mut iq = noise(0x77, delay);
            iq.extend_from_slice(&clean);
            let mut rng = Rng::new(7 + delay as u64);
            for sample in &mut iq {
                let re = (rng.uniform() as f32 * 2.0 - 1.0) * 0.5;
                let im = (rng.uniform() as f32 * 2.0 - 1.0) * 0.5;
                *sample += Complex::new(re, im);
            }
            let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
            listening(&mut demod, 0x1157);
            let mut soft = Vec::new();
            demod.process(&iq, &mut soft);
            let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
            let (e, t) = symbol_errors(&got[..400], &sent, 100);
            assert_eq!(t, 300);
            errors += e;
        }
        assert!(
            errors <= 100,
            "{errors} symbol errors after a hundred symbols"
        );
    }

    #[test]
    fn a_long_run_of_one_level_does_not_pull_the_slicer_off_centre() {
        let mut sent = symbols(400, 0x77, 4);
        for _ in 0..4 {
            sent.extend(std::iter::repeat_n(0u8, 150));
            sent.extend(symbols(40, 0x99, 4));
        }
        let params = four_level(1_944.0);
        let iq = transmit(&params, &sent);
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let (errors, total) = symbol_errors(&got, &sent, 300);
        assert!(total > 850, "only {total} symbols");
        assert!(errors <= 2, "{errors} symbol errors in {total}");
    }

    #[test]
    fn tracks_a_carrier_offset() {
        let sent = symbols(900, 5, 4);
        let params = four_level(1_944.0);
        let mut iq = transmit(&params, &sent);
        for (k, s) in iq.iter_mut().enumerate() {
            *s *= Complex::from_polar(1.0, (TAU * 400.0 * k as f64 / RATE) as f32);
        }
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let (errors, _) = symbol_errors(&got, &sent, soft.len() - 200);
        assert_eq!(errors, 0);
    }

    #[test]
    fn block_splits_do_not_change_the_symbols() {
        let sent = symbols(300, 41, 4);
        let params = four_level(1_944.0);
        let iq = transmit(&params, &sent);
        let mut whole = Vec::new();
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        demod.process(&iq, &mut whole);

        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut ragged = Vec::new();
        let mut pos = 0;
        for len in [997usize, 1, 4_096, 65, 7].iter().cycle() {
            if pos >= iq.len() {
                break;
            }
            let end = (pos + len).min(iq.len());
            demod.process(&iq[pos..end], &mut ragged);
            pos = end;
        }
        assert_eq!(whole, ragged);
    }

    #[test]
    fn a_keyed_transmitter_does_not_lose_its_clock_in_the_dead_time() {
        const ON: usize = 132;
        const FRAME: usize = 288;
        let params = four_level(1_944.0);
        let sent = symbols(2_880, 23, 4);
        let keyed: Vec<Option<u8>> = sent
            .iter()
            .enumerate()
            .map(|(i, &s)| (i % FRAME < ON).then_some(s))
            .collect();
        let mut iq = CpmMod::new(params.clone()).keyed(&keyed);
        let floor = noise(0xbeef, iq.len());
        for (s, n) in iq.iter_mut().zip(floor) {
            *s += n;
        }
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);

        let ideal = iq.len() / SPS as usize;
        assert!(
            (soft.len() as i64 - ideal as i64).abs() <= 2,
            "recovered {} symbols, ideal {ideal}",
            soft.len()
        );

        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let (delay, _) = (0..48)
            .map(|delay| {
                let errors = (300usize..400)
                    .filter(|&i| sent.get(i.wrapping_sub(delay)).is_none_or(|w| *w != got[i]))
                    .count();
                (delay, errors)
            })
            .min_by_key(|&(_, errors)| errors)
            .unwrap();
        let last = sent.len() - FRAME + delay;
        let bad: Vec<usize> = (last..last + ON)
            .filter(|&i| sent.get(i - delay).is_none_or(|w| *w != got[i]))
            .map(|i| i - last)
            .collect();
        assert!(
            bad.is_empty(),
            "symbol errors at {bad:?} in the last of {} bursts",
            sent.len() / FRAME
        );
    }

    #[test]
    fn the_first_burst_is_clean_at_any_timing_phase() {
        const ON: usize = 132;
        let params = four_level(1_944.0);
        let sent = symbols(ON, 23, 4);
        let mut keyed = vec![None; 40];
        keyed.extend(sent.iter().copied().map(Some));
        keyed.extend(vec![None; 40]);
        let burst = CpmMod::new(params.clone()).keyed(&keyed);
        for delay in 0..SPS as usize {
            let mut iq = noise(0x99, delay);
            iq.extend_from_slice(&burst);
            let floor = noise(0xbeef, iq.len());
            for (s, n) in iq.iter_mut().zip(floor) {
                *s += n;
            }
            let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
            listening(&mut demod, 0x1157);
            let mut soft = Vec::new();
            demod.process(&iq, &mut soft);
            let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
            let errors = (0..soft.len())
                .map(|lag| {
                    (1..ON)
                        .filter(|&i| got.get(i + lag) != Some(&sent[i]))
                        .count()
                })
                .min()
                .unwrap_or(ON);
            assert_eq!(errors, 0, "symbol errors at a delay of {delay} samples");
        }
    }

    #[test]
    fn a_continuous_stream_holds_lock_over_twenty_thousand_symbols() {
        let sent = symbols(20_000, 0x5eed, 4);
        let params = four_level(1_944.0);
        let iq = transmit(&params, &sent);
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_CONTINUOUS);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let (errors, total) = symbol_errors(&got, &sent, 500);
        assert!(total > 19_000, "only {total} symbols recovered");
        assert!(
            errors <= total / 1_000,
            "{errors} symbol errors in {total}: the continuous floor is back"
        );
    }

    #[test]
    fn gmsk_loopback_is_clean() {
        let params = CpmParams::from_h(
            Mapping::natural(2),
            0.5,
            pulse::gaussian_freq(SPS, 0.5, 3, Norm::Area),
            SPS,
        );
        let sent = symbols(600, 71, 2);
        let iq = transmit(&params, &sent);
        let mut demod = CpmDemod::new(
            &params,
            &pulse::gaussian(SPS, 0.5, 3, Norm::Area),
            TIMING_BW_BURST,
        );
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let (errors, total) = symbol_errors(&got, &sent, 100);
        assert!(total > 400);
        assert_eq!(errors, 0, "GMSK symbol errors");
    }

    #[test]
    fn two_level_cpfsk_loopback_is_clean() {
        let params = CpmParams::from_h(Mapping::natural(2), 0.5, pulse::rect(SPS, Norm::Area), SPS);
        let sent = symbols(600, 29, 2);
        let iq = transmit(&params, &sent);
        let mut demod = CpmDemod::new(&params, &pulse::rect(SPS, Norm::Area), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let (errors, total) = symbol_errors(&got, &sent, 100);
        assert!(total > 400);
        assert_eq!(errors, 0, "2FSK symbol errors");
    }

    #[test]
    fn eight_level_loopback_is_clean_on_the_known_symbol_hook() {
        const PATTERN: [u8; 16] = [7, 0, 5, 2, 6, 1, 4, 3, 0, 7, 3, 4, 1, 6, 2, 5];
        const PERIOD: usize = 128;
        const FRAMES: usize = 12;
        let params = CpmParams::from_h(Mapping::natural(8), 0.3, pulse::rect(SPS, Norm::Area), SPS);
        let mut sent = Vec::with_capacity(FRAMES * PERIOD);
        for frame in 0..FRAMES {
            sent.extend_from_slice(&PATTERN);
            sent.extend(symbols(PERIOD - PATTERN.len(), 0x0dd5 ^ frame as u32, 8));
        }
        let iq = transmit(&params, &sent);
        let mut demod = CpmDemod::new(&params, &pulse::rect(SPS, Norm::Area), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);

        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let (delay, _) = (0..48usize)
            .map(|d| {
                let errs = (200..1_000usize)
                    .filter(|&i| {
                        sent.get(i.wrapping_sub(d))
                            .is_none_or(|w| got.get(i).is_none_or(|g| g != w))
                    })
                    .count();
                (d, errs)
            })
            .min_by_key(|&(_, e)| e)
            .unwrap();
        let mut hook = KnownSymbols::new(&params, (4 * PERIOD) as u32);
        let mut errors = Vec::new();
        for frame in 1..FRAMES {
            let base = frame * PERIOD + delay;
            hook.anchor(&PATTERN, &soft[base..base + PATTERN.len()]);
            for k in PATTERN.len()..PERIOD {
                hook.tick();
                let Some(&s) = soft.get(base + k) else {
                    continue;
                };
                if params.mapping().slice(hook.correct(s)) != sent[frame * PERIOD + k] {
                    errors.push(frame * PERIOD + k);
                }
            }
        }
        assert!(
            errors.is_empty(),
            "8-level symbol errors at {errors:?} through the hook"
        );
    }

    #[test]
    fn a_silent_channel_produces_finite_symbols() {
        let params = four_level(1_944.0);
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        let mut soft = Vec::new();
        demod.process(&vec![Complex::new(0.0, 0.0); 4_800], &mut soft);
        assert!(!soft.is_empty());
        assert!(soft.iter().all(|s| s.is_finite()), "non-finite symbol");
    }

    fn afsk_params() -> CpmParams {
        CpmParams::from_deviation(
            Mapping::new(vec![1.0, -1.0]),
            500.0,
            1_200.0,
            pulse::rect(40.0, Norm::Area),
            40.0,
        )
    }

    fn afsk_rx() -> Vec<f32> {
        pulse::rect(20.0, Norm::Area)
    }

    fn afsk_audio(sent: &[u8]) -> Vec<f32> {
        let baseband = transmit(&afsk_params(), sent);
        let mut carrier = Nco::new(1_700.0, RATE as f32);
        baseband
            .iter()
            .map(|&s| (s * carrier.next_sample()).re)
            .collect()
    }

    fn afsk_roundtrip(detector: RealDetector, receive_filter: &[f32], seed: u32) {
        let params = afsk_params();
        let sent = symbols(500, seed, 2);
        let audio = afsk_audio(&sent);
        let mut demod = CpmDemod::real(&params, receive_filter, TIMING_BW_BURST, RATE, detector);
        let quiet = real_noise(0x1157, demod.gate.settle_samples + 19_200);
        let mut discard = Vec::new();
        demod.process_real(&quiet, &mut discard);
        let mut soft = Vec::new();
        demod.process_real(&audio, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let (errors, total) = symbol_errors(&got, &sent, 80);
        assert!(total > 350, "only {total} symbols");
        assert_eq!(errors, 0, "AFSK bit errors with {detector:?}");
    }

    #[test]
    fn afsk_decodes_through_the_tone_filterbank() {
        afsk_roundtrip(
            RealDetector::ToneFilterbank {
                plus_hz: 2_200.0,
                minus_hz: 1_200.0,
            },
            &afsk_rx(),
            33,
        );
    }

    #[test]
    fn afsk_decodes_through_the_analytic_discriminator() {
        afsk_roundtrip(
            RealDetector::Discriminator { centre_hz: 1_700.0 },
            &pulse::rect(40.0, Norm::Area),
            34,
        );
    }

    #[test]
    fn real_block_splits_do_not_change_the_symbols() {
        let params = afsk_params();
        let sent = symbols(200, 9, 2);
        let audio = afsk_audio(&sent);
        let detector = RealDetector::ToneFilterbank {
            plus_hz: 2_200.0,
            minus_hz: 1_200.0,
        };
        let filter = afsk_rx();
        let mut whole = Vec::new();
        let mut demod = CpmDemod::real(&params, &filter, TIMING_BW_BURST, RATE, detector);
        demod.process_real(&audio, &mut whole);

        let mut demod = CpmDemod::real(&params, &filter, TIMING_BW_BURST, RATE, detector);
        let mut ragged = Vec::new();
        let mut pos = 0;
        for len in [997usize, 1, 4_096, 65, 7].iter().cycle() {
            if pos >= audio.len() {
                break;
            }
            let end = (pos + len).min(audio.len());
            demod.process_real(&audio[pos..end], &mut ragged);
            pos = end;
        }
        assert_eq!(whole, ragged);
    }

    #[test]
    fn a_silent_audio_channel_produces_finite_symbols() {
        let params = afsk_params();
        let mut demod = CpmDemod::real(
            &params,
            &pulse::rect(40.0, Norm::Area),
            TIMING_BW_BURST,
            RATE,
            RealDetector::Discriminator { centre_hz: 1_700.0 },
        );
        let mut soft = Vec::new();
        demod.process_real(&vec![0.0; 48_000], &mut soft);
        assert!(!soft.is_empty());
        assert!(soft.iter().all(|s| s.is_finite()), "non-finite symbol");
    }

    #[test]
    fn each_construction_reads_only_the_domain_it_was_built_for() {
        let mut real = CpmDemod::real(
            &afsk_params(),
            &pulse::rect(40.0, Norm::Area),
            TIMING_BW_BURST,
            RATE,
            RealDetector::Discriminator { centre_hz: 1_700.0 },
        );
        let mut soft = Vec::new();
        real.process_real(&[0.0; 4_096], &mut soft);
        real.reset();

        let mut iq = CpmDemod::new(&four_level(1_944.0), &rx_rrc(), TIMING_BW_BURST);
        iq.process(&[Complex::new(0.0, 0.0); 4_096], &mut soft);
        iq.reset();
    }

    #[test]
    #[should_panic(expected = "two levels")]
    fn a_filterbank_needs_a_two_level_mapping() {
        let params = four_level(1_944.0);
        let _ = CpmDemod::real(
            &params,
            &rx_rrc(),
            TIMING_BW_BURST,
            RATE,
            RealDetector::ToneFilterbank {
                plus_hz: 2_200.0,
                minus_hz: 1_200.0,
            },
        );
    }

    #[test]
    fn complex_process_steady_state_allocates_nothing() {
        let params = four_level(1_944.0);
        let iq = transmit(&params, &symbols(1_200, 0x5eed, 4));
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        let mut soft = Vec::with_capacity(iq.len());
        demod.process(&iq, &mut soft);
        soft.clear();
        demod.process(&iq, &mut soft);
        soft.clear();
        sdrmm_modem_test_support::ber::perf::assert_no_alloc("CpmDemod::process", || {
            demod.process(&iq, &mut soft)
        });
        assert!(!soft.is_empty(), "the measured call recovered no symbols");
    }

    #[test]
    fn real_process_steady_state_allocates_nothing() {
        let params = afsk_params();
        let audio = afsk_audio(&symbols(300, 0x0dd5, 2));
        let mut demod = CpmDemod::real(
            &params,
            &afsk_rx(),
            TIMING_BW_BURST,
            RATE,
            RealDetector::ToneFilterbank {
                plus_hz: 2_200.0,
                minus_hz: 1_200.0,
            },
        );
        let mut soft = Vec::with_capacity(audio.len());
        demod.process_real(&audio, &mut soft);
        soft.clear();
        demod.process_real(&audio, &mut soft);
        soft.clear();
        sdrmm_modem_test_support::ber::perf::assert_no_alloc("CpmDemod::process_real", || {
            demod.process_real(&audio, &mut soft);
        });
        assert!(!soft.is_empty(), "the measured call recovered no symbols");
    }

    #[test]
    fn a_noise_floor_that_rises_with_a_burst_does_not_hold_the_gate_open() {
        const ON: usize = 132;
        const OFF: usize = 156;
        let params = four_level(1_944.0);
        for amplitude in [3.0 * NOISE, 10.0 * NOISE, 30.0 * NOISE] {
            let mut keyed: Vec<Option<u8>> = Vec::new();
            let mut sent = Vec::new();
            for burst in 0..6u32 {
                let syms = symbols(ON, 23 + burst, 4);
                keyed.extend(syms.iter().copied().map(Some));
                keyed.extend(vec![None; OFF]);
                sent.push(syms);
            }
            let mut iq = CpmMod::new(params.clone()).keyed(&keyed);
            let mut rng = Rng::new(7);
            for s in &mut iq {
                *s += Complex::new(
                    (rng.uniform() as f32 * 2.0 - 1.0) * amplitude,
                    (rng.uniform() as f32 * 2.0 - 1.0) * amplitude,
                );
            }
            let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
            listening(&mut demod, 0x1157);
            let mut soft = Vec::new();
            demod.process(&iq, &mut soft);
            let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
            for (index, burst) in sent.iter().enumerate() {
                let errors = (0..got.len().saturating_sub(ON))
                    .map(|lag| (8..ON).filter(|&i| got[lag + i] != burst[i]).count())
                    .min()
                    .unwrap_or(ON);
                assert_eq!(errors, 0, "noise {amplitude}: burst {index}");
            }
            let open = demod.settled().iter().filter(|&&s| s).count() as f32
                / demod.settled().len() as f32;
            assert!(open < 0.5, "noise {amplitude}: the gate stayed open {open}");
        }
    }
}
