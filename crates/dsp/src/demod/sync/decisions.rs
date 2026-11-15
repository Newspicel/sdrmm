use std::f64::consts::PI;

const AMPLITUDE_SYMBOLS: f32 = 64.0;
const SETTLE_SYMBOLS: f32 = 2.0 * AMPLITUDE_SYMBOLS;
const HOLD_SYMBOLS: f32 = 4.0 * AMPLITUDE_SYMBOLS;
const TRUST_MISFIT: f32 = 0.05;
const DISTRUST_MISFIT: f32 = 0.065;
const ACQUIRE_SYMBOLS: f32 = 256.0;

/// Decision-directed timing error for multi-level symbols: tracks the outer amplitude, slices
/// each symbol to its nearest level and trusts the result once the slices fit.
#[derive(Clone, Debug)]
pub(super) struct Decisions {
    levels: Vec<f32>,
    outer: f32,
    outer_edge: f32,
    mean_square: f32,
    amplitude: f32,
    previous: f32,
    seen: f32,
    since_outer: f32,
    spacing: f32,
    misfit: f32,
    trusted: bool,
    acquired: bool,
    since_hold: f32,
}

impl Decisions {
    pub(super) fn new(levels: &[f32]) -> Self {
        assert!(!levels.is_empty(), "decision levels must not be empty");
        let outer = levels.iter().fold(0.0f32, |acc, l| acc.max(l.abs()));
        assert!(outer > 0.0, "decision levels must not all be zero");
        let inner = levels
            .iter()
            .map(|l| l.abs())
            .filter(|&l| l < outer)
            .fold(0.0f32, f32::max);
        let mean_square = levels.iter().map(|l| l * l).sum::<f32>() / levels.len() as f32;
        let spacing = levels
            .iter()
            .flat_map(|a| levels.iter().map(move |b| (a - b).abs()))
            .filter(|&d| d > 0.0)
            .fold(f32::INFINITY, f32::min);
        Self {
            levels: levels.to_vec(),
            outer,
            outer_edge: 0.5 * (outer + inner) / outer,
            mean_square,
            amplitude: 0.0,
            previous: 0.0,
            seen: 0.0,
            since_outer: 0.0,
            spacing: if spacing.is_finite() { spacing } else { outer },
            misfit: 1.0,
            trusted: false,
            acquired: false,
            since_hold: 0.0,
        }
    }

    pub(super) fn trusted(&self) -> bool {
        self.trusted && self.seen >= SETTLE_SYMBOLS
    }

    pub(super) fn acquiring(&self) -> bool {
        !self.acquired && self.since_hold < ACQUIRE_SYMBOLS
    }

    pub(super) fn mark_acquired(&mut self) {
        self.acquired = true;
    }

    /// The clock is held: whatever follows must be acquired afresh.
    pub(super) fn hold(&mut self) {
        self.acquired = false;
        self.since_hold = 0.0;
    }

    pub(super) fn error(&mut self, symbol: f32, previous: f32) -> Option<f64> {
        self.track_amplitude(symbol.abs());
        let scale = self.amplitude / self.outer;
        if scale <= 0.0 || !scale.is_finite() {
            return None;
        }
        let decision = self.nearest(symbol / scale);
        self.judge(symbol / scale, decision);
        let raw = decision * previous - self.previous * symbol;
        self.previous = decision;
        let err = f64::from(raw) / (f64::from(scale * self.mean_square) * PI);
        err.is_finite().then_some(err)
    }

    pub(super) fn reset(&mut self) {
        self.amplitude = 0.0;
        self.previous = 0.0;
        self.seen = 0.0;
        self.since_outer = 0.0;
        self.misfit = 1.0;
        self.trusted = false;
        self.acquired = false;
        self.since_hold = 0.0;
    }

    fn judge(&mut self, normalised: f32, decision: f32) {
        let miss = (normalised - decision) / self.spacing;
        self.misfit += (miss * miss - self.misfit) / AMPLITUDE_SYMBOLS;
        self.trusted = if self.trusted {
            self.misfit < DISTRUST_MISFIT
        } else {
            self.misfit < TRUST_MISFIT
        };
    }

    fn nearest(&self, x: f32) -> f32 {
        self.levels
            .iter()
            .copied()
            .min_by(|a, b| (a - x).abs().total_cmp(&(b - x).abs()))
            .unwrap_or(0.0)
    }

    fn track_amplitude(&mut self, magnitude: f32) {
        self.seen += 1.0;
        self.since_hold += 1.0;
        if self.seen <= 1.0 {
            self.amplitude = magnitude;
        } else if magnitude >= self.outer_edge * self.amplitude {
            self.amplitude += (magnitude - self.amplitude) / self.seen.min(AMPLITUDE_SYMBOLS);
            self.since_outer = 0.0;
        } else {
            self.since_outer += 1.0;
            if self.since_outer > HOLD_SYMBOLS {
                self.amplitude *= 1.0 - 1.0 / AMPLITUDE_SYMBOLS;
            }
        }
    }
}
