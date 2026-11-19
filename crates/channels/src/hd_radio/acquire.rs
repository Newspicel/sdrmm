use std::f32::consts::{FRAC_PI_2, TAU};

use num_complex::Complex;
use sdrmm_dsp::{design_lowpass, math::fft::Transform};

use super::layout::{BLOCK, CP, FFT, INPUT_RATE_HZ, SYMBOL};

/// One block of symbols and one more, so a block found anywhere in the window still fits.
const WINDOW: usize = SYMBOL * (BLOCK + 1);
const SEARCH_TAPS: usize = 63;
/// The search filter passes the digital sidebands and stops the analog host between them.
const SIDEBAND_CENTRE_HZ: f64 = 160_000.0;
const SIDEBAND_HALF_WIDTH_HZ: f64 = 42_000.0;
const SEARCH_DELAY: usize = (SEARCH_TAPS - 1) / 2;
const ANGLE_SMOOTHING: f32 = 0.25;

/// What the block tracker measured on the last block, for the next one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tracking {
    pub samples: i32,
    pub angle: f32,
}

/// Collects one block of input, finds where its symbols start and how far off frequency it
/// is, and transforms each symbol: OFDM symbol timing from the cyclic prefix, carried from
/// block to block once the reference subcarriers track it.
pub struct Acquire {
    window: Vec<Complex<f32>>,
    filled: usize,
    filtered: Vec<Complex<f32>>,
    search: Vec<f32>,
    sums: Vec<Complex<f32>>,
    shape: Vec<f32>,
    transform: Transform,
    folded: Vec<Complex<f32>>,
    phase: Complex<f32>,
    angle: f32,
    cfo: i32,
    keep_extra: usize,
}

impl Acquire {
    pub fn new() -> Self {
        Self {
            window: vec![Complex::default(); WINDOW],
            filled: 0,
            filtered: vec![Complex::default(); WINDOW],
            search: search_taps(),
            sums: vec![Complex::default(); SYMBOL],
            shape: shape(),
            transform: Transform::forward(FFT),
            folded: vec![Complex::default(); FFT],
            phase: Complex::new(1.0, 0.0),
            angle: 0.0,
            cfo: 0,
            keep_extra: 0,
        }
    }

    pub fn reset(&mut self) {
        self.filled = 0;
        self.phase = Complex::new(1.0, 0.0);
        self.angle = 0.0;
        self.cfo = 0;
        self.keep_extra = 0;
    }

    /// Takes samples until the window is full; returns how many it took.
    pub fn push(&mut self, iq: &[Complex<f32>]) -> usize {
        let take = (WINDOW - self.filled).min(iq.len());
        for (slot, sample) in self.window[self.filled..self.filled + take]
            .iter_mut()
            .zip(iq)
        {
            *slot = sample.conj();
        }
        self.filled += take;
        take
    }

    pub const fn full(&self) -> bool {
        self.filled == WINDOW
    }

    /// Moves `bins` whole subcarriers up, as the reference subcarriers found them there.
    pub fn shift_carriers(&mut self, bins: i32) {
        self.cfo += bins;
    }

    /// Starts the next block `symbols` symbols later than timing alone would.
    pub fn delay(&mut self, symbols: usize) {
        self.keep_extra = symbols * SYMBOL;
    }

    /// The frequency the window is corrected by, in Hz of the input.
    pub fn offset_hz(&self) -> f64 {
        let angle = f64::from(self.angle) - f64::from(TAU) * f64::from(self.cfo);
        angle * INPUT_RATE_HZ / (f64::from(TAU) * FFT as f64)
    }

