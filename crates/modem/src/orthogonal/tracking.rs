use std::{f32::consts::TAU, fmt};

use num_complex::Complex;
use sdrmm_dsp::{hann, math::fft::Transform};

use crate::soft::argmax;

/// Timing estimates per symbol, as in Codec2's FSK modem.
pub const DEFAULT_OVERSAMPLE: usize = 8;
/// Symbols per processing block: long enough to average the tone and timing estimates, short
/// enough to follow a drifting transmitter.
pub const DEFAULT_BLOCK_SYMBOLS: usize = 50;
/// The tone search bins are this share of the symbol rate wide.
const BIN_SHARE_OF_BAUD: f64 = 0.1;
/// Weight of each new spectrum in the running tone search average.
const SPECTRUM_SMOOTHING: f32 = 0.1;
/// Around a found tone, the spectrum is blanked this share of the symbol rate either side so the
/// next search finds a different tone.
const BLANK_SHARE_OF_BAUD: f64 = 0.75;
/// A timing estimate past this share of a symbol shifts the next block by half a symbol.
const RETIME_THRESHOLD: f32 = 0.25;
const SNR_SMOOTHING: f32 = 0.5;
const EB_NO_OFFSET_DB: f32 = -6.0;
const TINY: f32 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackingMfskConfig {
    pub rate_hz: u32,
    pub baud: u32,
    pub tones: usize,
    /// The band the tones are searched in, low and high edge in Hz, negative below the centre.
    pub search_hz: (f32, f32),
    pub oversample: usize,
    pub block_symbols: usize,
}

impl TrackingMfskConfig {
    #[must_use]
    pub fn new(rate_hz: u32, baud: u32, tones: usize, search_hz: (f32, f32)) -> Self {
        Self {
            rate_hz,
            baud,
            tones,
            search_hz,
            oversample: DEFAULT_OVERSAMPLE,
            block_symbols: DEFAULT_BLOCK_SYMBOLS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackingMfskError {
    Rate { rate: u32, baud: u32 },
    Oversample { oversample: usize },
    Tones { tones: usize },
    Search { low: f32, high: f32 },
    Block { symbols: usize },
}

impl fmt::Display for TrackingMfskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Rate { rate, baud } => write!(
                f,
                "{rate} Hz is not a whole number of samples per symbol at {baud} baud"
            ),
            Self::Oversample { oversample } => {
                write!(f, "{oversample} timing phases do not divide a symbol")
            }
            Self::Tones { tones } => write!(f, "{tones} tones: need a power of two from 2 to 16"),
            Self::Search { low, high } => {
                write!(f, "search band {low}..{high} Hz is empty or past Nyquist")
            }
            Self::Block { symbols } => {
                write!(f, "a block needs at least two symbols, got {symbols}")
            }
        }
    }
}

impl std::error::Error for TrackingMfskError {}

/// A non-coherent M-FSK receiver for tones at unknown, drifting frequencies, after Codec2's
/// `fsk.c`: each block finds the M strongest peaks of a smoothed spectrum, mixes each tone to DC,
/// integrates a symbol at `oversample` phases, takes the timing from the symbol-rate line of the
/// summed energies and decides each symbol as the strongest tone. Tones count up from the lowest,
/// so symbol 0 is the lowest tone. The block length moves by half a symbol to follow the clock.
pub struct TrackingMfsk {
    m: usize,
    rate: f32,
    sps: usize,
    oversample: usize,
    symbols: usize,
    block: usize,
    memory: usize,
    nin: usize,
    staged: Vec<Complex<f32>>,
    search: Search,
    tones_hz: Vec<f32>,
    rotors: Vec<Complex<f32>>,
    mixed: Vec<Complex<f32>>,
    integrals: Vec<Complex<f32>>,
    energies: Vec<f32>,
    snr_db: f32,
}

struct Search {
    fft: Transform,
    window: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    smoothed: Vec<f32>,
    scratch: Vec<f32>,
    bins: (usize, usize),
    blank: usize,
}

