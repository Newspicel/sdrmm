use num_complex::Complex;

use super::{params::CssParams, phasor};

#[derive(Clone, Debug)]
pub struct CssMod {
    params: CssParams,
    base: Vec<Complex<f32>>,
    amplitude: f32,
}

impl CssMod {
    #[must_use]
    pub fn new(params: CssParams) -> Self {
        let base = params.base_chirp();
        let amplitude = (params.chips() as f32).sqrt().recip();
        Self {
            params,
            base,
            amplitude,
        }
    }

    #[must_use]
    pub fn params(&self) -> &CssParams {
        &self.params
    }

    pub fn frame(&self, preamble: &[u32], symbols: &[u32], out: &mut Vec<Complex<f32>>) {
        out.reserve((preamble.len() + symbols.len()) * self.params.chips());
        self.modulate(preamble, out);
        self.modulate(symbols, out);
    }

    pub fn modulate(&self, symbols: &[u32], out: &mut Vec<Complex<f32>>) {
        let n = self.params.chips();
        for &symbol in symbols {
            let shift = symbol as usize % n;
            for k in 0..n {
                let rotation = phasor((k * shift) as f64 / n as f64);
                out.push(self.base[k] * rotation * self.amplitude);
            }
        }
    }
}
