use std::f64::consts::{FRAC_1_SQRT_2, TAU};

use num_complex::Complex;

use super::{
    acquisition::{Acquisition, symbol_peak},
    decisions::Decisions,
    farrow::farrow,
};
use crate::demod::pll::loop_gains;

const ZERO: Complex<f32> = Complex::new(0.0, 0.0);
const TRACKING_RANGE: f64 = 0.05;
const FREE_RUN_MEMORY_SYMBOLS: f64 = 1024.0;
const SYMMETRY_TOLERANCE: f64 = 0.15;

pub struct SymbolSync {
    nominal_sps: f64,
    sps: f64,
    free_run_sps: f64,
    step: f64,
    alpha: f64,
    beta: f64,
    pos: usize,
    frac: f64,
    consumed: usize,
    buf: Vec<Complex<f32>>,
    at_symbol: bool,
    prev_symbol: Complex<f32>,
    mid: Complex<f32>,
    primed: bool,
    decisions: Option<Decisions>,
    acquisition: Option<Acquisition>,
}

impl SymbolSync {
    #[must_use]
    pub fn new(sps: f64, loop_bw: f64) -> Self {
        assert!(
            sps.is_finite() && sps >= 2.0,
            "sps must be at least 2 samples per symbol"
        );
        assert!(
            loop_bw.is_finite() && loop_bw > 0.0 && loop_bw < 1.0,
            "loop_bw must be in (0, 1) cycles per symbol"
        );
        let (alpha, beta) = loop_gains(loop_bw, FRAC_1_SQRT_2);
        let mut sync = Self {
            nominal_sps: sps,
            sps,
            free_run_sps: sps,
            step: sps,
            alpha,
            beta,
            pos: 0,
            frac: 0.0,
            consumed: 0,
            buf: Vec::new(),
            at_symbol: true,
            prev_symbol: ZERO,
            mid: ZERO,
            primed: false,
            decisions: None,
            acquisition: None,
        };
        sync.reset();
        sync
    }

    #[must_use]
    pub fn with_levels(mut self, levels: &[f32]) -> Self {
        self.decisions = Some(Decisions::new(levels));
        self
    }

    /// Estimates the timing of each burst from its first `symbols` symbols before tracking it.
    /// Symbols of a burst come out once that many have arrived.
    #[must_use]
    pub fn with_acquisition(mut self, symbols: usize) -> Self {
        self.acquisition = Some(Acquisition::new(symbols, self.nominal_sps));
        self
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        self.run(input, false, |y, at_symbol| {
            if at_symbol {
                out.push(y);
            }
            None
        });
    }

    pub fn process_held(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        self.run(input, true, |y, at_symbol| {
            if at_symbol {
                out.push(y);
            }
            None
        });
    }

    pub fn process_steered(
        &mut self,
        input: &[Complex<f32>],
        mut steer: impl FnMut(Complex<f32>) -> Option<f64>,
    ) {
        self.run(
            input,
            false,
            |y, at_symbol| {
                if at_symbol { steer(y) } else { None }
            },
        );
    }

    pub fn process_spaced(
        &mut self,
        input: &[Complex<f32>],
        steer: impl FnMut(Complex<f32>, bool) -> Option<f64>,
    ) {
        self.run(input, false, steer);
    }

    #[must_use]
    pub fn sps(&self) -> f64 {
        self.sps
    }

    pub fn reset(&mut self) {
        self.sps = self.nominal_sps;
        self.free_run_sps = self.nominal_sps;
        self.step = self.nominal_sps;
        self.pos = 1;
        self.frac = 0.0;
        self.consumed = 0;
        self.buf.clear();
        self.at_symbol = true;
        self.prev_symbol = ZERO;
        self.mid = ZERO;
        self.primed = false;
        if let Some(decisions) = &mut self.decisions {
            decisions.reset();
        }
        if let Some(acquisition) = &mut self.acquisition {
            acquisition.armed = true;
        }
    }

    fn run(
        &mut self,
        input: &[Complex<f32>],
        hold: bool,
        mut on_sample: impl FnMut(Complex<f32>, bool) -> Option<f64>,
    ) {
        if hold {
            self.step_through(false, &mut on_sample);
        }
        self.buf.extend_from_slice(input);
        if let Some(acquisition) = &mut self.acquisition {
            acquisition.armed |= hold;
        }
        if let Some(span) = self
            .acquisition
            .filter(|a| a.armed && !hold)
            .map(|a| a.span)
        {
            if self.ahead() < span {
                return;
            }
            self.acquire(span);
        }
        self.step_through(hold, &mut on_sample);
        let reach = self.acquisition.map_or(0, |a| a.reach);
        let drain = (self.pos - 1)
            .saturating_sub(reach)
            .saturating_sub(self.consumed)
            .min(self.buf.len());
        self.buf.drain(..drain);
        self.consumed += drain;
    }

