use num_complex::Complex;
use sdrmm_dsp::Decibels;

use super::{
    error::OfdmError,
    pilot::PilotPattern,
    preamble::Preamble,
    subcarrier::{Subcarrier, SubcarrierMap},
};

const MIN_REPEATS: usize = 2;
const REFERENCE_FFT: usize = 64;
const REFERENCE_CP: usize = 16;
const REFERENCE_PREAMBLE: Preamble = Preamble {
    short_stride: 4,
    short_repeats: 10,
    long_repeats: 2,
    long_guard: 32,
};
const WIFI_EDGE: i32 = 26;
const WIFI_PILOTS: [i32; 4] = [-21, -7, 7, 21];
const WIFI_PILOT_SIGNS: [f32; 4] = [1.0, 1.0, 1.0, -1.0];
const DMT_PILOTS: [i32; 2] = [7, 21];
const DMT_PILOT_SIGNS: [f32; 2] = [1.0, -1.0];
/// Multipliers of the integer hash that scatters the training signs over the subcarrier offsets.
const TRAINING_HASH: [u32; 2] = [0x9E37_79B9, 0x85EB_CA6B];
const TRAINING_HASH_SHIFTS: [u32; 2] = [16, 13];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    Complex,
    RealHermitian,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OfdmParams {
    fft: usize,
    cp: usize,
    map: SubcarrierMap,
    pilots: PilotPattern,
    preamble: Preamble,
    domain: Domain,
    short_bins: Vec<Subcarrier>,
}

impl OfdmParams {
    pub fn new(
        cp: usize,
        map: SubcarrierMap,
        pilots: PilotPattern,
        preamble: Preamble,
        domain: Domain,
    ) -> Result<Self, OfdmError> {
        let fft = map.fft();
        validate(fft, cp, &map, &pilots, preamble)?;
        if domain == Domain::RealHermitian {
            map.is_hermitian_capable()?;
        }
        let short_bins: Vec<Subcarrier> = map
            .occupied()
            .iter()
            .copied()
            .filter(|c| {
                c.offset != 0
                    && (c.offset.unsigned_abs() as usize).is_multiple_of(preamble.short_stride)
            })
            .collect();
        if short_bins.is_empty() {
            return Err(OfdmError::ShortTrainingEmpty);
        }
        Ok(Self {
            fft,
            cp,
            map,
            pilots,
            preamble,
            domain,
            short_bins,
        })
    }

    #[must_use]
    pub fn wifi_like() -> Self {
        let data: Vec<i32> = (-WIFI_EDGE..=WIFI_EDGE)
            .filter(|k| *k != 0 && !WIFI_PILOTS.contains(k))
            .collect();
        match reference(&data, &WIFI_PILOTS, &WIFI_PILOT_SIGNS, Domain::Complex) {
            Ok(params) => params,
            Err(why) => panic!("the reference OFDM configuration is invalid: {why}"),
        }
    }

    #[must_use]
    pub fn dmt_like() -> Self {
        let data: Vec<i32> = (1..=WIFI_EDGE)
            .filter(|k| !DMT_PILOTS.contains(k))
            .collect();
        match reference(&data, &DMT_PILOTS, &DMT_PILOT_SIGNS, Domain::RealHermitian) {
            Ok(params) => params,
            Err(why) => panic!("the reference DMT configuration is invalid: {why}"),
        }
    }

    #[must_use]
    pub fn fft(&self) -> usize {
        self.fft
    }

    #[must_use]
    pub fn cp(&self) -> usize {
        self.cp
    }

    #[must_use]
    pub fn symbol_samples(&self) -> usize {
        self.fft + self.cp
    }

    #[must_use]
    pub fn map(&self) -> &SubcarrierMap {
        &self.map
    }

    #[must_use]
    pub fn pilot_pattern(&self) -> &PilotPattern {
        &self.pilots
    }

    #[must_use]
    pub fn preamble(&self) -> Preamble {
        self.preamble
    }

