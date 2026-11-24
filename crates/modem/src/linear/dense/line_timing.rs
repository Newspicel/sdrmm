use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::farrow;

/// Symbols the opening square-law estimate spans before the loop takes over.
pub(super) const WARM_SYMBOLS: usize = 4096;

/// Noise bandwidths of the timing loop in cycles per symbol, each held until the symbol count
/// beside it. Wide first to settle what the opening block measured of the clock, then narrow
/// enough against the data self-noise on the square-law line to hold a million-point grid's
/// symbol centres.
const LOOP_GEARS: [(f64, u64); 3] = [(1e-4, 4_000), (3e-5, 10_000), (1e-5, u64::MAX)];

/// Symbols the line strength average spans.
const STRENGTH_SYMBOLS: f64 = 4096.0;

/// Largest correction one symbol may make to the next instant, in samples. The line error of a
/// single symbol is mostly data noise, and the loop only works on its average.
const MAX_NUDGE: f64 = 0.5;

/// Share of the nominal symbol period the loop may move the period by.
const PERIOD_RANGE: f64 = 0.01;

/// Symbol timing from the square-law symbol-rate line: a block estimate to start, then a narrow
/// second-order loop that keeps the line's phase at zero. Unlike a decision-directed detector it
/// needs no decisions, so it holds the symbol centres to a hundredth of a sample before any
/// decision of a dense constellation can be trusted.
pub(super) struct LineTiming {
    nominal: f64,
    period: f64,
    half: usize,
    gains: [(f64, f64); 3],
    symbols: u64,
    buf: Vec<Complex<f32>>,
    /// The next instant: sample `index` of `buf` plus `frac`. Kept apart so the arithmetic is
    /// the same wherever the input was split.
    index: usize,
    frac: f64,
    strength: f64,
    level: f64,
    started: bool,
}

impl LineTiming {
    pub(super) fn new(sps: f64) -> Self {
        Self {
            nominal: sps,
            period: sps,
            half: (sps / 2.0).ceil() as usize,
            gains: LOOP_GEARS.map(|(bw, _)| loop_gains(bw)),
            symbols: 0,
            buf: Vec::with_capacity(warm_samples(sps) * 2),
            index: 0,
            frac: 0.0,
            strength: 0.0,
            level: 0.0,
            started: false,
        }
    }

    /// Hands each timed symbol of `filtered` to `emit`, holding back the samples the next one
    /// still needs.
    pub(super) fn process(
        &mut self,
        filtered: &[Complex<f32>],
        mut emit: impl FnMut(Complex<f32>),
    ) {
        self.buf.extend_from_slice(filtered);
        if !self.started {
            if self.buf.len() < warm_samples(self.nominal) {
                return;
            }
            self.start();
        }
        while self.index + self.half + 3 <= self.buf.len() {
            let i = self.index;
            emit(farrow(&self.buf[i - 1..i + 3], self.frac as f32));
            let err = self.line_error(i);
            let (kp, ki) = self.gear();
            self.period = (self.period + ki * err).clamp(
                self.nominal * (1.0 - PERIOD_RANGE),
                self.nominal * (1.0 + PERIOD_RANGE),
            );
            let next = self.frac + self.period + (kp * err).clamp(-MAX_NUDGE, MAX_NUDGE);
            let whole = next.floor();
            self.index += whole as usize;
            self.frac = next - whole;
        }
        let behind = self.index.saturating_sub(self.half + 2);
        self.buf.drain(..behind);
        self.index -= behind;
    }

    pub(super) fn reset(&mut self) {
        self.buf.clear();
        self.symbols = 0;
        self.period = self.nominal;
        self.index = 0;
        self.frac = 0.0;
        self.strength = 0.0;
        self.level = 0.0;
        self.started = false;
    }

    /// Places the first instant and measures the clock from the opening block: the line's phase
    /// over each half gives the offset at each half's centre, and their difference the period.
    fn start(&mut self) {
        let warm = warm_samples(self.nominal);
        let half = warm / 2;
        let early = self.square_law_line(0, half);
        let late = self.square_law_line(half, warm);
        let line = early + late;
        let to_samples = |phase: f64| -phase / TAU * self.nominal;
        let drift = to_samples((late * early.conj()).arg());
        let half_symbols = half as f64 / self.nominal;
        self.period = (self.nominal + drift / half_symbols).clamp(
            self.nominal * (1.0 - PERIOD_RANGE),
            self.nominal * (1.0 + PERIOD_RANGE),
        );
        self.strength = line.norm() / (warm as f64 / self.nominal);
        self.level = self.buf[..warm]
            .iter()
            .map(|s| f64::from(s.norm_sqr()))
            .sum::<f64>()
            / warm as f64;
        let centre_symbols = warm as f64 / 2.0 / self.nominal;
        let mut at = (to_samples(line.arg()) - (self.period - self.nominal) * centre_symbols)
            .rem_euclid(self.nominal);
        while at < (self.half + 1) as f64 {
            at += self.nominal;
        }
        self.index = at.floor() as usize;
        self.frac = at - at.floor();
        self.started = true;
    }

    /// How far, in samples, the square-law line around sample `i` peaks after the current
    /// instant. The mean power is taken out first: at a fractional rate the window is not a whole
    /// period, and the mean would leak into the line as a timing error that follows the fraction.
    fn line_error(&mut self, i: usize) -> f64 {
        let first = i + 1 - self.half;
        let offset = (first as f64 - i as f64) - self.frac;
        let mut turn = Complex::from_polar(1.0, -TAU * offset / self.period);
        let step = Complex::from_polar(1.0, -TAU / self.period);
        let mut line = Complex::new(0.0f64, 0.0);
        let mut power = 0.0f64;
        for s in &self.buf[first..=i + self.half] {
            let p = f64::from(s.norm_sqr());
            line += turn * (p - self.level);
            power += p;
            turn *= step;
        }
        let window = (2 * self.half) as f64;
        self.level += (power / window - self.level) / STRENGTH_SYMBOLS;
        self.strength += (line.re - self.strength) / STRENGTH_SYMBOLS;
        -line.im / self.strength.max(f64::MIN_POSITIVE) / TAU * self.period
    }
}

impl LineTiming {
    fn gear(&mut self) -> (f64, f64) {
        self.symbols += 1;
        let gear = LOOP_GEARS
            .iter()
            .position(|&(_, until)| self.symbols <= until)
            .unwrap_or(LOOP_GEARS.len() - 1);
        self.gains[gear]
    }

    fn square_law_line(&self, from: usize, to: usize) -> Complex<f64> {
        let mut line = Complex::new(0.0f64, 0.0);
        let mut turn = Complex::from_polar(1.0, -TAU * from as f64 / self.nominal);
        let step = Complex::from_polar(1.0, -TAU / self.nominal);
        for s in &self.buf[from..to] {
            line += turn * f64::from(s.norm_sqr());
            turn *= step;
        }
        line
    }
}

fn warm_samples(sps: f64) -> usize {
    (WARM_SYMBOLS as f64 * sps).ceil() as usize
}

/// Proportional and integral gains of a critically damped loop with noise bandwidth `bw`.
pub(super) fn loop_gains(bw: f64) -> (f64, f64) {
    let zeta = std::f64::consts::FRAC_1_SQRT_2;
    let wn = bw * TAU / (zeta + 1.0 / (4.0 * zeta));
    (2.0 * zeta * wn, wn * wn)
}