    fn ahead(&self) -> usize {
        (self.consumed + self.buf.len()).saturating_sub(self.pos)
    }

    /// Moves the next symbol onto the nearest peak of the symbol-rate line over the `span`
    /// samples ahead. The rate starts over from the long-run one: whatever a noisy last burst
    /// left in the loop says nothing about this one's clock.
    fn acquire(&mut self, span: usize) {
        if let Some(acquisition) = &mut self.acquisition {
            acquisition.armed = false;
        }
        let start = self.pos - self.consumed;
        let window = &self.buf[start..(start + span).min(self.buf.len())];
        let Some(peak) = symbol_peak(window, self.sps) else {
            return;
        };
        let next = if self.at_symbol {
            self.frac
        } else {
            self.frac + 0.5 * self.step
        };
        let mut at = peak + ((next - peak) / self.sps).round() * self.sps;
        let earliest = 1.0 - start as f64;
        if at < earliest {
            at += self.sps;
        }
        let whole = at.floor();
        self.pos = self.pos.saturating_add_signed(whole as isize);
        self.frac = at - whole;
        self.at_symbol = true;
        self.primed = false;
        self.sps = self.free_run_sps;
        self.step = self.sps;
    }

    fn step_through(
        &mut self,
        hold: bool,
        on_sample: &mut impl FnMut(Complex<f32>, bool) -> Option<f64>,
    ) {
        while self.pos + 3 <= self.consumed + self.buf.len() {
            let base = self.pos - self.consumed;
            let y = farrow(&self.buf[base - 1..base + 3], self.frac as f32);
            if self.at_symbol {
                let steered = on_sample(y, true);
                if hold {
                    self.step = self.free_run_sps;
                    if let Some(decisions) = &mut self.decisions {
                        decisions.hold();
                    }
                } else if let Some(err) = steered.filter(|_| self.primed) {
                    self.steer(err);
                } else if self.primed {
                    self.retime(y);
                }
                self.prev_symbol = y;
                self.primed = true;
            } else {
                self.mid = y;
                let _ = on_sample(y, false);
            }
            self.at_symbol = !self.at_symbol;
            self.advance();
        }
    }

    fn advance(&mut self) {
        self.frac += 0.5 * self.step;
        let whole = self.frac.floor();
        self.frac -= whole;
        self.pos += whole as usize;
    }

    fn retime(&mut self, symbol: Complex<f32>) {
        if let Some(decisions) = &mut self.decisions {
            let decided = decisions.error(symbol.re, self.prev_symbol.re);
            if decisions.trusted() {
                decisions.mark_acquired();
                match decided {
                    Some(err) => self.steer(err),
                    None => self.step = self.free_run_sps,
                }
                return;
            }
        }
        let (before, after) = (self.prev_symbol.norm_sqr(), symbol.norm_sqr());
        if before <= 0.0 || after <= 0.0 {
            self.step = self.free_run_sps;
            return;
        }
        let Some(centre) = self.transition(symbol) else {
            self.step = self.sps;
            return;
        };
        let raw = ((symbol - self.prev_symbol) * (self.mid - centre).conj()).re;
        let err = f64::from(raw) / (f64::from(0.5 * (before + after)) * TAU);
        if !err.is_finite() {
            self.step = self.free_run_sps;
            return;
        }
        self.steer(err);
    }

    fn transition(&self, symbol: Complex<f32>) -> Option<Complex<f32>> {
        let sum = f64::from((symbol + self.prev_symbol).norm_sqr());
        let diff = f64::from((symbol - self.prev_symbol).norm_sqr());
        if !self.decisions.as_ref().is_some_and(Decisions::acquiring) {
            return (sum <= SYMMETRY_TOLERANCE * diff).then_some(ZERO);
        }
        let crossing = (symbol * self.prev_symbol.conj()).re < 0.0;
        crossing.then(|| (symbol + self.prev_symbol) * 0.5)
    }

    fn steer(&mut self, err: f64) {
        let err = err.clamp(-0.5, 0.5);
        self.sps = (self.sps - self.beta * err * self.nominal_sps).clamp(
            self.nominal_sps * (1.0 - TRACKING_RANGE),
            self.nominal_sps * (1.0 + TRACKING_RANGE),
        );
        self.free_run_sps += (self.sps - self.free_run_sps) / FREE_RUN_MEMORY_SYMBOLS;
        self.step = self.sps - self.alpha * err * self.nominal_sps;
    }
}

#[cfg(test)]
mod tests;
