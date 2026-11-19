use num_complex::Complex;
use sdrmm_dsp::{FmDemod, MovingAverage, demod::sync::CrossingClock};

use super::model::WenetMode;

pub(crate) const CHUNK: usize = 4_096;

/// Scales the discriminator so the tones land near plus and minus one.
const NOMINAL_DEVIATION_HZ: f64 = 70_000.0;
/// Most of a symbol at both baud rates, the matched filter of a rectangular pulse.
const SMOOTHING_SAMPLES: usize = 6;
const DC_TRACK_SAMPLES: f32 = 8_192.0;
const TIMING_GAIN: f64 = 0.05;

/// FM discriminator, symbol-length smoothing and frequency-offset removal.
pub(crate) struct FrontEnd {
    fm: FmDemod,
    smooth: MovingAverage,
    dc: f32,
    audio: Vec<f32>,
}

impl FrontEnd {
    pub(crate) fn new(rate: f64) -> Self {
        Self {
            fm: FmDemod::new(rate, NOMINAL_DEVIATION_HZ),
            smooth: MovingAverage::new(SMOOTHING_SAMPLES),
            dc: 0.0,
            audio: Vec::with_capacity(CHUNK),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.dc = 0.0;
    }

    /// Frequency per sample, centred on the middle of the two tones.
    pub(crate) fn demodulate(&mut self, iq: &[Complex<f32>]) -> &[f32] {
        self.fm.process(iq, &mut self.audio);
        for sample in &mut self.audio {
            let smoothed = self.smooth.push(*sample);
            self.dc += (smoothed - self.dc) / DC_TRACK_SAMPLES;
            *sample = smoothed - self.dc;
        }
        &self.audio
    }
}

/// The symbol clock for one framing.
pub(crate) fn symbol_clock(rate: f64, mode: WenetMode) -> CrossingClock {
    CrossingClock::new(rate, mode.baud(), TIMING_GAIN)
}
