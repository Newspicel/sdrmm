use super::{SAMPLE_RATE_HZ, SpeechError};
use crate::{RealDecimator, RealInterpolator, design_lowpass};

const TAPS_PER_FACTOR: usize = 32;
const CUTOFF_PER_FACTOR: f64 = 0.45;

/// `pcm` at `rate` brought to the model's rate, which must be a whole multiple or divisor of it.
pub fn to_model_rate(pcm: &[f32], rate: u32) -> Result<Vec<f32>, SpeechError> {
    let target = SAMPLE_RATE_HZ;
    if rate == target {
        return Ok(pcm.to_vec());
    }
    if rate > 0 && target.is_multiple_of(rate) {
        return Ok(up(pcm, (target / rate) as usize));
    }
    if rate.is_multiple_of(target) {
        return Ok(down(pcm, (rate / target) as usize));
    }
    Err(SpeechError::Audio(format!(
        "cannot bring {rate} Hz audio to {target} Hz"
    )))
}

fn taps(factor: usize) -> Vec<f32> {
    design_lowpass(
        TAPS_PER_FACTOR * factor + 1,
        CUTOFF_PER_FACTOR / factor as f64,
    )
}

fn up(pcm: &[f32], factor: usize) -> Vec<f32> {
    let taps = taps(factor);
    let delay = (taps.len() - 1) / 2;
    let mut padded = pcm.to_vec();
    padded.resize(pcm.len() + delay.div_ceil(factor), 0.0);
    let mut out = Vec::new();
    RealInterpolator::new(&taps, factor).process(&padded, &mut out);
    out.into_iter()
        .skip(delay)
        .take(pcm.len() * factor)
        .collect()
}

fn down(pcm: &[f32], factor: usize) -> Vec<f32> {
    let taps = taps(factor);
    let delay = (taps.len() - 1) / 2;
    let mut padded = pcm.to_vec();
    padded.resize(pcm.len() + delay, 0.0);
    let mut out = Vec::new();
    RealDecimator::new(&taps, factor).process(&padded, &mut out);
    out.into_iter()
        .skip(delay / factor)
        .take(pcm.len() / factor)
        .collect()
}
