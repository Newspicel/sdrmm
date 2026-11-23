use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use sdrmm_audio_out::{CHANNELS, Voice};
use sdrmm_dsp::RealResampler;
use sdrmm_engine::{PcmBlock, PcmPayload};
use sdrmm_wire::audio::AUDIO_RATE_HZ;
use tokio::sync::broadcast::{self, error::RecvError};

const ATTENUATION_DB: f64 = 70.0;

/// Turns channel audio into the interleaved stereo an output plays, at the output's rate.
pub(super) struct Shaper {
    sides: Option<[RealResampler; CHANNELS]>,
    split: [Vec<f32>; CHANNELS],
    resampled: [Vec<f32>; CHANNELS],
    stereo: Vec<f32>,
}

impl Shaper {
    pub(super) fn new(rate: u32) -> Self {
        let sides = (rate != AUDIO_RATE_HZ).then(|| {
            let ratio = f64::from(rate) / f64::from(AUDIO_RATE_HZ);
            let edge = 0.5 * ratio.min(1.0);
            let side = || RealResampler::new(ratio, edge * 0.84, edge * 0.96, ATTENUATION_DB);
            [side(), side()]
        });
        Self {
            sides,
            split: Default::default(),
            resampled: Default::default(),
            stereo: Vec::new(),
        }
    }

    pub(super) fn shape(&mut self, block: &PcmBlock, gain: f32) -> &[f32] {
        self.split_sides(block);
        let Some(sides) = &mut self.sides else {
            return interleave(&self.split, gain, &mut self.stereo);
        };
        for ((side, input), output) in sides.iter_mut().zip(&self.split).zip(&mut self.resampled) {
            side.process(input, output);
        }
        interleave(&self.resampled, gain, &mut self.stereo)
    }

    fn split_sides(&mut self, block: &PcmBlock) {
        for side in &mut self.split {
            side.clear();
        }
        let [left, right] = &mut self.split;
        match &block.payload {
            PcmPayload::Silence(frames) => {
                left.resize(*frames, 0.0);
                right.resize(*frames, 0.0);
            }
            PcmPayload::Samples(samples) => {
                let lanes = usize::from(block.channels).max(1);
                for frame in samples.chunks_exact(lanes) {
                    left.push(frame[0]);
                    right.push(frame[usize::from(lanes > 1)]);
                }
            }
        }
    }
}

fn interleave<'a>(sides: &[Vec<f32>; CHANNELS], gain: f32, stereo: &'a mut Vec<f32>) -> &'a [f32] {
    stereo.clear();
    for (left, right) in sides[0].iter().zip(&sides[1]) {
        stereo.extend([left * gain, right * gain]);
    }
    stereo
}

/// Plays one wire into a voice until the channel stops or the speaker lets go.
pub(super) async fn run(
    mut pcm: broadcast::Receiver<PcmBlock>,
    mut voice: Voice,
    rate: u32,
    gain: Arc<AtomicU32>,
) {
    let mut shaper = Shaper::new(rate);
    let mut lost = 0u64;
    loop {
        match pcm.recv().await {
            Ok(block) => {
                let level = f32::from_bits(gain.load(Ordering::Relaxed));
                let dropped = voice.push(shaper.shape(&block, level));
                if dropped > 0 && voice.overflow() >= lost.saturating_mul(2).max(1) {
                    lost = voice.overflow();
                    tracing::warn!(
                        frames = lost,
                        "server speaker dropped audio the card could not take"
                    );
                }
            }
            Err(RecvError::Lagged(blocks)) => {
                tracing::warn!(
                    blocks,
                    "server speaker fell behind the channel and skipped audio"
                );
            }
            Err(RecvError::Closed) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn block(channels: u8, samples: &[f32]) -> PcmBlock {
        PcmBlock {
            start_frame: 0,
            channels,
            payload: PcmPayload::Samples(Arc::from(samples)),
        }
    }

    #[test]
    fn mono_plays_on_both_sides() {
        let mut shaper = Shaper::new(AUDIO_RATE_HZ);
        assert_eq!(
            shaper.shape(&block(1, &[0.5, -0.25]), 1.0),
            [0.5, 0.5, -0.25, -0.25]
        );
    }

    #[test]
    fn stereo_passes_through() {
        let mut shaper = Shaper::new(AUDIO_RATE_HZ);
        assert_eq!(shaper.shape(&block(2, &[0.5, -0.25]), 1.0), [0.5, -0.25]);
    }

    #[test]
    fn the_fader_scales_both_sides() {
        let mut shaper = Shaper::new(AUDIO_RATE_HZ);
        assert_eq!(shaper.shape(&block(2, &[0.5, -0.25]), 0.5), [0.25, -0.125]);
    }

    #[test]
    fn silence_keeps_its_length() {
        let mut shaper = Shaper::new(AUDIO_RATE_HZ);
        let silent = PcmBlock {
            start_frame: 0,
            channels: 1,
            payload: PcmPayload::Silence(3),
        };
        assert_eq!(shaper.shape(&silent, 1.0), [0.0; 6]);
    }

    #[test]
    fn another_card_rate_is_met() {
        let mut shaper = Shaper::new(44_100);
        let second = vec![0.1; AUDIO_RATE_HZ as usize];
        let frames = shaper.shape(&block(1, &second), 1.0).len() / CHANNELS;
        assert!(frames.abs_diff(44_100) < 100, "{frames}");
    }
}
