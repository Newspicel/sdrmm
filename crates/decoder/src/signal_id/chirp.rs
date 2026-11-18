use std::f64::consts::PI;

use num_complex::Complex;
use sdrmm_dsp::math::fft::Transform;
use sdrmm_wire::{ChirpSweep, Modulation};

use super::{classify::Verdict, detect::Band, features::Zoom};

const MIN_OCCUPANCY: f64 = 0.55;
const MAX_OCCUPANCY: f64 = 1.3;
const MIN_SYMBOLS: usize = 4;
const MAX_SYMBOLS: usize = 16;
const OFFSETS: usize = 2;
const MIN_GAIN: f32 = 3.5;
const MIN_LEAD: f32 = 1.6;
const MIN_CONFIDENCE: f32 = 0.75;
const NYQUIST_SHARE: f64 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chirp {
    pub bandwidth_hz: f64,
    pub spreading_factor: u8,
    pub inverted: bool,
    pub gain: f32,
}

impl Chirp {
    pub fn symbol_rate_hz(&self) -> f64 {
        self.bandwidth_hz / f64::from(1u32 << self.spreading_factor)
    }

    fn slope(&self) -> f64 {
        self.bandwidth_hz * self.symbol_rate_hz()
    }

    fn sweeps_like(&self, other: &Self) -> bool {
        self.inverted == other.inverted && (self.slope() / other.slope() - 1.0).abs() < 0.1
    }
}

pub fn verdict(chirp: Chirp) -> Verdict {
    Verdict {
        modulation: Modulation::Chirp,
        confidence: (MIN_CONFIDENCE + (chirp.gain - MIN_GAIN) / MIN_GAIN * (1.0 - MIN_CONFIDENCE))
            .min(1.0),
        sideband: None,
    }
}

/// The scratch a chirp search reuses window to window: a transform per symbol length, the
/// dechirp references and spectra, and the candidates scored.
#[derive(Default)]
pub struct Chirps {
    transforms: Vec<(usize, Transform)>,
    reference: Vec<Complex<f32>>,
    rising: Vec<Complex<f32>>,
    falling: Vec<Complex<f32>>,
    windows: Vec<(f32, usize)>,
    scored: Vec<Chirp>,
}

impl Chirps {
    /// The chirp `zoom` sweeps like, among the `sweeps` the registered decoders declare.
    pub fn find(&mut self, zoom: &Zoom, band: &Band, sweeps: &[ChirpSweep]) -> Option<Chirp> {
        let occupied_hz = band.bandwidth_hz;
        self.scored.clear();
        for sweep in sweeps {
            for &bandwidth_hz in sweep.bandwidths_hz.iter().filter(|&&bandwidth| {
                (MIN_OCCUPANCY..=MAX_OCCUPANCY).contains(&(occupied_hz / bandwidth))
                    && bandwidth <= zoom.rate * NYQUIST_SHARE
            }) {
                for spreading_factor in sweep.min_spreading_factor..=sweep.max_spreading_factor {
                    let up = Chirp {
                        bandwidth_hz,
                        spreading_factor,
                        inverted: false,
                        gain: 0.0,
                    };
                    if self.scored.iter().any(|seen| {
                        seen.bandwidth_hz == bandwidth_hz
                            && seen.spreading_factor == spreading_factor
                    }) {
                        continue;
                    }
                    if let Some((rising, falling)) = self.gains(zoom, &up) {
                        self.scored.push(Chirp { gain: rising, ..up });
                        self.scored.push(Chirp {
                            inverted: true,
                            gain: falling,
                            ..up
                        });
                    }
                }
            }
        }
        best(&mut self.scored, occupied_hz)
    }

    fn transform(&mut self, len: usize) -> usize {
        if let Some(at) = self.transforms.iter().position(|(size, _)| *size == len) {
            return at;
        }
        self.transforms.push((len, Transform::forward(len)));
        self.transforms.len() - 1
    }

