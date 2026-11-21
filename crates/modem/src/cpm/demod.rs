mod gate;
mod offset;
#[cfg(test)]
mod tests;

use num_complex::Complex;
use sdrmm_dsp::{
    Decimator, FmDemod, Nco, RealDecimator, SymbolSync, ToneCorrelator, design_lowpass,
    one_pole_coeff,
};

use self::{gate::Gate, offset::OffsetTracker};
use super::{
    mapping::Mapping,
    params::{CpmParams, assert_unit_area},
};

pub const TIMING_BW_BURST: f64 = 0.015;

pub const TIMING_BW_CONTINUOUS: f64 = 0.003;

const CENTRE_SYMBOLS_FLOOR: f32 = 150.0;

const CENTRE_POWER_SYMBOLS: f32 = 30.0;

const PEAK_SYMBOLS: f32 = 60.0;

const PEAK_HOLD_SYMBOLS: f32 = 4.0 * PEAK_SYMBOLS;

const PEAK_ATTACK: f32 = 0.125;

/// How far past the outer symbol level the discriminator must swing to count as wild. A carrier
/// stays inside its deviation; noise of any strength throws the phase around and lands out here
/// far more often, which power alone cannot tell from a weak carrier.
const WILD_LEVELS: f32 = 2.0;

/// How quickly the centre wild swings are measured from follows the discriminator, so a
/// carrier offset the loops have not caught up with does not read as wild.
const SWING_TAU_SYMBOLS: f64 = 4.0;

const OFFSET_SYMBOLS: f32 = 64.0;

const OFFSET_TRUST: f32 = 0.5;

const INNER_RUN_SYMBOLS: usize = 16;

const IMAGE_TAPS: usize = 127;

/// Symbols each burst's opening timing estimate spans: half a DMR burst.
const ACQUIRE_SYMBOLS: usize = 64;

/// Smallest symbol unit the slicer divides by; below it the output is zero.
const MIN_UNIT: f32 = 1e-6;

const MIN_TONE_WINDOW: usize = 2;

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

