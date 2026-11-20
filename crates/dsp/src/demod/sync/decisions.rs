use std::f64::consts::PI;

const AMPLITUDE_SYMBOLS: f32 = 64.0;
const INV_AMPLITUDE_SYMBOLS: f32 = 1.0 / AMPLITUDE_SYMBOLS;
const AMPLITUDE_DECAY: f32 = 1.0 - INV_AMPLITUDE_SYMBOLS;
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
    inv_outer: f32,
    outer_edge: f32,
    inv_mean_square_pi: f64,
    amplitude: f32,
    previous: f32,
    seen: f32,
    since_outer: f32,
    inv_spacing: f32,
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
        let spacing = if spacing.is_finite() { spacing } else { outer };
        Self {
            levels: levels.to_vec(),
            inv_outer: outer.recip(),
            outer_edge: f32::midpoint(outer, inner) / outer,
            inv_mean_square_pi: (f64::from(mean_square) * PI).recip(),
            amplitude: 0.0,
            previous: 0.0,
            seen: 0.0,
            since_outer: 0.0,
            inv_spacing: spacing.recip(),
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
        let scale = self.amplitude * self.inv_outer;
        if scale <= 0.0 || !scale.is_finite() {
            return None;
        }
        let inv_scale = scale.recip();
        let normalised = symbol * inv_scale;
        let decision = self.nearest(normalised);
        self.judge(normalised, decision);
        let raw = decision * previous - self.previous * symbol;
        self.previous = decision;
        let err = f64::from(raw) * (f64::from(inv_scale) * self.inv_mean_square_pi);
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
        let miss = (normalised - decision) * self.inv_spacing;
        self.misfit += (miss * miss - self.misfit) * INV_AMPLITUDE_SYMBOLS;
        self.trusted = if self.trusted {
            self.misfit < DISTRUST_MISFIT
        } else {
            self.misfit < TRUST_MISFIT
        };
    }

    #[inline]
    fn nearest(&self, x: f32) -> f32 {
        let mut best = self.levels[0];
        let mut best_dist = (best - x).abs();
        for &level in &self.levels[1..] {
            let dist = (level - x).abs();
            if dist < best_dist {
                best_dist = dist;
                best = level;
            }
        }
        best
    }

    fn track_amplitude(&mut self, magnitude: f32) {
        self.seen += 1.0;
        self.since_hold += 1.0;
        if self.seen <= 1.0 {
            self.amplitude = magnitude;
        } else if magnitude >= self.outer_edge * self.amplitude {
            let gain = if self.seen >= AMPLITUDE_SYMBOLS {
                INV_AMPLITUDE_SYMBOLS
            } else {
                self.seen.recip()
            };
            self.amplitude += (magnitude - self.amplitude) * gain;
            self.since_outer = 0.0;
        } else {
            self.since_outer += 1.0;
            if self.since_outer > HOLD_SYMBOLS {
                self.amplitude *= AMPLITUDE_DECAY;
            }
        }
    }
}
