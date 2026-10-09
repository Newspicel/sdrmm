use std::f64::consts::TAU;

use num_complex::Complex;

use crate::{ChannelOutputs, ChannelRx, synth, testutil::complex_noise};

const CHUNK: usize = 1 << 20;

pub(crate) struct Scene {
    rate: f64,
    noise: f32,
    bursts: Vec<(usize, Vec<Complex<f32>>)>,
    carriers: Vec<(f64, f32)>,
}

impl Scene {
    pub(crate) fn new(rate: f64, noise: f32) -> Self {
        Self {
            rate,
            noise,
            bursts: Vec::new(),
            carriers: Vec::new(),
        }
    }

    pub(crate) fn at(&mut self, start_s: f64, offset_hz: f64, gain: f32, mut burst: Vec<Complex<f32>>) {
        synth::shift(&mut burst, offset_hz, self.rate);
        synth::scale(&mut burst, gain);
        self.bursts.push(((start_s * self.rate) as usize, burst));
    }

    pub(crate) fn every(&mut self, period_s: f64, until_s: f64, offset_hz: f64, gain: f32, burst: &[Complex<f32>]) {
        let mut start = period_s / 2.0;
        while start < until_s {
            self.at(start, offset_hz, gain, burst.to_vec());
            start += period_s;
        }
    }

    pub(crate) fn carrier(&mut self, offset_hz: f64, amplitude: f32) {
        self.carriers.push((offset_hz, amplitude));
    }

    pub(crate) fn run(&self, seconds: f64, channel: &mut dyn ChannelRx) -> ChannelOutputs {
        let total = (seconds * self.rate) as usize;
        let mut out = ChannelOutputs::default();
        let mut events = Vec::new();
        let mut start = 0;
        while start < total {
            let len = CHUNK.min(total - start);
            let mut chunk = complex_noise((start / CHUNK) as u32 * 7_919 + 1, self.noise, len);
            for &(offset, amplitude) in &self.carriers {
                let step = TAU * offset / self.rate;
                for (index, sample) in chunk.iter_mut().enumerate() {
                    *sample += Complex::from_polar(amplitude, (step * (start + index) as f64) as f32);
                }
            }
            for (at, burst) in &self.bursts {
                let from = (*at).max(start);
                let to = (at + burst.len()).min(start + len);
                for index in from..to {
                    chunk[index - start] += burst[index - at];
                }
            }
            out.reset();
            channel.process(&chunk, &mut out);
            events.append(&mut out.events);
            start += len;
        }
        out.events = events;
        out
    }
}
