//! Opus for `TeamSpeak`: 20 ms mono frames at 48 kHz, tuned for music since radio carries it.

use opus::{Application, Channels};

use crate::{FRAME, RATE_HZ};

/// Largest Opus packet a 20 ms voice frame needs.
const MAX_PACKET: usize = 1275;
/// Longest Opus frame, 120 ms.
const MAX_DECODED: usize = FRAME * 6;

pub(crate) struct Encoder(opus::Encoder);

impl Encoder {
    pub(crate) fn new() -> Result<Self, opus::Error> {
        opus::Encoder::new(RATE_HZ, Channels::Mono, Application::Audio).map(Self)
    }

    pub(crate) fn encode(&mut self, frame: &[f32]) -> Result<Vec<u8>, opus::Error> {
        self.0.encode_vec_float(frame, MAX_PACKET)
    }
}

pub(crate) struct Decoder {
    inner: opus::Decoder,
    scratch: Vec<f32>,
}

impl Decoder {
    pub(crate) fn new() -> Result<Self, opus::Error> {
        Ok(Self {
            inner: opus::Decoder::new(RATE_HZ, Channels::Mono)?,
            scratch: vec![0.0; MAX_DECODED],
        })
    }

    pub(crate) fn decode(&mut self, packet: &[u8]) -> Result<Vec<f32>, opus::Error> {
        let samples = self.inner.decode_float(packet, &mut self.scratch, false)?;
        Ok(self.scratch[..samples].to_vec())
    }
}

/// Cuts samples of any length into whole frames.
#[derive(Default)]
pub(crate) struct Framer {
    pending: Vec<f32>,
}

impl Framer {
    pub(crate) fn push(&mut self, samples: &[f32]) {
        self.pending.extend_from_slice(samples);
    }

    pub(crate) fn pop(&mut self) -> Option<Vec<f32>> {
        (self.pending.len() >= FRAME).then(|| self.pending.drain(..FRAME).collect())
    }

    /// The partial frame left, padded with silence.
    pub(crate) fn flush(&mut self) -> Option<Vec<f32>> {
        if self.pending.is_empty() {
            return None;
        }
        self.pending.resize(FRAME, 0.0);
        self.pop()
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    fn tone(frame: usize) -> Vec<f32> {
        (0..FRAME)
            .map(|i| {
                let t = (frame * FRAME + i) as f32 / RATE_HZ as f32;
                0.5 * (TAU * 1000.0 * t).sin()
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    fn zero_crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
            .count()
    }

    #[test]
    fn opus_round_trip_keeps_a_tone() {
        let mut encoder = Encoder::new().unwrap();
        let mut decoder = Decoder::new().unwrap();
        let mut last = Vec::new();
        for frame in 0..10 {
            let packet = encoder.encode(&tone(frame)).unwrap();
            assert!(packet.len() <= MAX_PACKET);
            last = decoder.decode(&packet).unwrap();
        }
        assert_eq!(last.len(), FRAME);
        let level = rms(&last);
        assert!((level - 0.5 / 2.0_f32.sqrt()).abs() < 0.07, "rms {level}");
        assert!((36..=44).contains(&zero_crossings(&last)), "not 1 kHz");
    }

    #[test]
    fn framer_cuts_whole_frames_and_pads_the_rest() {
        let mut framer = Framer::default();
        framer.push(&[0.25; FRAME + 10]);
        assert_eq!(framer.pop(), Some(vec![0.25; FRAME]));
        assert_eq!(framer.pop(), None);
        let rest = framer.flush().unwrap();
        assert_eq!(rest.len(), FRAME);
        assert_eq!(rest[9], 0.25);
        assert_eq!(rest[10], 0.0);
        assert_eq!(framer.flush(), None);
    }
}
