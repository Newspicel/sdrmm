mod blind;
mod config;
mod delay_line;
mod step;

use num_complex::Complex;

use blind::BlindTarget;
pub use config::{EqualiserConfig, EqualiserError};
use delay_line::DelayLine;
use step::StepControl;

use super::carrier::{CarrierLoop, PhaseDetector};
use crate::constellation::Constellation;

/// Taps per symbol: the equaliser runs at half-symbol spacing.
pub(super) const TAPS_PER_SYMBOL: usize = 2;

const ZERO: Complex<f32> = Complex::new(0.0, 0.0);

const ONE: Complex<f32> = Complex::new(1.0, 0.0);

const POWER_MEMORY_SAMPLES: f32 = 8_192.0;

const MISFIT_SYMBOLS: f32 = 128.0;

const TRUST_MISFIT: f32 = 0.1;

const DISTRUST_MISFIT: f32 = 0.13;

const OUTPUT_MEMORY_SYMBOLS: f32 = 256.0;

const MAX_OUTPUT_POWER: f32 = 6.0;

const MIN_OUTPUT_POWER: f32 = 0.05;

const LEAKAGE: f32 = 1e-5;

const ENERGY_FLOOR: f32 = 1e-6;

/// A fractionally spaced, optionally decision-feedback equaliser that starts blind and turns
/// decision-directed once its output sits on the table.
#[derive(Clone, Debug)]
pub struct Equaliser {
    config: EqualiserConfig,
    blind: BlindTarget,
    blind_detector: PhaseDetector,
    spacing2: f32,
    forward: Vec<Complex<f32>>,
    feedback: Vec<Complex<f32>>,
    line: DelayLine,
    decisions: DelayLine,
    centre: usize,
    power: f32,
    seen: f32,
    misfit: f32,
    output_power: f32,
    trusted: bool,
    step: StepControl,
    resets: u64,
    slips: u64,
}

impl Equaliser {
    pub fn new(config: EqualiserConfig, table: &Constellation) -> Result<Self, EqualiserError> {
        config.validate()?;
        let taps = TAPS_PER_SYMBOL * config.span_symbols;
        let spacing = table.min_distance().max(f64::from(ENERGY_FLOOR)) as f32;
        let mut eq = Self {
            config,
            blind: BlindTarget::for_table(table),
            blind_detector: PhaseDetector::MthPower {
                m: table.rotational_order().max(2),
            },
            spacing2: spacing * spacing,
            forward: vec![ZERO; taps],
            feedback: vec![ZERO; config.feedback_symbols],
            line: DelayLine::new(taps),
            decisions: DelayLine::new(config.feedback_symbols),
            centre: TAPS_PER_SYMBOL * (config.span_symbols / 2),
            power: 0.0,
            seen: 0.0,
            misfit: 1.0,
            output_power: 1.0,
            trusted: false,
            step: StepControl::default(),
            resets: 0,
            slips: 0,
        };
        eq.reset();
        Ok(eq)
    }

    #[must_use]
    pub fn config(&self) -> &EqualiserConfig {
        &self.config
    }

    #[must_use]
    pub fn delay_symbols(&self) -> usize {
        self.centre / TAPS_PER_SYMBOL
    }

    #[must_use]
    pub fn decision_directed(&self) -> bool {
        self.trusted
    }

    #[must_use]
    pub fn resets(&self) -> u64 {
        self.resets
    }

    #[must_use]
    pub fn slips(&self) -> u64 {
        self.slips
    }

    #[must_use]
    pub fn taps(&self) -> &[Complex<f32>] {
        &self.forward
    }

    pub fn push(&mut self, x: Complex<f32>) {
        let x = if x.re.is_finite() && x.im.is_finite() {
            x
        } else {
            ZERO
        };
        self.seen = (self.seen + 1.0).min(POWER_MEMORY_SAMPLES);
        self.power += (x.norm_sqr() - self.power) / self.seen;
        let scale = self.power.max(ENERGY_FLOOR).sqrt().recip();
        self.line.push(x * scale);
    }

    pub fn symbol(
        &mut self,
        carrier: Option<&mut CarrierLoop>,
        table: &Constellation,
    ) -> Complex<f32> {
        let rotation = carrier.as_ref().map_or(ONE, |c| c.rotation());
        let z = dot(&self.forward, self.line.window()) * rotation
            + dot(&self.feedback, self.decisions.window());
        let decision = table.nearest(z);
        self.judge(z, decision);
        self.adapt(z, decision, rotation);
        if let Some(carrier) = carrier {
            if self.trusted {
                carrier.steer(z, table);
            } else {
                carrier.steer_with(self.blind_detector, z, table);
            }
        }
        self.decisions
            .push(if self.trusted { decision } else { ZERO });
        self.guard(z);
        self.recentre();
        z
    }

    fn recentre(&mut self) {
        let drift = self.timing_error_symbols();
        if drift > 1.0 {
            self.forward.rotate_left(TAPS_PER_SYMBOL);
            self.forward
                .iter_mut()
                .rev()
                .take(TAPS_PER_SYMBOL)
                .for_each(|w| *w = ZERO);
            self.slips += 1;
        } else if drift < -1.0 {
            self.forward.rotate_right(TAPS_PER_SYMBOL);
            self.forward
                .iter_mut()
                .take(TAPS_PER_SYMBOL)
                .for_each(|w| *w = ZERO);
            self.slips += 1;
        }
    }

