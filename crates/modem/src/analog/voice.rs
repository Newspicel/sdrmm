//! Land mobile FM voice from a discriminator, and whether a carrier is there to carry it.

use std::f64::consts::FRAC_1_SQRT_2;

use sdrmm_dsp::{
    Biquad, Deemphasis, RealDecimator, audio::clip::clamp_full_scale, deemphasis_gain,
    design_lowpass, one_pole_coeff,
};

use super::tone_squelch::AUDIO_CORNER_HZ;

/// The 750 us time constant land mobile FM pre-emphasises its voice with.
pub const EMPHASIS_TAU_US: f32 = 750.0;
/// Where de-emphasis leaves the level as it was.
pub const EMPHASIS_REFERENCE_HZ: f64 = 1_000.0;
pub const VOICE_CUTOFF_HZ: f64 = 3_400.0;

const VOICE_TAPS: usize = 129;
const SUBAUDIBLE_SECTIONS: usize = 2;

/// Cuts the subaudible band, tones and slow data alike, out from under voice.
pub struct SubaudibleCut {
    sections: [Biquad; SUBAUDIBLE_SECTIONS],
}

impl SubaudibleCut {
    #[must_use]
    pub fn new(rate: f64) -> Self {
        Self {
            sections: std::array::from_fn(|_| {
                Biquad::highpass(rate, AUDIO_CORNER_HZ, FRAC_1_SQRT_2)
            }),
        }
    }

    pub fn reset(&mut self) {
        for section in &mut self.sections {
            section.reset();
        }
    }

    pub fn process(&mut self, audio: &mut [f32]) {
        for section in &mut self.sections {
            section.process(audio);
        }
    }
}

/// Undoes the transmitter's pre-emphasis, level at [`EMPHASIS_REFERENCE_HZ`].
pub struct VoiceDeemphasis {
    rate: f64,
    filter: Deemphasis,
    gain: f32,
}

impl VoiceDeemphasis {
    #[must_use]
    pub fn new(rate: f64) -> Self {
        Self {
            rate,
            filter: Deemphasis::new(rate, EMPHASIS_TAU_US),
            gain: 1.0 / deemphasis_gain(rate, EMPHASIS_TAU_US, EMPHASIS_REFERENCE_HZ),
        }
    }

    pub fn reset(&mut self) {
        self.filter = Deemphasis::new(self.rate, EMPHASIS_TAU_US);
    }

    pub fn process(&mut self, audio: &mut [f32]) {
        self.filter.process(audio);
        for s in audio {
            *s *= self.gain;
        }
    }
}

/// Speech band-limited at `rate`, the rate it is played at.
#[must_use]
pub fn voice_lowpass(rate: f64) -> RealDecimator {
    RealDecimator::new(&design_lowpass(VOICE_TAPS, VOICE_CUTOFF_HZ / rate), 1)
}

/// The voice a trunked analog channel plays: subaudible signalling cut, de-emphasised and
/// band-limited to speech, at the rate the discriminator ran at.
pub struct FmVoice {
    cut: SubaudibleCut,
    deemphasis: VoiceDeemphasis,
    lowpass: RealDecimator,
    shaped: Vec<f32>,
}

impl FmVoice {
    #[must_use]
    pub fn new(rate: f64) -> Self {
        Self {
            cut: SubaudibleCut::new(rate),
            deemphasis: VoiceDeemphasis::new(rate),
            lowpass: voice_lowpass(rate),
            shaped: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.cut.reset();
        self.deemphasis.reset();
        self.lowpass.reset();
    }

    /// Replaces `out` with the voice in `discriminator`, scaled so full deviation is full scale.
    pub fn process(&mut self, discriminator: &[f32], out: &mut Vec<f32>) {
        self.shaped.clear();
        self.shaped.extend_from_slice(discriminator);
        self.cut.process(&mut self.shaped);
        self.deemphasis.process(&mut self.shaped);
        self.lowpass.process(&self.shaped, out);
        clamp_full_scale(out);
    }
}

/// A discriminator past this many deviations has been thrown by noise, not keyed.
const WILD_DEVIATIONS: f32 = 1.8;
const WILD_TAU_S: f64 = 0.03;
/// The share of wild samples under which a carrier is taken to be up, and over which down.
const OPEN_BELOW: f32 = 0.04;
const CLOSE_ABOVE: f32 = 0.1;

/// Tells a keyed FM carrier from noise by how often the discriminator swings wild: a carrier
/// stays within its deviation, noise throws the phase about whatever its level.
pub struct Quieting {
    alpha: f32,
    wild: f32,
    open: bool,
}

impl Quieting {
    #[must_use]
    pub fn new(rate: f64) -> Self {
        Self {
            alpha: one_pole_coeff(rate, WILD_TAU_S),
            wild: 1.0,
            open: false,
        }
    }

    pub fn reset(&mut self) {
        self.wild = 1.0;
        self.open = false;
    }

    /// Takes discriminator output scaled to one per deviation; reports whether a carrier holds
    /// at the end of it.
    pub fn process(&mut self, discriminator: &[f32]) -> bool {
        for &s in discriminator {
            let wild = if s.abs() > WILD_DEVIATIONS { 1.0 } else { 0.0 };
            self.wild += self.alpha * (wild - self.wild);
        }
        if self.open {
            self.open = self.wild < CLOSE_ABOVE;
        } else {
            self.open = self.wild < OPEN_BELOW;
        }
        self.open
    }
}

#[cfg(test)]
mod tests {
    use num_complex::Complex;
    use sdrmm_dsp::FmDemod;

    use super::*;
    use crate::synth::{add_noise, fm_modulate, silence, tone_audio};

    const RATE: f64 = 48_000.0;
    const DEVIATION_HZ: f64 = 2_500.0;

    fn discriminated(iq: &[Complex<f32>]) -> Vec<f32> {
        let mut out = Vec::new();
        FmDemod::new(RATE, DEVIATION_HZ).process(iq, &mut out);
        out
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn a_keyed_carrier_opens_and_noise_does_not() {
        let tone = tone_audio(1_000.0, 0.7, RATE, 24_000);
        let mut keyed = fm_modulate(&tone, DEVIATION_HZ, RATE);
        add_noise(&mut keyed, 3, 0.2);
        let mut quieting = Quieting::new(RATE);
        assert!(quieting.process(&discriminated(&keyed)));

        let mut noise = silence(24_000);
        add_noise(&mut noise, 5, 1.0);
        assert!(!quieting.process(&discriminated(&noise)));
    }

    #[test]
    fn voice_passes_and_subaudible_signalling_does_not() {
        let mut voice = FmVoice::new(RATE);
        let mut out = Vec::new();
        voice.process(&tone_audio(1_000.0, 0.5, RATE, 24_000), &mut out);
        assert!((rms(&out[4_800..]) - 0.5 / 2f32.sqrt()).abs() < 0.03);

        voice.reset();
        voice.process(&tone_audio(100.0, 0.5, RATE, 24_000), &mut out);
        assert!(rms(&out[4_800..]) < 0.02);
    }
}
