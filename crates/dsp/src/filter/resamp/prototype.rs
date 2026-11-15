use crate::filter::fir::{blackman_taps, design_lowpass, design_lowpass_kaiser, kaiser_taps};

/// The transition band a branch spans, as a share of the lower of the two rates.
const TRANSITION_SHARE: f64 = 0.1;

pub(crate) fn taps_per_phase(ratio: f64, prototype: Prototype) -> usize {
    prototype.taps(TRANSITION_SHARE * ratio.min(1.0))
}

/// The low-pass a resampler's branches are cut from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Prototype {
    /// About 74 dB of stopband, the shortest filter for audio and symbol-rate work.
    Blackman,
    /// Holds the given attenuation, for a chain that promises it.
    Kaiser(f64),
}

impl Prototype {
    pub(super) fn taps(self, transition: f64) -> usize {
        match self {
            Self::Blackman => blackman_taps(transition),
            Self::Kaiser(attenuation_db) => kaiser_taps(transition, attenuation_db),
        }
    }

    pub(super) fn lowpass(self, taps: usize, cutoff: f64) -> Vec<f32> {
        match self {
            Self::Blackman => design_lowpass(taps, cutoff),
            Self::Kaiser(attenuation_db) => design_lowpass_kaiser(taps, cutoff, attenuation_db),
        }
    }
}
