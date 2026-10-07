use sdrmm_dsp::{RealInterpolator, design_lowpass};

use super::{
    bfield::{ADPCM_SAMPLES, BField, adpcm_codes},
    burst::{FRAME_SAMPLES, INPUT_RATE_HZ},
    g726::{FULL_SCALE, G726},
};
use crate::{AUDIO_RATE, ChannelOutputs, clamp_full_scale};

const VOICE_RATE: u64 = 8_000;
const UPSAMPLE: usize = (AUDIO_RATE as u64 / VOICE_RATE) as usize;
const BURST_PER_VOICE: u64 = INPUT_RATE_HZ as u64 / VOICE_RATE;
const LATENCY: u64 = 480;
const RING: usize = 2_048;
const INTERPOLATOR_TAPS: usize = 6 * UPSAMPLE * 2 + 1;
const VOICE_CUTOFF_HZ: f64 = 3_600.0;
const RESTART_SAMPLES: u64 = FRAME_SAMPLES * 20;

pub(crate) struct BearerVoice {
    codec: G726,
    last: Option<u64>,
}

impl BearerVoice {
    pub fn new() -> Self {
        Self {
            codec: G726::default(),
            last: None,
        }
    }

    pub fn decode(&mut self, field: &BField, frame: u8, sample: u64) -> [i16; ADPCM_SAMPLES] {
        if self
            .last
            .is_none_or(|last| sample.saturating_sub(last) > RESTART_SAMPLES)
        {
            self.codec.reset();
        }
        self.last = Some(sample);
        let data = field.descrambled(frame);
        let mut pcm = [0i16; ADPCM_SAMPLES];
        for (slot, code) in pcm.iter_mut().zip(adpcm_codes(&data)) {
            *slot = self.codec.decode(code);
        }
        pcm
    }
}

pub(crate) struct Playout {
    ring: Box<[f32; RING]>,
    read: u64,
    consumed: u128,
    input_rate: u64,
    call: Option<usize>,
    last_voice: u64,
    interpolator: RealInterpolator,
    pending: Vec<f32>,
    upsampled: Vec<f32>,
}

impl Playout {
    pub fn new(input_rate: f64) -> Self {
        let taps = design_lowpass(INTERPOLATOR_TAPS, VOICE_CUTOFF_HZ / f64::from(AUDIO_RATE));
        Self {
            ring: Box::new([0.0; RING]),
            read: 0,
            consumed: 0,
            input_rate: input_rate as u64,
            call: None,
            last_voice: 0,
            interpolator: RealInterpolator::new(&taps, UPSAMPLE),
            pending: Vec::with_capacity(RING),
            upsampled: Vec::with_capacity(RING * UPSAMPLE),
        }
    }

    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.call = None;
        self.interpolator.reset();
    }

    pub fn routes(&mut self, call: usize, sample: u64) -> bool {
        let idle = sample.saturating_sub(self.last_voice) > RESTART_SAMPLES;
        if self.call.is_none() || idle {
            self.call = Some(call);
        }
        self.call == Some(call)
    }

    pub fn place(&mut self, at: u64, pcm: &[i16; ADPCM_SAMPLES]) -> bool {
        let start = at / BURST_PER_VOICE + LATENCY;
        if start < self.read || start + ADPCM_SAMPLES as u64 > self.read + RING as u64 {
            return false;
        }
        self.last_voice = at;
        for (offset, &sample) in pcm.iter().enumerate() {
            let slot = (start as usize + offset) % RING;
            self.ring[slot] += f32::from(sample) / FULL_SCALE;
        }
        true
    }

    pub fn advance(&mut self, input_samples: usize, out: &mut ChannelOutputs) {
        self.consumed += input_samples as u128;
        let now = (self.consumed * u128::from(VOICE_RATE) / u128::from(self.input_rate)) as u64;
        self.pending.clear();
        while self.read < now {
            let slot = (self.read % RING as u64) as usize;
            self.pending.push(self.ring[slot]);
            self.ring[slot] = 0.0;
            self.read += 1;
        }
        if self.pending.is_empty() {
            return;
        }
        self.interpolator
            .process(&self.pending, &mut self.upsampled);
        let start = out.audio_pcm.len();
        out.audio_pcm.extend_from_slice(&self.upsampled);
        clamp_full_scale(&mut out.audio_pcm[start..]);
        out.audio_rate = AUDIO_RATE;
    }
}

#[cfg(test)]
mod tests {
    use super::{ADPCM_SAMPLES, LATENCY, Playout};
    use crate::{AUDIO_RATE, ChannelOutputs};

    const fn frame(level: i16) -> [i16; ADPCM_SAMPLES] {
        [level; ADPCM_SAMPLES]
    }

    #[test]
    fn output_keeps_pace_with_the_input_clock() {
        let mut playout = Playout::new(20_736_000.0);
        let mut out = ChannelOutputs::default();
        for _ in 0..100 {
            playout.advance(20_736, &mut out);
        }
        assert_eq!(out.audio_pcm.len(), 2_073_600 * 48 / 20_736);
        assert_eq!(out.audio_rate, AUDIO_RATE);
    }

    #[test]
    fn a_frame_plays_after_the_fixed_latency() {
        let mut playout = Playout::new(2_304_000.0);
        assert!(playout.place(23_040, &frame(4_096)));
        let mut out = ChannelOutputs::default();
        playout.advance(2_304_000 / 10, &mut out);
        let start = ((80 + LATENCY) * 6) as usize;
        let loud = out.audio_pcm[start + 60..start + 400]
            .iter()
            .all(|&s| (s - 0.5).abs() < 0.05);
        assert!(loud, "voice did not land where expected");
        assert!(out.audio_pcm[..start - 60].iter().all(|&s| s.abs() < 1e-3));
    }

    #[test]
    fn a_frame_that_misses_its_slot_is_refused() {
        let mut playout = Playout::new(2_304_000.0);
        let mut out = ChannelOutputs::default();
        playout.advance(2_304_000, &mut out);
        assert!(!playout.place(0, &frame(100)));
    }

    #[test]
    fn the_first_call_keeps_the_output_until_it_goes_quiet() {
        let mut playout = Playout::new(2_304_000.0);
        assert!(playout.routes(3, 0));
        playout.place(0, &frame(1));
        assert!(!playout.routes(5, 23_040));
        assert!(playout.routes(5, 23_040 * 40));
        assert!(playout.routes(5, 23_040 * 41));
    }
}