    /// The dechirped concentration of `chirp` rising and falling. The falling reference is
    /// the rising one conjugated, so both share their windows, transform and reference.
    fn gains(&mut self, zoom: &Zoom, chirp: &Chirp) -> Option<(f32, f32)> {
        let chips = f64::from(1u32 << chirp.spreading_factor);
        let duration = chips / chirp.bandwidth_hz;
        let len = (duration * zoom.rate).round() as usize;
        if len == 0 || zoom.iq.len() / len <= MIN_SYMBOLS {
            return None;
        }
        reference(chirp, len, zoom.rate, &mut self.reference);
        let at = self.transform(len);
        self.rising.resize(len, Complex::new(0.0, 0.0));
        self.falling.resize(len, Complex::new(0.0, 0.0));
        let (mut best_rising, mut best_falling) = (0.0f32, 0.0f32);
        for offset in (0..OFFSETS).map(|k| k * len / OFFSETS) {
            loudest(&zoom.iq, offset, len, &mut self.windows);
            let (mut total_rising, mut total_falling) = (0.0f32, 0.0f32);
            for &(_, start) in &self.windows {
                for (((up, down), &sample), &swept) in self
                    .rising
                    .iter_mut()
                    .zip(self.falling.iter_mut())
                    .zip(&zoom.iq[start..start + len])
                    .zip(&self.reference)
                {
                    *up = sample * swept;
                    *down = sample * swept.conj();
                }
                let transform = &mut self.transforms[at].1;
                transform.process(&mut self.rising);
                transform.process(&mut self.falling);
                total_rising += concentration(&self.rising);
                total_falling += concentration(&self.falling);
            }
            let count = self.windows.len().max(1) as f32;
            best_rising = best_rising.max(total_rising / count);
            best_falling = best_falling.max(total_falling / count);
        }
        let noise = (len as f32).ln() + 0.577;
        Some((best_rising / noise, best_falling / noise))
    }
}

/// The strongest chirp, if it leads every chirp sweeping otherwise, settled on the width
/// nearest the occupied band among those sweeping alike.
fn best(scored: &mut [Chirp], occupied_hz: f64) -> Option<Chirp> {
    scored.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    let best = *scored.first()?;
    let rival = scored
        .iter()
        .find(|other| !best.sweeps_like(other))
        .map_or(1.0, |other| other.gain.max(1.0));
    if best.gain < MIN_GAIN || best.gain < rival * MIN_LEAD {
        return None;
    }
    scored
        .iter()
        .filter(|other| best.sweeps_like(other) && other.gain >= best.gain / MIN_LEAD)
        .min_by(|a, b| {
            (a.bandwidth_hz - occupied_hz)
                .abs()
                .total_cmp(&(b.bandwidth_hz - occupied_hz).abs())
        })
        .copied()
}

fn reference(chirp: &Chirp, len: usize, rate: f64, out: &mut Vec<Complex<f32>>) {
    let slope = chirp.slope();
    let sign = if chirp.inverted { -1.0 } else { 1.0 };
    out.clear();
    out.extend((0..len).map(|n| {
        let t = n as f64 / rate;
        let phase = sign * (PI * chirp.bandwidth_hz * t - PI * slope * t * t);
        Complex::from_polar(1.0, phase.rem_euclid(2.0 * PI) as f32)
    }));
}

/// The loudest symbol-long windows from `offset` on, at most [`MAX_SYMBOLS`] of them.
fn loudest(iq: &[Complex<f32>], offset: usize, len: usize, windows: &mut Vec<(f32, usize)>) {
    windows.clear();
    windows.extend(
        (offset..)
            .step_by(len)
            .take_while(|start| start + len <= iq.len())
            .map(|start| {
                let energy = iq[start..start + len]
                    .iter()
                    .map(num_complex::Complex::norm_sqr)
                    .sum();
                (energy, start)
            }),
    );
    windows.sort_by(|a, b| b.0.total_cmp(&a.0));
    windows.truncate(MAX_SYMBOLS);
}

fn concentration(spectrum: &[Complex<f32>]) -> f32 {
    let (peak, sum) = spectrum.iter().fold((0.0f32, 0.0f32), |(peak, sum), bin| {
        let power = bin.norm_sqr();
        (peak.max(power), sum + power)
    });
    if sum <= 0.0 {
        return 0.0;
    }
    peak * spectrum.len() as f32 / sum
}
