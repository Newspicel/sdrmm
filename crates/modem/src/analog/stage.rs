use num_complex::Complex;
use sdrmm_dsp::{RealDecimator, design_lowpass};

use super::filter::Band;

/// Taps of the band and audio filters unless a caller asks for others.
pub(super) const DEFAULT_TAPS: usize = 129;
/// Damping of every carrier loop: critically flat.
pub(super) const LOOP_DAMPING: f64 = 0.707;

/// The message lowpass at `bandwidth` cycles per sample.
pub(super) fn message_filter(taps: usize, bandwidth: f64) -> RealDecimator {
    RealDecimator::new(&design_lowpass(taps, bandwidth), 1)
}

/// `iq` through the predetection band, or as it came without one.
pub(super) fn band_or_copy(
    band: Option<&mut Band>,
    iq: &[Complex<f32>],
    out: &mut Vec<Complex<f32>>,
) {
    if let Some(band) = band {
        band.process(iq, out);
    } else {
        out.clear();
        out.extend_from_slice(iq);
    }
}

/// `detected` through the audio filter, or as it came without one.
pub(super) fn audio_or_copy(
    audio: Option<&mut RealDecimator>,
    detected: &[f32],
    out: &mut Vec<f32>,
) {
    if let Some(audio) = audio {
        audio.process(detected, out);
    } else {
        out.clear();
        out.extend_from_slice(detected);
    }
}
