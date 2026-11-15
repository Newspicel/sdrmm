use num_complex::Complex;

use super::{modulator::CpmMod, params::CpmParams};

pub(super) fn transmit(params: &CpmParams, symbols: &[u8]) -> Vec<Complex<f32>> {
    let mut modulator = CpmMod::new(params.clone());
    let mut out = Vec::new();
    modulator.modulate(symbols, &mut out);
    modulator.flush(&mut out);
    out
}
