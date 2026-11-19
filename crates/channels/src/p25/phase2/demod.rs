use num_complex::Complex;
use sdrmm_dsp::Nco;
use sdrmm_modem::constellation::{Constellation, ConstellationError, tables};
use sdrmm_modem::linear::{
    DifferentialDetector, LinearDemod, LinearError, LinearParams, LinearTiming,
};
use sdrmm_modem::pulse::{self, Norm};

pub(crate) const SYMBOL_RATE: f64 = 6_000.0;
pub(crate) const INPUT_RATE_HZ: f64 = 48_000.0;
const SPS: usize = 8;
pub(crate) const RRC_ALPHA: f64 = 0.2;
const RRC_SPAN: usize = 8;

const FREQUENCY_MEMORY_SYMBOLS: f32 = 256.0;
/// The coarse offset averages the sample-to-sample turn over a tenth of a second.
const COARSE_MEMORY_SAMPLES: f32 = 4_800.0;
const LEVEL_MEMORY_SYMBOLS: f32 = 64.0;
const SOFT_SCALE: f32 = 64.0;

/// H-DQPSK: the carrier brought near zero by the mean turn between samples, a matched filter and
/// symbol timing, then each symbol against the one before, with the rest of the carrier taken
/// out of the differential phase by its fourth power.
pub(crate) struct Demod {
    coarse: Complex<f32>,
    coarse_samples: f32,
    previous: Complex<f32>,
    nco: Nco,
    centred: Vec<Complex<f32>>,
    linear: LinearDemod,
    differential: DifferentialDetector,
    symbols: Vec<Complex<f32>>,
    products: Vec<Complex<f32>>,
    rotation: Complex<f32>,
    level: f32,
}

#[derive(Debug)]
pub(crate) enum DemodError {
    Constellation(ConstellationError),
    Waveform(LinearError),
}

impl std::fmt::Display for DemodError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Constellation(error) => write!(f, "H-DQPSK table: {error}"),
            Self::Waveform(error) => write!(f, "H-DQPSK waveform: {error}"),
        }
    }
}

pub(crate) fn constellation() -> Result<Constellation, DemodError> {
    tables::psk_rotated(4, std::f64::consts::FRAC_PI_4).map_err(DemodError::Constellation)
}

impl Demod {
    pub(crate) fn new() -> Result<Self, DemodError> {
        let pulse = pulse::root_raised_cosine(SPS as f64, RRC_ALPHA, RRC_SPAN, Norm::Energy);
        let params = LinearParams::new(constellation()?, pulse.clone(), SPS)
            .map_err(DemodError::Waveform)?;
        Ok(Self {
            coarse: Complex::new(0.0, 0.0),
            coarse_samples: 0.0,
            previous: Complex::new(0.0, 0.0),
            nco: Nco::new(0.0, INPUT_RATE_HZ as f32),
            centred: Vec::new(),
            linear: LinearDemod::new(&params, &pulse, LinearTiming::CONTINUOUS, None),
            differential: DifferentialDetector::new(),
            symbols: Vec::new(),
            products: Vec::new(),
            rotation: Complex::new(0.0, 0.0),
            level: 0.0,
        })
    }

    pub(crate) fn reset(&mut self) {
        self.coarse = Complex::new(0.0, 0.0);
        self.coarse_samples = 0.0;
        self.previous = Complex::new(0.0, 0.0);
        self.nco = Nco::new(0.0, INPUT_RATE_HZ as f32);
        self.linear.reset();
        self.differential.reset();
        self.rotation = Complex::new(0.0, 0.0);
        self.level = 0.0;
    }

    pub(crate) fn frequency_offset_hz(&self) -> f64 {
        self.coarse_offset_hz()
            + f64::from(self.rotation.arg()) / 4.0 * SYMBOL_RATE / std::f64::consts::TAU
    }

    fn coarse_offset_hz(&self) -> f64 {
        f64::from(self.coarse.arg()) * INPUT_RATE_HZ / std::f64::consts::TAU
    }

    fn centre(&mut self, iq: &[Complex<f32>]) {
        for &sample in iq {
            self.coarse_samples = (self.coarse_samples + 1.0).min(COARSE_MEMORY_SAMPLES);
            self.coarse += (sample * self.previous.conj() - self.coarse) / self.coarse_samples;
            self.previous = sample;
        }
        self.nco
            .set_freq(-self.coarse_offset_hz() as f32, INPUT_RATE_HZ as f32);
        self.centred.clear();
        self.centred.extend_from_slice(iq);
        self.nco.mix(&mut self.centred);
    }

    /// Appends one differential phasor per symbol, near unit size, its phase one of ±π/4 and
    /// ±3π/4.
    pub(crate) fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        self.symbols.clear();
        self.products.clear();
        self.centre(iq);
        self.linear.process(&self.centred, &mut self.symbols);
        self.differential.process(&self.symbols, &mut self.products);
        for index in 0..self.products.len() {
            let corrected = self.correct(self.products[index]);
            out.push(corrected);
        }
    }

    fn correct(&mut self, product: Complex<f32>) -> Complex<f32> {
        let magnitude = product.norm();
        if magnitude <= 0.0 || !magnitude.is_finite() {
            return Complex::new(0.0, 0.0);
        }
        let unit = product / magnitude;
        let fourth = unit * unit * unit * unit;
        self.rotation += (-fourth - self.rotation) / FREQUENCY_MEMORY_SYMBOLS;
        self.level += (magnitude - self.level) / LEVEL_MEMORY_SYMBOLS;
        let correction = if self.rotation.norm_sqr() > 0.0 {
            Complex::from_polar(1.0, -self.rotation.arg() / 4.0)
        } else {
            Complex::new(1.0, 0.0)
        };
        product * correction / self.level.max(f32::MIN_POSITIVE)
    }
}

/// The dibit a differential phasor stands for: +π/4 is 0, +3π/4 is 1, −π/4 is 2, −3π/4 is 3.
pub(crate) fn dibit(phasor: Complex<f32>) -> u8 {
    u8::from(phasor.im < 0.0) << 1 | u8::from(phasor.re < 0.0)
}

/// The two bits of a phasor, high first, positive for a one.
pub(crate) fn soft_bits(phasor: Complex<f32>) -> [i8; 2] {
    [soft(-phasor.im), soft(-phasor.re)]
}

fn soft(value: f32) -> i8 {
    (value * SOFT_SCALE).clamp(-f32::from(i8::MAX), f32::from(i8::MAX)) as i8
}

/// A phasor turned back by `quarters` quarter turns.
pub(crate) fn unturn(phasor: Complex<f32>, quarters: u8) -> Complex<f32> {
    match quarters & 3 {
        0 => phasor,
        1 => Complex::new(phasor.im, -phasor.re),
        2 => -phasor,
        _ => Complex::new(-phasor.im, phasor.re),
    }
}
