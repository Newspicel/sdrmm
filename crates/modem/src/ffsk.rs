//! Fast FSK: two audio tones keyed at the baud rate inside an FM voice channel. MDC-1200,
//! `FleetSync` and railroad End-of-Train telemetry all ride on it.

use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{BitSync, Decimator, FmDemod, Nco, RealDecimator, design_lowpass};

const AUDIO_TARGET_HZ: f64 = 12_000.0;
const AUDIO_TAPS: usize = 31;
const AUDIO_HEADROOM_HZ: f64 = 800.0;
const TONE_TAPS: usize = 31;
const TONE_CUTOFF_PER_BAUD: f64 = 5.0 / 6.0;
const CLIP: f32 = 1.2;

/// The tone pair and keying rate. Mark is the lower tone and slices as `true`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FfskTones {
    pub mark_hz: f64,
    pub space_hz: f64,
    pub baud: f64,
}

impl FfskTones {
    /// CCIR 1200 baud: 1200 Hz mark, 1800 Hz space.
    pub const CCIR_1200: Self = Self {
        mark_hz: 1_200.0,
        space_hz: 1_800.0,
        baud: 1_200.0,
    };
    /// 2400 baud MSK: 1200 Hz mark, 2400 Hz space.
    pub const MSK_2400: Self = Self {
        mark_hz: 1_200.0,
        space_hz: 2_400.0,
        baud: 2_400.0,
    };

    /// Phase-continuous audio of `bits` at `rate`, peaking at `level`.
    #[must_use]
    pub fn audio(&self, bits: &[bool], level: f32, rate: f64) -> Vec<f32> {
        let len = (bits.len() as f64 * rate / self.baud) as usize;
        let mut phase = 0.0f64;
        (0..len)
            .map(|k| {
                let at = ((k as f64 * self.baud / rate) as usize).min(bits.len() - 1);
                let hz = if bits[at] {
                    self.mark_hz
                } else {
                    self.space_hz
                };
                phase = (phase + TAU * hz / rate).rem_euclid(TAU);
                level * phase.sin() as f32
            })
            .collect()
    }
}

/// FM IQ or discriminator audio in, one soft decision per bit out, positive for mark. The
/// audio is mixed to the centre of the tone pair and frequency-discriminated, so the decision
/// does not depend on the tones' relative level.
pub struct FfskDemod {
    fm: FmDemod,
    audio: RealDecimator,
    mixer: Nco,
    tone: Decimator,
    discriminator: FmDemod,
    matched: RealDecimator,
    sync: BitSync,
    discriminated: Vec<f32>,
    decimated: Vec<f32>,
    mixed: Vec<Complex<f32>>,
    filtered: Vec<Complex<f32>>,
    frequency: Vec<f32>,
    smoothed: Vec<f32>,
}

impl FfskDemod {
    #[must_use]
    pub fn new(rate: f64, deviation_hz: f64, tones: FfskTones) -> Self {
        let decimation = ((rate / AUDIO_TARGET_HZ).floor() as usize).max(1);
        let audio_rate = rate / decimation as f64;
        let centre = f64::midpoint(tones.mark_hz, tones.space_hz);
        let per_bit = ((audio_rate / tones.baud).round() as usize).max(1);
        let audio_cutoff = (tones.space_hz + AUDIO_HEADROOM_HZ).min(audio_rate * 0.45);
        Self {
            fm: FmDemod::new(rate, deviation_hz),
            audio: RealDecimator::new(&design_lowpass(AUDIO_TAPS, audio_cutoff / rate), decimation),
            mixer: Nco::new(-centre as f32, audio_rate as f32),
            tone: Decimator::new(
                &design_lowpass(TONE_TAPS, tones.baud * TONE_CUTOFF_PER_BAUD / audio_rate),
                1,
            ),
            discriminator: FmDemod::new(audio_rate, (tones.space_hz - tones.mark_hz) / 2.0),
            matched: RealDecimator::new(&vec![1.0 / per_bit as f32; per_bit], 1),
            sync: BitSync::new(audio_rate, tones.baud),
            discriminated: Vec::new(),
            decimated: Vec::new(),
            mixed: Vec::new(),
            filtered: Vec::new(),
            frequency: Vec::new(),
            smoothed: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.audio.reset();
        self.mixer.reset();
        self.tone.reset();
        self.matched.reset();
        self.sync.reset();
    }

    /// Demodulates FM IQ and slices it into `out`, which is cleared first.
    pub fn symbols(&mut self, iq: &[Complex<f32>], out: &mut Vec<f32>) {
        self.fm.process(iq, &mut self.discriminated);
        for sample in &mut self.discriminated {
            *sample = sample.clamp(-CLIP, CLIP);
        }
        self.slice(out);
    }

    /// Slices discriminator audio, scaled so the peak deviation reads 1, into `out`.
    pub fn audio_symbols(&mut self, audio: &[f32], out: &mut Vec<f32>) {
        self.discriminated.clear();
        self.discriminated
            .extend(audio.iter().map(|sample| sample.clamp(-CLIP, CLIP)));
        self.slice(out);
    }

    fn slice(&mut self, out: &mut Vec<f32>) {
        out.clear();
        self.mixed.clear();
        self.audio.process(&self.discriminated, &mut self.decimated);
        let mixer = &mut self.mixer;
        self.mixed.extend(
            self.decimated
                .iter()
                .map(|&sample| Complex::new(sample, 0.0) * mixer.next_sample()),
        );
        self.tone.process(&self.mixed, &mut self.filtered);
        self.discriminator
            .process(&self.filtered, &mut self.frequency);
        self.matched.process(&self.frequency, &mut self.smoothed);
        for &sample in &self.smoothed {
            if let Some(symbol) = self.sync.push_soft(sample) {
                out.push(-symbol);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::fm_modulate;

    const RATE: f64 = 48_000.0;
    const DEVIATION_HZ: f64 = 3_000.0;

    fn pattern(count: usize) -> Vec<bool> {
        let mut state = 0x1234_5678u32;
        (0..count)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state & 1 == 1
            })
            .collect()
    }

    fn sliced(tones: FfskTones, sent: &[bool]) -> Vec<bool> {
        let mut bits = [true, false].repeat(32);
        bits.extend_from_slice(sent);
        bits.extend([true, false].repeat(8));
        let iq = fm_modulate(&tones.audio(&bits, 0.8, RATE), DEVIATION_HZ, RATE);
        let mut demod = FfskDemod::new(RATE, DEVIATION_HZ, tones);
        let mut out = Vec::new();
        let mut symbols = Vec::new();
        for block in iq.chunks(733) {
            demod.symbols(block, &mut out);
            symbols.extend(out.iter().map(|&s| s > 0.0));
        }
        symbols
    }

    fn contains(haystack: &[bool], needle: &[bool]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn ccir_1200_slices_every_bit() {
        let sent = pattern(400);
        assert!(contains(&sliced(FfskTones::CCIR_1200, &sent), &sent));
    }

    #[test]
    fn msk_2400_slices_every_bit() {
        let sent = pattern(400);
        assert!(contains(&sliced(FfskTones::MSK_2400, &sent), &sent));
    }

    #[test]
    fn mark_slices_positive() {
        let sent = vec![true; 64];
        let rx = sliced(FfskTones::CCIR_1200, &sent);
        assert!(contains(&rx, &sent));
    }

    #[test]
    fn audio_lasts_one_bit_per_baud() {
        let audio = FfskTones::MSK_2400.audio(&[true; 24], 1.0, RATE);
        assert_eq!(audio.len(), 480);
    }
}
