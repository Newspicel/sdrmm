use std::ops::RangeInclusive;

use sdrmm_dsp::{RealDecimator, ToneCorrelator, design_bandpass, design_lowpass};

use super::{AFSK_BAUD, AFSK_MARK_HZ, AFSK_SPACE_HZ, lane::Lane};

/// Lanes spread the space tone's weight evenly in dB, so one of them matches the twist a radio's
/// pre-emphasis or a receiver's de-emphasis left on the tones.
const TWIST_LANES: usize = 9;
const TWIST_SPAN_DB: f32 = 6.0;
/// The lane that weighs both tones alike, the one a scope watches.
const FLAT_LANE: usize = TWIST_LANES / 2;
const CLIP: f32 = 1.2;
const DECIMATION: usize = 4;
const CUTOFF_HZ: f64 = 4_800.0;
const TAPS: usize = 31;
const BAND_TAPS: usize = 31;
const BAND_LOW_HZ: f64 = 700.0;
const BAND_HIGH_HZ: f64 = 2_400.0;

fn twist_gains() -> impl Iterator<Item = f32> {
    let step = 2.0 * TWIST_SPAN_DB / (TWIST_LANES - 1) as f32;
    (0..TWIST_LANES).map(move |k| 10f32.powf((k as f32 * step - TWIST_SPAN_DB) / 20.0))
}

pub(super) struct Afsk {
    audio: RealDecimator,
    band: RealDecimator,
    decimated: Vec<f32>,
    filtered: Vec<f32>,
    mark: ToneCorrelator,
    space: ToneCorrelator,
    lanes: Vec<Lane>,
}

impl Afsk {
    pub(super) fn new(rate: f64, bytes: &RangeInclusive<usize>) -> Self {
        let audio_rate = rate / DECIMATION as f64;
        let window = (audio_rate / (AFSK_SPACE_HZ - AFSK_MARK_HZ)).round() as usize;
        Self {
            audio: RealDecimator::new(&design_lowpass(TAPS, CUTOFF_HZ / rate), DECIMATION),
            band: RealDecimator::new(
                &design_bandpass(
                    BAND_TAPS,
                    BAND_LOW_HZ / audio_rate,
                    BAND_HIGH_HZ / audio_rate,
                ),
                1,
            ),
            decimated: Vec::new(),
            filtered: Vec::new(),
            mark: ToneCorrelator::new(audio_rate, AFSK_MARK_HZ, window),
            space: ToneCorrelator::new(audio_rate, AFSK_SPACE_HZ, window),
            lanes: twist_gains()
                .map(|gain| Lane::new(audio_rate, AFSK_BAUD, gain, bytes))
                .collect(),
        }
    }

    pub(super) fn frames(
        &mut self,
        discriminated: &mut [f32],
        mut levels: Option<&mut Vec<f32>>,
        heard: &mut impl FnMut(&[u8]),
    ) {
        for v in &mut *discriminated {
            *v = v.clamp(-CLIP, CLIP);
        }
        self.audio.process(discriminated, &mut self.decimated);
        self.band.process(&self.decimated, &mut self.filtered);
        for &s in &self.filtered {
            let (m, sp) = (self.mark.push(s), self.space.push(s));
            for (k, lane) in self.lanes.iter_mut().enumerate() {
                let sample = m - lane.space_gain * sp;
                match levels.as_deref_mut() {
                    Some(levels) if k == FLAT_LANE => lane.push_watched(sample, levels, heard),
                    _ => lane.push(sample, heard),
                }
            }
        }
    }
}