impl TrackingMfsk {
    pub fn new(config: TrackingMfskConfig) -> Result<Self, TrackingMfskError> {
        let sps = samples_per_symbol(&config)?;
        let symbols = config.block_symbols;
        let block = sps * symbols;
        let memory = block + 2 * sps;
        let phases = (symbols + 1) * config.oversample;
        Ok(Self {
            m: config.tones,
            rate: config.rate_hz as f32,
            sps,
            oversample: config.oversample,
            symbols,
            block,
            memory,
            nin: block,
            staged: Vec::with_capacity(block + sps),
            search: Search::new(&config)?,
            tones_hz: vec![0.0; config.tones],
            rotors: vec![Complex::new(1.0, 0.0); config.tones],
            mixed: vec![Complex::default(); config.tones * memory],
            integrals: vec![Complex::default(); config.tones * phases],
            energies: vec![0.0; config.tones],
            snr_db: 0.0,
        })
    }

    #[must_use]
    pub fn tones(&self) -> usize {
        self.m
    }

    /// The tone frequencies the last block used, lowest first.
    #[must_use]
    pub fn tones_hz(&self) -> &[f32] {
        &self.tones_hz
    }

    /// Eb/N0 of the recent blocks, from the spread of the winning tone's amplitude.
    #[must_use]
    pub fn snr_db(&self) -> f32 {
        self.snr_db
    }

    /// Takes `iq` and hands each decided symbol to `emit`, in order.
    pub fn push(&mut self, iq: &[Complex<f32>], mut emit: impl FnMut(u8)) {
        self.push_levels(iq, |symbol, _| emit(symbol));
    }

    /// Like [`Self::push`], with each symbol's level: the tone its energies centre on, on the
    /// odd-integer rail of [`crate::cpm::Mapping::natural`], so a clean tone lands on its level.
    pub fn push_levels(&mut self, mut iq: &[Complex<f32>], mut emit: impl FnMut(u8, f32)) {
        while !iq.is_empty() {
            let take = (self.nin - self.staged.len()).min(iq.len());
            self.staged.extend_from_slice(&iq[..take]);
            iq = &iq[take..];
            if self.staged.len() == self.nin {
                let staged = std::mem::take(&mut self.staged);
                self.demodulate(&staged, &mut emit);
                self.staged = staged;
                self.staged.clear();
            }
        }
    }

    fn demodulate(&mut self, input: &[Complex<f32>], emit: &mut impl FnMut(u8, f32)) {
        self.search.estimate(input, self.rate, &mut self.tones_hz);
        self.mix(input);
        self.integrate();
        let Some(timing) = self.timing() else {
            return;
        };
        self.nin = match timing {
            t if t > RETIME_THRESHOLD => self.block + self.sps / 2,
            t if t < -RETIME_THRESHOLD => self.block - self.sps / 2,
            _ => self.block,
        };
        self.decide(timing * self.oversample as f32, emit);
    }

    fn mix(&mut self, input: &[Complex<f32>]) {
        let kept = self.memory - input.len();
        for (tone, &freq) in self.tones_hz.iter().enumerate() {
            let memory = &mut self.mixed[tone * self.memory..(tone + 1) * self.memory];
            memory.copy_within(input.len().., 0);
            let step = Complex::from_polar(1.0, TAU * freq / self.rate);
            let mut rotor = self.rotors[tone];
            for (slot, &sample) in memory[kept..].iter_mut().zip(input) {
                rotor *= step;
                *slot = sample * rotor.conj();
            }
            self.rotors[tone] = rotor / rotor.norm().max(TINY);
        }
    }

    fn integrate(&mut self) {
        let phases = self.phases();
        for tone in 0..self.m {
            let memory = &self.mixed[tone * self.memory..(tone + 1) * self.memory];
            let sums = &mut self.integrals[tone * phases..(tone + 1) * phases];
            for (phase, sum) in sums.iter_mut().enumerate() {
                let start = phase * self.sps / self.oversample;
                *sum = memory[start..start + self.sps].iter().sum();
            }
        }
    }

