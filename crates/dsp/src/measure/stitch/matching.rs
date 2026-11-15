use num_complex::Complex;

use super::Stitcher;

const MATCH_SMOOTHING: f32 = 0.2;
pub(super) const MATCH_COHERENCE: f32 = 0.3;

/// One output bin both lanes of a pair reach, with each side's recent spur history.
#[derive(Clone, Copy)]
pub(super) struct Overlap {
    pub(super) low_bin: usize,
    pub(super) high_bin: usize,
    pub(super) low_seen: u16,
    pub(super) high_seen: u16,
}

/// The smoothed cross-spectrum of two neighbouring lanes over their shared bins, and the
/// complex gain that carries the higher lane onto the lower.
#[derive(Clone, Copy, Default)]
pub(super) struct Match {
    pub(super) low: usize,
    pub(super) high: usize,
    pub(super) start: usize,
    pub(super) end: usize,
    cross: Complex<f32>,
    low_power: f32,
    high_power: f32,
    gain: Complex<f32>,
    pub(super) coherence: Option<f32>,
}

impl Match {
    pub(super) fn fresh(low: usize, high: usize) -> Self {
        Self {
            low,
            high,
            gain: Complex::new(1.0, 0.0),
            ..Self::default()
        }
    }

    /// This match over the overlap entries `start..end`.
    pub(super) fn spanning(self, start: usize, end: usize) -> Self {
        Self { start, end, ..self }
    }

    fn update(&mut self, cross: Complex<f32>, low_power: f32, high_power: f32) {
        let keep = 1.0 - MATCH_SMOOTHING;
        self.cross = self.cross * keep + cross * MATCH_SMOOTHING;
        self.low_power = self.low_power * keep + low_power * MATCH_SMOOTHING;
        self.high_power = self.high_power * keep + high_power * MATCH_SMOOTHING;
        let product = self.low_power * self.high_power;
        if product <= f32::MIN_POSITIVE {
            return;
        }
        let coherence = self.cross.norm_sqr() / product;
        self.coherence = Some(coherence);
        if coherence > MATCH_COHERENCE {
            self.gain = self.cross / self.high_power;
        }
    }

    fn step(&self, phase_only: bool) -> Complex<f32> {
        if !phase_only {
            return self.gain;
        }
        let norm = self.gain.norm();
        if norm > f32::MIN_POSITIVE {
            self.gain / norm
        } else {
            Complex::new(1.0, 0.0)
        }
    }
}

impl Stitcher {
    pub(super) fn match_lanes(&mut self) {
        let start = self.frame_start;
        let (lanes, overlaps) = (&self.lanes, &self.overlaps);
        for pair in &mut self.matches {
            let (low, high) = (&lanes[pair.low], &lanes[pair.high]);
            let (low_turn, high_turn) = (low.rotation(start), high.rotation(start));
            let mut cross = Complex::default();
            let (mut low_power, mut high_power) = (0.0f32, 0.0f32);
            for entry in &overlaps[pair.start..pair.end] {
                let a = low.spectrum[entry.low_bin] * low_turn;
                let b = high.spectrum[entry.high_bin] * high_turn;
                cross += a * b.conj();
                low_power += a.norm_sqr();
                high_power += b.norm_sqr();
            }
            pair.update(cross, low_power, high_power);
        }
        self.chain_corrections();
    }

    fn chain_corrections(&mut self) {
        for lane in &mut self.lanes {
            lane.correction = Complex::new(1.0, 0.0);
        }
        if !self.options.match_phase {
            return;
        }
        let phase_only = self.options.equalise;
        for pair in &self.matches {
            let base = self.lanes[pair.low].correction;
            self.lanes[pair.high].correction = base * pair.step(phase_only);
        }
    }
}