    #[must_use]
    pub fn timing_error_symbols(&self) -> f64 {
        let mut energy = 0.0f64;
        let mut moment = 0.0f64;
        for (k, w) in self.forward.iter().enumerate() {
            let e = f64::from(w.norm_sqr());
            energy += e;
            moment += e * k as f64;
        }
        if energy <= f64::from(ENERGY_FLOOR) {
            return 0.0;
        }
        (moment / energy - self.centre as f64) / TAPS_PER_SYMBOL as f64
    }

    pub fn equalise_burst(
        &mut self,
        halves: &[Complex<f32>],
        carrier: Option<&CarrierLoop>,
        table: &Constellation,
        out: &mut Vec<Complex<f32>>,
    ) {
        self.hold_power(halves);
        for _ in 1..self.config.burst_passes {
            let start = out.len();
            self.run_pass(halves, carrier, table, out);
            out.truncate(start);
        }
        self.run_pass(halves, carrier, table, out);
    }

    fn hold_power(&mut self, halves: &[Complex<f32>]) {
        let finite = halves
            .iter()
            .filter(|x| x.re.is_finite() && x.im.is_finite());
        let count = finite.clone().count();
        if count == 0 {
            return;
        }
        self.power = finite.map(Complex::norm_sqr).sum::<f32>() / count as f32;
        self.seen = POWER_MEMORY_SAMPLES;
    }

    fn run_pass(
        &mut self,
        halves: &[Complex<f32>],
        carrier: Option<&CarrierLoop>,
        table: &Constellation,
        out: &mut Vec<Complex<f32>>,
    ) {
        let mut carrier = carrier.cloned();
        self.line.clear();
        self.decisions.clear();
        let delay = self.delay_symbols();
        let mut emitted = 0usize;
        let mut emit = |eq: &mut Self, carrier: &mut Option<CarrierLoop>| {
            let rotation = carrier.as_ref().map_or(ONE, CarrierLoop::rotation);
            let z = eq.symbol(carrier.as_mut(), table);
            if emitted >= delay {
                out.push(z * rotation.conj());
            }
            emitted += 1;
        };
        for pair in halves.chunks(TAPS_PER_SYMBOL) {
            self.push(pair[0]);
            emit(self, &mut carrier);
            if let Some(&mid) = pair.get(1) {
                self.push(mid);
            }
        }
        for _ in 0..delay {
            self.line.push(ZERO);
            emit(self, &mut carrier);
            self.line.push(ZERO);
        }
    }

    pub fn reset(&mut self) {
        self.forward.fill(ZERO);
        if let Some(tap) = self.forward.get_mut(self.centre) {
            *tap = ONE;
        }
        self.feedback.fill(ZERO);
        self.line.clear();
        self.decisions.clear();
        self.power = 0.0;
        self.seen = 0.0;
        self.misfit = 1.0;
        self.output_power = 1.0;
        self.trusted = false;
        self.step = StepControl::default();
    }

    fn judge(&mut self, z: Complex<f32>, decision: Complex<f32>) {
        let miss = (z - decision).norm_sqr() / self.spacing2;
        self.misfit += (miss.min(1.0) - self.misfit) / MISFIT_SYMBOLS;
        let was = self.trusted;
        self.trusted = if was {
            self.misfit < DISTRUST_MISFIT
        } else {
            self.misfit < TRUST_MISFIT
        };
        if was && !self.trusted {
            self.feedback.fill(ZERO);
        }
        if !was && self.trusted {
            self.step = StepControl::default();
        }
    }

    fn adapt(&mut self, z: Complex<f32>, decision: Complex<f32>, rotation: Complex<f32>) {
        let (error, step) = if self.trusted {
            let error = decision - z;
            let max = self.config.decision_step;
            (error, self.step.next(error / self.spacing2.sqrt(), max))
        } else {
            (self.blind.error(z, decision), self.config.blind_step)
        };
        let expected = 0.5 * (self.forward.len() + self.feedback.len()) as f32;
        let energy = energy(self.line.window()) + energy(self.decisions.window());
        let gain = step / energy.max(expected);
        let back = error * rotation.conj() * gain;
        for (w, x) in self.forward.iter_mut().zip(self.line.window()) {
            *w = *w * (1.0 - LEAKAGE) + back * x.conj();
        }
        if self.trusted {
            let forward = error * gain;
            for (b, d) in self.feedback.iter_mut().zip(self.decisions.window()) {
                *b += forward * d.conj();
            }
        }
    }

    fn guard(&mut self, z: Complex<f32>) {
        self.output_power += (z.norm_sqr() - self.output_power) / OUTPUT_MEMORY_SYMBOLS;
        let finite = self.output_power.is_finite()
            && self
                .forward
                .iter()
                .all(|w| w.re.is_finite() && w.im.is_finite());
        if !finite || !(MIN_OUTPUT_POWER..=MAX_OUTPUT_POWER).contains(&self.output_power) {
            self.resets += 1;
            self.reset();
        }
    }
}

fn dot(taps: &[Complex<f32>], window: &[Complex<f32>]) -> Complex<f32> {
    taps.iter().zip(window).map(|(w, x)| w * x).sum()
}

fn energy(window: &[Complex<f32>]) -> f32 {
    window.iter().map(Complex::norm_sqr).sum()
}

#[cfg(test)]
mod tests;
