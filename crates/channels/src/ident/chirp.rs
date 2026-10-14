use std::f64::consts::PI;

use num_complex::Complex;
use sdrmm_dsp::fft::Transform;
use sdrmm_wire::{LoraBandwidth, LoraSpreadingFactor, Modulation};

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
pub(crate) struct Chirp {
    pub(crate) bandwidth_hz: f64,
    pub(crate) spreading_factor: u8,
    pub(crate) inverted: bool,
    pub(crate) gain: f32,
}

impl Chirp {
    pub(crate) fn symbol_rate_hz(&self) -> f64 {
        self.bandwidth_hz / f64::from(1u32 << self.spreading_factor)
    }

    fn slope(&self) -> f64 {
        self.bandwidth_hz * self.symbol_rate_hz()
    }

    fn sweeps_like(&self, other: &Self) -> bool {
        self.inverted == other.inverted && (self.slope() / other.slope() - 1.0).abs() < 0.1
    }
}

pub(crate) fn verdict(chirp: Chirp) -> Verdict {
    Verdict {
        modulation: Modulation::Chirp,
        confidence: (MIN_CONFIDENCE + (chirp.gain - MIN_GAIN) / MIN_GAIN * (1.0 - MIN_CONFIDENCE))
            .min(1.0),
        sideband: None,
    }
}

