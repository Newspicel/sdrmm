//! Slow binary data keyed straight onto an FM carrier under the voice, as LTR and EDACS working
//! channels send it: read back from discriminator audio.

use sdrmm_dsp::{BitSync, DcBlocker, RealDecimator, design_lowpass};

/// Samples per bit the receiver times its bits at.
const SAMPLES_PER_BIT: f64 = 8.0;
const STAGE1_TAPS: usize = 63;
const STAGE2_TAPS: usize = 127;
const STAGE2_DECIM: usize = 2;
/// The first stage only has to keep voice from folding onto the data, so it cuts this many
/// bit rates up.
const STAGE1_CUTOFF_BAUDS: f64 = 2.0;
/// Where the data filter cuts, in bit rates: the fundamental and a little of the third
/// harmonic, below most of the voice.
const DATA_CUTOFF_BAUDS: f64 = 1.0;

/// Recovers the bits of a subaudible NRZ stream at `baud` from discriminator audio.
pub struct NrzReceiver {
    stage1: RealDecimator,
    stage2: RealDecimator,
    dc: DcBlocker,
    sync: BitSync,
    stage1_out: Vec<f32>,
    decimated: Vec<f32>,
}

impl NrzReceiver {
    /// # Panics
    /// When `baud` is not positive or `rate` is too low to carry it.
    #[must_use]
    pub fn new(rate: f64, baud: f64) -> Self {
        assert!(
            baud > 0.0 && rate >= 2.0 * SAMPLES_PER_BIT * baud,
            "the rate must carry {SAMPLES_PER_BIT} samples per bit after halving"
        );
        let stage1_decim = (rate / (SAMPLES_PER_BIT * baud * STAGE2_DECIM as f64))
            .floor()
            .max(1.0) as usize;
        let stage1_rate = rate / stage1_decim as f64;
        let data_rate = stage1_rate / STAGE2_DECIM as f64;
        Self {
            stage1: RealDecimator::new(
                &design_lowpass(STAGE1_TAPS, STAGE1_CUTOFF_BAUDS * baud / rate),
                stage1_decim,
            ),
            stage2: RealDecimator::new(
                &design_lowpass(STAGE2_TAPS, DATA_CUTOFF_BAUDS * baud / stage1_rate),
                STAGE2_DECIM,
            ),
            dc: DcBlocker::new(),
            sync: BitSync::new(data_rate, baud),
            stage1_out: Vec::new(),
            decimated: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.stage1.reset();
        self.stage2.reset();
        self.dc = DcBlocker::new();
        self.sync.reset();
    }

    /// Appends the bits `discriminator` completes, a rising frequency read as one.
    pub fn process(&mut self, discriminator: &[f32], bits: &mut Vec<bool>) {
        self.stage1.process(discriminator, &mut self.stage1_out);
        self.stage2.process(&self.stage1_out, &mut self.decimated);
        self.dc.process(&mut self.decimated);
        for &s in &self.decimated {
            if let Some(bit) = self.sync.push(s) {
                bits.push(bit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::tone_audio;

    const RATE: f64 = 48_000.0;
    const BAUD: f64 = 300.0;

    fn keyed(bits: &[bool]) -> Vec<f32> {
        let per_bit = (RATE / BAUD) as usize;
        bits.iter()
            .flat_map(|&bit| std::iter::repeat_n(if bit { 0.2 } else { -0.2 }, per_bit))
            .collect()
    }

    fn pattern(len: usize) -> Vec<bool> {
        let mut state = 0x5A5Au32;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state & 1 == 1
            })
            .collect()
    }

    fn found(heard: &[bool], sent: &[bool]) -> bool {
        heard.windows(sent.len()).any(|window| window == sent)
    }

    #[test]
    fn reads_back_data_under_voice() {
        let sent = pattern(400);
        let voice = tone_audio(1_000.0, 0.6, RATE, sent.len() * 160);
        let audio: Vec<f32> = keyed(&sent)
            .iter()
            .zip(&voice)
            .map(|(d, v)| d + v)
            .collect();
        let mut receiver = NrzReceiver::new(RATE, BAUD);
        let mut heard = Vec::new();
        for block in audio.chunks(1_000) {
            receiver.process(block, &mut heard);
        }
        assert!(found(&heard, &sent[100..380]), "{heard:?}");
    }

    #[test]
    fn a_carrier_offset_does_not_shift_the_slicer() {
        let sent = pattern(400);
        let audio: Vec<f32> = keyed(&sent).iter().map(|d| d + 0.3).collect();
        let mut receiver = NrzReceiver::new(RATE, BAUD);
        let mut heard = Vec::new();
        receiver.process(&audio, &mut heard);
        assert!(found(&heard, &sent[100..380]));
    }
}
