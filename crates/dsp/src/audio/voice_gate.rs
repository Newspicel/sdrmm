//! Voice-activated gate: a level threshold and earshot speech detection, held open after speech.

use earshot::Detector;

use crate::measure::level::{LEVEL_FLOOR_DB, power_db};

pub const VOICE_GATE_RATE_HZ: u32 = 48_000;

const DETECTOR_FRAME: usize = 256;
const DECIMATION: usize = 3;
const SPEECH_SCORE: f32 = 0.5;
const SPEECH_FRAMES: usize = 3;

/// earshot scores its first frames high on any input while its state settles.
const WARMUP_FRAMES: usize = 8;

/// Opens on loud, speech-like blocks and closes `hang_ms` after the last one.
pub struct VoiceGate {
    detector: Box<Detector>,
    pending: Vec<f32>,
    recent: [f32; SPEECH_FRAMES],
    next_slot: usize,
    frames_seen: usize,
    triple_sum: f32,
    triple_len: usize,
    threshold_db: f32,
    hang_samples: u64,
    hold_left: u64,
    level_db: f32,
}

impl VoiceGate {
    pub fn new(threshold_db: f32, hang_ms: u32) -> Self {
        let mut gate = Self {
            detector: Detector::default_boxed(),
            pending: Vec::with_capacity(2 * DETECTOR_FRAME),
            recent: [0.0; SPEECH_FRAMES],
            next_slot: 0,
            frames_seen: 0,
            triple_sum: 0.0,
            triple_len: 0,
            threshold_db,
            hang_samples: 0,
            hold_left: 0,
            level_db: LEVEL_FLOOR_DB,
        };
        gate.set(threshold_db, hang_ms);
        gate
    }

    pub fn set(&mut self, threshold_db: f32, hang_ms: u32) {
        self.threshold_db = threshold_db;
        self.hang_samples = u64::from(hang_ms) * u64::from(VOICE_GATE_RATE_HZ) / 1000;
    }

    /// Peak level of the last block in dB full scale.
    pub fn level_db(&self) -> f32 {
        self.level_db
    }

    /// Feeds a 48 kHz block and returns whether the gate is open after it.
    pub fn feed(&mut self, block: &[f32]) -> bool {
        let peak = block.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        self.level_db = power_db(peak * peak, LEVEL_FLOOR_DB);
        self.score(block);
        let speaking = self.level_db >= self.threshold_db && self.speech_recent();
        if speaking {
            self.hold_left = self.hang_samples;
        } else {
            self.hold_left = self.hold_left.saturating_sub(block.len() as u64);
        }
        speaking || self.hold_left > 0
    }

    fn score(&mut self, block: &[f32]) {
        for &sample in block {
            self.triple_sum += sample;
            self.triple_len += 1;
            if self.triple_len == DECIMATION {
                self.pending.push(self.triple_sum / DECIMATION as f32);
                self.triple_sum = 0.0;
                self.triple_len = 0;
            }
        }
        let mut used = 0;
        while self.pending.len() - used >= DETECTOR_FRAME {
            let score = self
                .detector
                .predict_f32(&self.pending[used..used + DETECTOR_FRAME]);
            self.recent[self.next_slot] = score;
            self.next_slot = (self.next_slot + 1) % SPEECH_FRAMES;
            self.frames_seen = self.frames_seen.saturating_add(1);
            used += DETECTOR_FRAME;
        }
        self.pending.drain(..used);
    }

    fn speech_recent(&self) -> bool {
        self.frames_seen >= WARMUP_FRAMES && self.recent.iter().any(|&score| score >= SPEECH_SCORE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = 960;

    fn hiss(blocks: usize, amplitude: f32) -> Vec<f32> {
        let mut seed = 0x1234_5678u32;
        (0..blocks * BLOCK)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed as f32 / u32::MAX as f32 - 0.5) * 2.0 * amplitude
            })
            .collect()
    }

    fn voice(blocks: usize) -> Vec<f32> {
        let rate = VOICE_GATE_RATE_HZ as f32;
        let (mut a, mut av, mut b, mut bv, mut phase) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let (w1, w2) = (
            std::f32::consts::TAU * 730.0 / rate,
            std::f32::consts::TAU * 1090.0 / rate,
        );
        let raw: Vec<f32> = (0..blocks * BLOCK)
            .map(|i| {
                let last = phase;
                phase = (phase + (120.0 + 30.0 * (i as f32 / rate * 1.3).sin()) / rate).fract();
                let pulse = f32::from(phase < last);
                av = av * 0.993 - w1 * a + pulse;
                a += w1 * av;
                bv = bv * 0.993 - w2 * b + pulse;
                b += w2 * bv;
                a + 0.5 * b
            })
            .collect();
        let peak = raw.iter().fold(0.0f32, |p, s| p.max(s.abs()));
        raw.iter().map(|s| s * 0.5 / peak).collect()
    }

    fn open_after(gate: &mut VoiceGate, signal: &[f32]) -> Vec<bool> {
        signal
            .as_chunks::<BLOCK>()
            .0
            .iter()
            .map(|b| gate.feed(b))
            .collect()
    }

    #[test]
    fn gate_stays_shut_below_threshold() {
        let mut gate = VoiceGate::new(-35.0, 600);
        let quiet: Vec<f32> = voice(100).iter().map(|s| s * 0.001).collect();
        assert!(open_after(&mut gate, &quiet).iter().all(|open| !open));
    }

    #[test]
    fn gate_holds_for_hang_then_shuts() {
        let mut gate = VoiceGate::new(-35.0, 600);
        let opened = open_after(&mut gate, &voice(100));
        assert!(
            opened.iter().any(|&open| open),
            "voice never opened the gate"
        );
        let silence = vec![0.0f32; 50 * BLOCK];
        let after = open_after(&mut gate, &silence);
        assert!(after[0], "gate shut at once");
        assert!(after[20], "gate shut before 600 ms");
        assert!(!after[49], "gate never shut");
    }

    #[test]
    fn steady_hiss_does_not_open_the_gate() {
        let mut gate = VoiceGate::new(-35.0, 600);
        assert!(
            open_after(&mut gate, &hiss(200, 0.3))
                .iter()
                .all(|open| !open)
        );
    }

    #[test]
    fn odd_block_lengths_keep_every_sample() {
        let signal = voice(100);
        let mut whole = VoiceGate::new(-35.0, 600);
        let mut odd = VoiceGate::new(-35.0, 600);
        for block in signal.chunks(BLOCK) {
            whole.feed(block);
        }
        for block in signal.chunks(BLOCK - 1) {
            odd.feed(block);
        }
        assert_eq!(whole.recent, odd.recent);
    }

    #[test]
    fn level_follows_the_block_peak() {
        let mut gate = VoiceGate::new(-35.0, 600);
        gate.feed(&[0.1; BLOCK]);
        assert!((gate.level_db() + 20.0).abs() < 0.5);
    }
}