pub(crate) fn find(zoom: &Zoom, band: &Band) -> Option<Chirp> {
    let occupied_hz = band.bandwidth_hz;
    let mut scored: Vec<Chirp> = LoraBandwidth::ALL
        .iter()
        .map(|bandwidth| bandwidth.hz())
        .filter(|&bandwidth| {
            (MIN_OCCUPANCY..=MAX_OCCUPANCY).contains(&(occupied_hz / bandwidth))
                && bandwidth <= zoom.rate * NYQUIST_SHARE
        })
        .flat_map(|bandwidth_hz| {
            (LoraSpreadingFactor::MIN..=LoraSpreadingFactor::MAX).flat_map(
                move |spreading_factor| {
                    [false, true].map(|inverted| Chirp {
                        bandwidth_hz,
                        spreading_factor,
                        inverted,
                        gain: 0.0,
                    })
                },
            )
        })
        .filter_map(|chirp| {
            let gain = gain(zoom, &chirp)?;
            Some(Chirp { gain, ..chirp })
        })
        .collect();
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

fn gain(zoom: &Zoom, chirp: &Chirp) -> Option<f32> {
    let chips = f64::from(1u32 << chirp.spreading_factor);
    let duration = chips / chirp.bandwidth_hz;
    let len = (duration * zoom.rate).round() as usize;
    if len == 0 || zoom.iq.len() / len <= MIN_SYMBOLS {
        return None;
    }
    let reference = reference(chirp, len, zoom.rate);
    let mut transform = Transform::forward(len);
    let mut buffer = vec![Complex::new(0.0, 0.0); len];
    let mut best = 0.0f32;
    for offset in (0..OFFSETS).map(|k| k * len / OFFSETS) {
        let starts = loudest(&zoom.iq, offset, len);
        let mut total = 0.0f32;
        for &start in &starts {
            for ((slot, &sample), &chirp) in buffer
                .iter_mut()
                .zip(&zoom.iq[start..start + len])
                .zip(&reference)
            {
                *slot = sample * chirp;
            }
            transform.process(&mut buffer);
            total += concentration(&buffer);
        }
        best = best.max(total / starts.len().max(1) as f32);
    }
    let noise = (len as f32).ln() + 0.577;
    Some(best / noise)
}

fn reference(chirp: &Chirp, len: usize, rate: f64) -> Vec<Complex<f32>> {
    let slope = chirp.slope();
    let sign = if chirp.inverted { -1.0 } else { 1.0 };
    (0..len)
        .map(|n| {
            let t = n as f64 / rate;
            let phase = sign * (PI * chirp.bandwidth_hz * t - PI * slope * t * t);
            Complex::from_polar(1.0, phase.rem_euclid(2.0 * PI) as f32)
        })
        .collect()
}

fn loudest(iq: &[Complex<f32>], offset: usize, len: usize) -> Vec<usize> {
    let mut windows: Vec<(f32, usize)> = (offset..)
        .step_by(len)
        .take_while(|start| start + len <= iq.len())
        .map(|start| {
            let energy = iq[start..start + len].iter().map(|s| s.norm_sqr()).sum();
            (energy, start)
        })
        .collect();
    windows.sort_by(|a, b| b.0.total_cmp(&a.0));
    windows.truncate(MAX_SYMBOLS);
    windows.into_iter().map(|(_, start)| start).collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ident::{detect::Band, features},
        synth::lora::LoraWaveform,
        testutil::{cf32_le, complex_noise},
    };

    fn band(bandwidth_hz: f64) -> Band {
        Band {
            center_hz: 0.0,
            bandwidth_hz,
            snr_db: 10.0,
            carrier_db: 0.0,
            flatness: 0.9,
            skew: 0.0,
            peak_hz: 0.0,
        }
    }

    fn chirp_of(iq: &[Complex<f32>], rate: f64, occupied_hz: f64) -> Option<Chirp> {
        let band = band(occupied_hz);
        let zoom = features::zoom(iq, rate, &band)?;
        find(&zoom, &band)
    }

    #[test]
    fn a_real_rn2483_frame_reads_as_sf7_at_125_khz() {
        let iq = cf32_le(include_bytes!(
            "../../../../fixtures/lora_rn2483_sf7_250k.sigmf-data"
        ));
        let chirp = chirp_of(&iq, 250_000.0, 118_000.0).expect("chirps");
        assert_eq!(chirp.spreading_factor, 7);
        assert_eq!(chirp.bandwidth_hz, 125_000.0);
        assert!(!chirp.inverted);
    }

    #[test]
    fn identify_names_a_real_lora_frame_and_offers_the_lora_decoder() {
        let iq = cf32_le(include_bytes!(
            "../../../../fixtures/lora_rn2483_sf7_250k.sigmf-data"
        ));
        let band = band(118_000.0);
        let signal = crate::ident::identify(&iq, 250_000.0, &band, 868_100_000.0);
        assert_eq!(signal.modulation, Modulation::Chirp);
        assert_eq!(signal.bandwidth_hz, 125_000.0);
        assert_eq!(signal.symbol_rate_hz, Some(125_000.0 / 128.0));
        assert_eq!(
            signal.best().and_then(|best| best.type_id.as_deref()),
            Some("lora")
        );
    }

    #[test]
    fn a_weak_sf10_frame_is_found_under_the_noise() {
        let waveform = LoraWaveform {
            spreading_factor: 10,
            bandwidth_hz: 125_000.0,
            ..LoraWaveform::default()
        };
        let rate = 500_000.0;
        let mut iq = waveform.frame_at(&[0x42; 24], rate, 0.0);
        let noise = complex_noise(7, 1.8, iq.len());
        for (sample, noise) in iq.iter_mut().zip(noise) {
            *sample += noise;
        }
        let chirp = chirp_of(&iq, rate, 110_000.0).expect("chirps");
        assert_eq!(chirp.spreading_factor, 10);
        assert_eq!(chirp.bandwidth_hz, 125_000.0);
    }

    #[test]
    fn inverted_iq_reads_as_downchirps() {
        let waveform = LoraWaveform {
            inverted: true,
            ..LoraWaveform::default()
        };
        let rate = 500_000.0;
        let iq = waveform.frame_at(&[7; 16], rate, 0.0);
        let chirp = chirp_of(&iq, rate, 120_000.0).expect("chirps");
        assert!(chirp.inverted);
    }

    #[test]
    fn noise_and_fsk_are_not_chirps() {
        let rate = 500_000.0;
        assert!(chirp_of(&complex_noise(3, 1.0, 200_000), rate, 120_000.0).is_none());
        let fsk: Vec<Complex<f32>> = (0..200_000)
            .map(|n| {
                let bit = (n / 25) % 3 == 0;
                let shift = if bit { 40_000.0 } else { -40_000.0 };
                Complex::from_polar(1.0, (2.0 * PI * shift * n as f64 / rate) as f32)
            })
            .collect();
        assert!(chirp_of(&fsk, rate, 120_000.0).is_none());
    }
}