    #[must_use]
    pub fn domain(&self) -> Domain {
        self.domain
    }

    #[must_use]
    pub fn data_subcarriers(&self) -> usize {
        self.map.data().len()
    }

    #[must_use]
    pub fn data_offset(&self) -> usize {
        self.preamble.samples(self.fft)
    }

    #[must_use]
    pub fn frame_samples(&self, symbols: usize) -> usize {
        self.data_offset() + symbols * self.symbol_samples()
    }

    #[must_use]
    pub fn short_bins(&self) -> &[Subcarrier] {
        &self.short_bins
    }

    #[must_use]
    pub fn long_training(&self, index: usize) -> Complex<f32> {
        Complex::new(training_sign(self.map.occupied()[index].offset), 0.0)
    }

    #[must_use]
    pub fn short_training(&self, index: usize) -> Complex<f32> {
        let scale = (self.map.occupied().len() as f64 / self.short_bins.len() as f64).sqrt()
            / std::f64::consts::SQRT_2;
        let value = (f64::from(training_sign(self.short_bins[index].offset)) * scale) as f32;
        Complex::new(value, value)
    }

    #[must_use]
    pub fn spectral_copies(&self) -> f64 {
        match self.domain {
            Domain::Complex => 1.0,
            Domain::RealHermitian => 2.0,
        }
    }

    #[must_use]
    pub fn frame_energy(&self, symbols: usize) -> f64 {
        self.spectral_copies()
            * self.map.occupied().len() as f64
            * self.frame_samples(symbols) as f64
            / self.fft as f64
    }

    #[must_use]
    pub fn framing_overhead_db(&self, symbols: usize) -> f64 {
        let carried = (symbols * self.data_subcarriers()) as f64;
        (self.frame_energy(symbols) / carried).power_to_db()
    }
}

fn validate(
    fft: usize,
    cp: usize,
    map: &SubcarrierMap,
    pilots: &PilotPattern,
    preamble: Preamble,
) -> Result<(), OfdmError> {
    if cp == 0 || cp >= fft {
        return Err(OfdmError::CyclicPrefix { fft, cp });
    }
    if map.pilots().len() != pilots.len() {
        return Err(OfdmError::PilotCountMismatch {
            pilots: map.pilots().len(),
            values: pilots.len(),
        });
    }
    if preamble.short_stride == 0 || !fft.is_multiple_of(preamble.short_stride) {
        return Err(OfdmError::ShortStride {
            fft,
            stride: preamble.short_stride,
        });
    }
    for (what, repeats) in [
        ("short training", preamble.short_repeats),
        ("long training", preamble.long_repeats),
    ] {
        if repeats < MIN_REPEATS {
            return Err(OfdmError::Repeats { what, repeats });
        }
    }
    if preamble.long_guard > fft {
        return Err(OfdmError::LongGuard {
            fft,
            guard: preamble.long_guard,
        });
    }
    Ok(())
}

fn reference(
    data: &[i32],
    pilots: &[i32],
    signs: &[f32],
    domain: Domain,
) -> Result<OfdmParams, OfdmError> {
    let map = SubcarrierMap::new(REFERENCE_FFT, data, pilots)?;
    let base = signs.iter().map(|&sign| Complex::new(sign, 0.0)).collect();
    let pattern = PilotPattern::m_sequence(base)?;
    OfdmParams::new(REFERENCE_CP, map, pattern, REFERENCE_PREAMBLE, domain)
}

fn training_sign(offset: i32) -> f32 {
    let mut h = (offset as u32).wrapping_mul(TRAINING_HASH[0]);
    h ^= h >> TRAINING_HASH_SHIFTS[0];
    h = h.wrapping_mul(TRAINING_HASH[1]);
    h ^= h >> TRAINING_HASH_SHIFTS[1];
    if h & 1 == 1 { 1.0 } else { -1.0 }
}

#[cfg(test)]
mod tests;