    /// Where the symbols start as a share of a symbol, from -0.5 to 0.5.
    fn timing(&self) -> Option<f32> {
        let phases = self.phases();
        let step = TAU / self.oversample as f32;
        let mut line = Complex::<f32>::default();
        for phase in 0..phases {
            let energy: f32 = (0..self.m)
                .map(|tone| self.integrals[tone * phases + phase].norm_sqr())
                .sum();
            line += Complex::from_polar(energy, step * phase as f32);
        }
        let timing = line.im.atan2(line.re) / TAU;
        timing.is_finite().then_some(timing)
    }

    fn decide(&mut self, at: f32, emit: &mut impl FnMut(u8, f32)) {
        let phases = self.phases();
        let low = at.floor();
        let fraction = at - low;
        let low = low as isize;
        let high = at.ceil() as isize;
        let mut amplitude_sum = 0.0f32;
        let mut power_sum = 0.0f32;
        for symbol in 0..self.symbols {
            let start = ((symbol + 1) * self.oversample) as isize;
            for (tone, energy) in self.energies.iter_mut().enumerate() {
                let sums = &self.integrals[tone * phases..(tone + 1) * phases];
                let early = sums[(start + low) as usize];
                let late = sums[(start + high) as usize];
                *energy = (early * (1.0 - fraction) + late * fraction).norm_sqr();
            }
            let decided = argmax(&self.energies);
            let peak = self.energies[usize::from(decided)];
            amplitude_sum += peak.sqrt();
            power_sum += peak;
            emit(decided, self.level());
        }
        self.track_snr(amplitude_sum, power_sum);
    }

    fn level(&self) -> f32 {
        let total: f32 = self.energies.iter().sum();
        if total <= TINY {
            return 0.0;
        }
        let top = (self.m - 1) as f32;
        let weighted: f32 = self
            .energies
            .iter()
            .enumerate()
            .map(|(tone, &energy)| (2.0 * tone as f32 - top) * energy)
            .sum();
        weighted / total
    }

    fn track_snr(&mut self, amplitude_sum: f32, power_sum: f32) {
        let count = self.symbols as f32;
        let mean = amplitude_sum / count;
        let spread = (power_sum / count - mean * mean).max(0.0).sqrt();
        let eb_no = EB_NO_OFFSET_DB + 20.0 * ((TINY + mean) / (TINY + spread)).log10();
        if eb_no.is_finite() {
            self.snr_db = SNR_SMOOTHING * self.snr_db + (1.0 - SNR_SMOOTHING) * eb_no;
        }
    }

    fn phases(&self) -> usize {
        (self.symbols + 1) * self.oversample
    }
}

fn samples_per_symbol(config: &TrackingMfskConfig) -> Result<usize, TrackingMfskError> {
    let TrackingMfskConfig {
        rate_hz: rate,
        baud,
        tones,
        oversample,
        block_symbols,
        ..
    } = *config;
    if baud == 0 || !rate.is_multiple_of(baud) || rate / baud < 2 {
        return Err(TrackingMfskError::Rate { rate, baud });
    }
    let sps = (rate / baud) as usize;
    if oversample == 0 || !sps.is_multiple_of(oversample) {
        return Err(TrackingMfskError::Oversample { oversample });
    }
    if !(2..=16).contains(&tones) || !tones.is_power_of_two() {
        return Err(TrackingMfskError::Tones { tones });
    }
    if block_symbols < 2 {
        return Err(TrackingMfskError::Block {
            symbols: block_symbols,
        });
    }
    Ok(sps)
}