pub struct CpmDemod<F = IqFront> {
    front: F,
    matched: RealDecimator,
    sync: SymbolSync,
    level_max: f32,
    inv_level_max: f32,
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
    centred: Vec<f32>,
    retimed: Vec<f32>,
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
        let window = (sps.round() as usize).max(MIN_TONE_WINDOW);
        let shift = params.h() / 2.0;
        let front = IqFront(IqKind::Tones(Box::new(ToneFront {
            plus: ToneCorrelator::complex(sps, shift, window),
            minus: ToneCorrelator::complex(sps, -shift, window),
            tracker: OffsetTracker::new(shift, sps),
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
                mixed.resize(audio.len(), Complex::new(0.0, 0.0));
                mixer.mix_real_into(audio, mixed);
                image.process(mixed, baseband);
                demod.process(baseband, &mut self.demod_buf);
            }
            RealKind::Filterbank { plus, minus } => {
                ToneCorrelator::process_pair_diff(plus, minus, audio, &mut self.demod_buf);
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
        assert_unit_area(receive_filter, "receive filter");
        let sps = params.sps();
        let level_max = params.mapping().max_level();
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
            inv_level_max: level_max.recip(),
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
            gate: Gate::new(sps, receive_filter.len() + front_support),
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

    fn gate_run(&mut self, powers: impl ExactSizeIterator<Item = (f32, Option<bool>)>) {
        let len = powers.len();
        self.carrier_run.resize(len, false);
        self.settled_run.resize(len, false);
        if self.gate.always_keyed {
            self.carrier_run.fill(true);
            let need = self.gate.support.saturating_sub(self.gate.keyed);
            if need == 0 {
                self.settled_run.fill(true);
            } else {
                let unsettled = (need - 1).min(len);
                self.settled_run[..unsettled].fill(false);
                self.settled_run[unsettled..].fill(true);
                self.gate.keyed = (self.gate.keyed + len).min(self.gate.support);
            }
            return;
        }
        let mut gate = self.gate;
        for ((carrier, settled), (power, wild)) in self
            .carrier_run
            .iter_mut()
            .zip(&mut self.settled_run)
            .zip(powers)
        {
            (*carrier, *settled) = gate.step(power, wild);
        }
        self.gate = gate;
    }

    fn finish(&mut self, out: &mut Vec<f32>) {
        self.matched.process(&self.demod_buf, &mut self.filtered);
        self.centre_run();
        self.retime();
        self.normalise(out);
    }

    fn centre_run(&mut self) {
        let len = self.filtered.len().min(self.settled_run.len());
        self.centred.resize(len, 0.0);
        let mut centre = self.centre;
        let alpha = self.centre_alpha;
        for ((dst, &sample), &settled) in self
            .centred
            .iter_mut()
            .zip(&self.filtered[..len])
            .zip(&self.settled_run[..len])
        {
            if settled {
                centre += alpha * (sample - centre);
            }
            *dst = sample - centre;
        }
        self.centre = centre;
    }

    /// Retimes each run of samples that share one gate state, holding the clock while unsettled.
    fn retime(&mut self) {
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
                self.sync.process_real(run, &mut self.retimed);
            } else {
                self.sync.process_real_held(run, &mut self.retimed);
            }
            self.retimed_carrier.resize(self.retimed.len(), carrier);
            self.retimed_settled.resize(self.retimed.len(), settled);
            start = end;
        }
    }

    fn normalise(&mut self, out: &mut Vec<f32>) {
        const INV_PEAK_SYMBOLS: f32 = 1.0 / PEAK_SYMBOLS;
        const INV_OFFSET_SYMBOLS: f32 = 1.0 / OFFSET_SYMBOLS;
        out.reserve(self.retimed.len());
        let (mut peak, mut idle) = (self.peak, self.idle_peak);
        let inv_level_max = self.inv_level_max;
        let wild_bound = WILD_LEVELS * self.level_max;
        let carriers = self.retimed_carrier.iter().zip(&self.retimed_settled);
        for (&symbol, (&carrier, &settled)) in self.retimed.iter().zip(carriers) {
            let value = symbol - self.offset;
            let magnitude = value.abs();
            if carrier {
                if settled && (self.levels.is_none() || magnitude <= wild_bound) {
                    self.since_outer += 1;
                    if magnitude > peak {
                        peak += PEAK_ATTACK * (magnitude - peak);
                        self.since_outer = 0;
                    } else if magnitude > peak * self.outer_region {
                        peak += (magnitude - peak) * INV_PEAK_SYMBOLS;
                        self.since_outer = 0;
                    } else if let Some(scaled) = self.inner_scale(magnitude, peak) {
                        peak += (scaled - peak) * INV_PEAK_SYMBOLS;
                    } else {
                        peak *= self.peak_hold_decay;
                    }
                }
            } else if magnitude > idle {
                idle += PEAK_ATTACK * (magnitude - idle);
            } else {
                idle *= self.peak_decay;
            }
            let level = if carrier { peak } else { idle };
            let unit = level * inv_level_max;
            let normalised = if unit > MIN_UNIT { value / unit } else { 0.0 };
            if carrier
                && settled
                && let Some(levels) = &self.levels
            {
                let miss = normalised - levels.level(levels.slice(normalised));
                if miss.abs() < OFFSET_TRUST {
                    self.offset =
                        (self.offset + miss * unit * INV_OFFSET_SYMBOLS).clamp(-unit, unit);
                }
            }
            out.push(normalised);
        }
        (self.peak, self.idle_peak) = (peak, idle);
    }

    fn inner_scale(&self, magnitude: f32, peak: f32) -> Option<f32> {
        let levels = self.levels.as_ref()?;
        let unit = peak * self.inv_level_max;
        if self.since_outer < INNER_RUN_SYMBOLS || unit <= MIN_UNIT {
            return None;
        }
        let scaled_mag = magnitude / unit;
        let decision = levels.level(levels.slice(scaled_mag)).abs();
        let reliable = (scaled_mag - decision).abs() < OFFSET_TRUST;
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
