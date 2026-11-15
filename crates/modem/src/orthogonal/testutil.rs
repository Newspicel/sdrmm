use super::{
    modulator::{MfskMod, TonePhase},
    params::MfskParams,
};
use num_complex::Complex;

pub(super) fn wave(params: &MfskParams, policy: TonePhase, symbols: &[u8]) -> Vec<Complex<f32>> {
    let mut modulator = MfskMod::new(params.clone(), policy);
    let mut out = Vec::new();
    modulator.modulate(symbols, &mut out);
    modulator.flush(&mut out);
    out
}