impl Search {
    fn new(config: &TrackingMfskConfig) -> Result<Self, TrackingMfskError> {
        let rate = f64::from(config.rate_hz);
        let baud = f64::from(config.baud);
        let len = (rate / (BIN_SHARE_OF_BAUD * baud)).ceil().max(2.0) as usize;
        let len = len.next_power_of_two();
        let (low, high) = config.search_hz;
        let nyquist = config.rate_hz as f32 / 2.0;
        if !(low.is_finite()
            && high.is_finite()
            && low < high
            && low >= -nyquist
            && high <= nyquist)
        {
            return Err(TrackingMfskError::Search { low, high });
        }
        let bin = |hz: f32| (f64::from(hz) * len as f64 / rate) as isize + (len / 2) as isize;
        let bins = (
            bin(low).clamp(0, len as isize) as usize,
            bin(high).clamp(0, len as isize) as usize,
        );
        if bins.0 >= bins.1 {
            return Err(TrackingMfskError::Search { low, high });
        }
        Ok(Self {
            fft: Transform::forward(len),
            window: hann(len),
            buffer: vec![Complex::default(); len],
            smoothed: vec![0.0; len],
            scratch: vec![0.0; len],
            bins,
            blank: (BLANK_SHARE_OF_BAUD * baud * len as f64 / rate) as usize,
        })
    }

    fn estimate(&mut self, input: &[Complex<f32>], rate: f32, tones_hz: &mut [f32]) {
        let len = self.buffer.len();
        let half = len / 2;
        let spectra = (input.len() / half).saturating_sub(1);
        for spectrum in 0..spectra {
            let segment = &input[spectrum * half..spectrum * half + len];
            for ((slot, &sample), &weight) in self.buffer.iter_mut().zip(segment).zip(&self.window)
            {
                *slot = sample * weight;
            }
            self.fft.process(&mut self.buffer);
            for (bin, value) in self.buffer.iter().enumerate() {
                let shifted = &mut self.smoothed[(bin + half) % len];
                *shifted =
                    *shifted * (1.0 - SPECTRUM_SMOOTHING) + value.norm() * SPECTRUM_SMOOTHING;
            }
        }
        self.peaks(rate, tones_hz);
    }