    /// Transforms the window's block into `bins` (`BLOCK` symbols per subcarrier, centred) and
    /// returns the timing step the subcarrier phases must follow. `tracking` holds the block
    /// tracker's correction while it is locked; without it the cyclic prefix sets the timing.
    pub fn process(&mut self, tracking: Option<Tracking>, bins: &mut [Complex<f32>]) -> i32 {
        let start = match tracking {
            Some(tracking) => {
                self.angle -= tracking.angle;
                (SYMBOL / 2) as i32 + tracking.samples
            }
            None => self.search_prefix(),
        }
        .clamp(0, SYMBOL as i32 - 1);
        let step = (SYMBOL / 2) as i32 - start;
        let angle = self.angle - TAU * self.cfo as f32;
        self.phase *= Complex::from_polar(1.0, -(step as f32) * angle / FFT as f32);
        let increment = Complex::from_polar(1.0, angle / FFT as f32);
        for symbol in 0..BLOCK {
            self.fold(symbol * SYMBOL + start as usize, increment);
            self.transform.process(&mut self.folded);
            for (bin, value) in self.folded.iter().enumerate() {
                bins[((bin + FFT / 2) % FFT) * BLOCK + symbol] = *value;
            }
        }
        let keep = SYMBOL as i32 + step + self.keep_extra as i32;
        let keep = keep.clamp(0, WINDOW as i32) as usize;
        self.keep_extra = 0;
        self.window.copy_within(WINDOW - keep.., 0);
        self.filled = keep;
        step
    }

    fn fold(&mut self, from: usize, increment: Complex<f32>) {
        for (offset, &raw) in self.window[from..from + SYMBOL].iter().enumerate() {
            let sample = self.phase * raw;
            if offset < CP {
                self.folded[offset] = sample * self.shape[offset];
            } else if offset < FFT {
                self.folded[offset] = sample;
            } else {
                self.folded[offset - FFT] += sample * self.shape[offset];
            }
            self.phase *= increment;
        }
        self.phase /= self.phase.norm();
    }

    /// The block start the cyclic prefixes agree on, and the frequency their phase shows.
    fn search_prefix(&mut self) -> i32 {
        self.filter();
        self.sums.fill(Complex::default());
        for (offset, sum) in self.sums.iter_mut().enumerate() {
            for symbol in 0..BLOCK {
                let at = offset + symbol * SYMBOL;
                *sum += self.filtered[at] * self.filtered[at + FFT].conj();
            }
        }
        let mut best = (-1.0f32, Complex::default(), 0usize);
        for offset in 0..SYMBOL {
            let value: Complex<f32> = (0..CP)
                .map(|j| self.sums[(offset + j) % SYMBOL] * self.shape[j] * self.shape[j + FFT])
                .sum();
            if value.norm_sqr() > best.0 {
                best = (value.norm_sqr(), value, offset);
            }
        }
        let (_, value, offset) = best;
        let turn = (value * Complex::from_polar(1.0, -self.angle)).arg();
        let gain = if self.angle == 0.0 {
            1.0
        } else {
            ANGLE_SMOOTHING
        };
        self.angle += turn * gain;
        ((offset + SYMBOL - SEARCH_DELAY) % SYMBOL) as i32
    }

    fn filter(&mut self) {
        for (at, out) in self.filtered.iter_mut().enumerate() {
            let reach = (at + 1).min(SEARCH_TAPS);
            *out = (0..reach)
                .map(|tap| self.window[at - tap] * self.search[tap])
                .sum();
        }
    }
}

/// The raised-sine edges each symbol is shaped with, so folding the cyclic extension back
/// onto the symbol sums to one.
fn shape() -> Vec<f32> {
    (0..SYMBOL)
        .map(|i| {
            if i < CP {
                (FRAC_PI_2 * i as f32 / CP as f32).sin()
            } else if i < FFT {
                1.0
            } else {
                (FRAC_PI_2 * (i - FFT) as f32 / CP as f32).cos()
            }
        })
        .collect()
}

fn search_taps() -> Vec<f32> {
    let centre = SIDEBAND_CENTRE_HZ / INPUT_RATE_HZ;
    design_lowpass(SEARCH_TAPS, SIDEBAND_HALF_WIDTH_HZ / INPUT_RATE_HZ)
        .iter()
        .enumerate()
        .map(|(k, &tap)| {
            let t = k as f64 - SEARCH_DELAY as f64;
            (2.0 * (std::f64::consts::TAU * centre * t).cos() * f64::from(tap)) as f32
        })
        .collect()
}
