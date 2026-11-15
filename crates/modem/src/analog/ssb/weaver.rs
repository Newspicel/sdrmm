use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_lowpass};

use super::SsbParams;

/// The narrowest Weaver lowpass, in cycles per sample, so a zero-width band still designs.
const MIN_HALF_WIDTH: f64 = 1e-4;

struct Mixer {
    step: f64,
    phase: f64,
}

impl Mixer {
    fn new(freq: f64) -> Self {
        Self {
            step: TAU * freq,
            phase: 0.0,
        }
    }

    fn next(&mut self) -> Complex<f32> {
        let phasor = Complex::from_polar(1.0, self.phase);
        self.phase = (self.phase + self.step).rem_euclid(TAU);
        Complex::new(phasor.re as f32, phasor.im as f32)
    }
}

/// The Weaver stage: mix the band's centre to DC, lowpass it to half the band, flip it for the
/// lower sideband, mix it back up.
pub(super) struct Weaver {
    down: Mixer,
    up: Mixer,
    lowpass: Decimator,
    sign: f64,
    mixed: Vec<Complex<f32>>,
    cut: Vec<Complex<f32>>,
}

impl Weaver {
    /// A stage mixing down by `down` and up by `up`, both in band centres.
    pub(super) fn new(params: &SsbParams, down: f64, up: f64) -> Self {
        let (centre, half) = params.weaver();
        Self {
            down: Mixer::new(down * centre),
            up: Mixer::new(up * centre),
            lowpass: Decimator::new(
                &design_lowpass(params.band_taps, half.max(MIN_HALF_WIDTH)),
                1,
            ),
            sign: params.sideband.sign(),
            mixed: Vec::new(),
            cut: Vec::new(),
        }
    }

    /// Mixes `input` down, each sample by `mix` against the oscillator, and lowpasses it.
    pub(super) fn mix_down<T: Copy>(
        &mut self,
        input: &[T],
        mix: impl Fn(T, Complex<f32>) -> Complex<f32>,
    ) {
        self.mixed.clear();
        let down = &mut self.down;
        self.mixed
            .extend(input.iter().map(|&x| mix(x, down.next())));
        self.lowpass.process(&self.mixed, &mut self.cut);
    }

    /// The lowpassed band, flipped for the lower sideband and mixed back up.
    pub(super) fn mix_up(&mut self) -> impl Iterator<Item = Complex<f32>> + '_ {
        let (sign, up) = (self.sign, &mut self.up);
        self.cut.iter().map(move |&z| {
            let z = if sign > 0.0 { z } else { z.conj() };
            z * up.next()
        })
    }
}