    fn peaks(&mut self, rate: f32, tones_hz: &mut [f32]) {
        let len = self.buffer.len();
        self.scratch.copy_from_slice(&self.smoothed);
        let (start, end) = self.bins;
        for tone in tones_hz.iter_mut() {
            let mut best = start;
            for bin in start..end {
                if self.scratch[bin] > self.scratch[best] {
                    best = bin;
                }
            }
            let from = best.saturating_sub(self.blank);
            let to = (best + self.blank).min(len);
            self.scratch[from..to].fill(0.0);
            *tone = (best as f32 - (len / 2) as f32) * rate / len as f32;
        }
        tones_hz.sort_unstable_by(f32::total_cmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{add_noise, draws};

    const RATE: u32 = 8_000;
    const BAUD: u32 = 100;
    const SPACING: f32 = 270.0;

    fn wave(symbols: &[u8], low_hz: f32, delay: usize, rate: f32) -> Vec<Complex<f32>> {
        let sps = rate / BAUD as f32;
        let mut phase = 0.0f32;
        let total = (symbols.len() as f32 * sps) as usize;
        let mut out = vec![Complex::default(); delay];
        for n in 0..total {
            let symbol = symbols[((n as f32 / sps) as usize).min(symbols.len() - 1)];
            phase += TAU * (low_hz + SPACING * f32::from(symbol)) / rate;
            out.push(Complex::from_polar(1.0, phase));
        }
        out
    }

    fn demodulated(config: TrackingMfskConfig, iq: &[Complex<f32>]) -> Vec<u8> {
        let mut demod = TrackingMfsk::new(config).expect("valid config");
        let mut out = Vec::new();
        demod.push(iq, |symbol| out.push(symbol));
        out
    }

    fn holds(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    fn config(search_hz: (f32, f32)) -> TrackingMfskConfig {
        TrackingMfskConfig::new(RATE, BAUD, 4, search_hz)
    }

    #[test]
    fn a_clean_tone_lands_on_its_own_level() {
        let symbols: Vec<u8> = draws(1_000, 4, 7);
        let iq = wave(&symbols, -700.0, 37, RATE as f32);
        let mut demod = TrackingMfsk::new(config((-3_000.0, 3_000.0))).expect("valid config");
        let mut worst = 0.0f32;
        let mut seen = 0;
        demod.push_levels(&iq, |symbol, level| {
            seen += 1;
            if seen > 100 {
                worst = worst.max((level - (2.0 * f32::from(symbol) - 3.0)).abs());
            }
        });
        assert!(seen > 900, "only {seen} symbols");
        assert!(worst < 0.2, "a level strayed {worst} from its tone");
    }

    #[test]
    fn recovers_four_tones_off_centre_at_a_fractional_start() {
        let symbols: Vec<u8> = draws(1_000, 4, 7);
        let iq = wave(&symbols, -700.0, 37, RATE as f32);
        let out = demodulated(config((-3_000.0, 3_000.0)), &iq);
        assert!(holds(&out, &symbols[200..900]), "lost the symbols");
    }

    #[test]
    fn finds_the_tones_where_they_are() {
        let symbols: Vec<u8> = draws(400, 4, 3);
        let iq = wave(&symbols, 1_000.0, 0, RATE as f32);
        let mut demod = TrackingMfsk::new(config((100.0, 3_900.0))).expect("valid config");
        demod.push(&iq, |_| {});
        for (k, &hz) in demod.tones_hz().iter().enumerate() {
            let want = 1_000.0 + SPACING * k as f32;
            assert!((hz - want).abs() < 20.0, "tone {k} at {hz} Hz, want {want}");
        }
    }

    #[test]
    fn follows_a_sample_clock_off_by_200_ppm() {
        let symbols: Vec<u8> = draws(3_000, 4, 11);
        let iq = wave(&symbols, 300.0, 5, RATE as f32 * 1.000_2);
        let out = demodulated(config((-3_000.0, 3_000.0)), &iq);
        assert!(
            holds(&out, &symbols[2_000..2_900]),
            "lost lock on the clock"
        );
    }

    #[test]
    fn decides_through_noise() {
        let symbols: Vec<u8> = draws(1_500, 4, 5);
        let mut iq = wave(&symbols, -400.0, 13, RATE as f32);
        add_noise(&mut iq, 9, 4.0);
        let out = demodulated(config((-2_000.0, 2_000.0)), &iq);
        let tail = &symbols[500..1_400];
        let errors = (0..out.len().saturating_sub(tail.len()))
            .map(|at| {
                out[at..at + tail.len()]
                    .iter()
                    .zip(tail)
                    .filter(|(a, b)| a != b)
                    .count()
            })
            .min()
            .unwrap_or(tail.len());
        assert!(errors < tail.len() / 50, "{errors} symbol errors");
    }

    #[test]
    fn ignores_a_carrier_outside_the_search_band() {
        let symbols: Vec<u8> = draws(800, 4, 2);
        let mut iq = wave(&symbols, 500.0, 0, RATE as f32);
        for (n, sample) in iq.iter_mut().enumerate() {
            *sample += Complex::from_polar(3.0, -TAU * 2_500.0 * n as f32 / RATE as f32);
        }
        let out = demodulated(config((0.0, 3_900.0)), &iq);
        assert!(holds(&out, &symbols[200..700]), "the carrier pulled a tone");
    }

    #[test]
    fn refuses_what_it_cannot_run() {
        let bad = |config| TrackingMfsk::new(config).err();
        assert!(matches!(
            bad(TrackingMfskConfig::new(8_000, 300, 4, (0.0, 1.0))),
            Some(TrackingMfskError::Rate { .. })
        ));
        assert!(matches!(
            bad(TrackingMfskConfig::new(8_000, 100, 3, (0.0, 1.0))),
            Some(TrackingMfskError::Tones { tones: 3 })
        ));
        assert!(matches!(
            bad(TrackingMfskConfig::new(8_000, 100, 4, (0.0, 5_000.0))),
            Some(TrackingMfskError::Search { .. })
        ));
        let mut odd = TrackingMfskConfig::new(8_000, 100, 4, (0.0, 1.0));
        odd.oversample = 7;
        assert!(matches!(
            bad(odd),
            Some(TrackingMfskError::Oversample { oversample: 7 })
        ));
    }
}
