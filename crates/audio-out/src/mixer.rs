use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::{CHANNELS, Error, Result};

const MAX_VOICES: usize = 32;
const RING_SECONDS: f64 = 1.0;
/// Audio a voice holds before it plays, so the feeding thread's jitter never reaches the card.
const PRIME_SECONDS: f64 = 0.08;
/// Past this much held audio a voice drops back to the prime, so a radio clock running fast
/// against the card never lets the delay grow without bound.
const BEHIND_SECONDS: f64 = 0.3;

#[derive(Debug, Default)]
pub(crate) struct Shared {
    fault: OnceLock<String>,
    underruns: AtomicU64,
}

impl Shared {
    #[cfg_attr(
        not(any(target_os = "linux", target_os = "macos")),
        expect(dead_code, reason = "only a platform that plays can fail")
    )]
    pub(crate) fn fail(&self, reason: String) {
        let _ = self.fault.set(reason);
    }

    pub(crate) fn fault(&self) -> Option<String> {
        self.fault.get().cloned()
    }

    pub(crate) fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
}

/// One input into an output. Takes interleaved stereo at the output's rate.
pub struct Voice {
    producer: Producer<f32>,
    closed: Arc<AtomicBool>,
    overflow: u64,
}

impl Voice {
    /// Queues what fits and returns the frames that did not.
    pub fn push(&mut self, stereo: &[f32]) -> usize {
        let whole = stereo.len() - stereo.len() % CHANNELS;
        let room = self.producer.slots() - self.producer.slots() % CHANNELS;
        let take = whole.min(room);
        if let Ok(mut chunk) = self.producer.write_chunk(take) {
            let (first, second) = chunk.as_mut_slices();
            first.copy_from_slice(&stereo[..first.len()]);
            second.copy_from_slice(&stereo[first.len()..take]);
            chunk.commit_all();
        }
        let dropped = (whole - take) / CHANNELS;
        self.overflow += dropped as u64;
        dropped
    }

    /// Frames lost because the device played slower than the voice was fed.
    #[must_use]
    pub const fn overflow(&self) -> u64 {
        self.overflow
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}

pub(crate) struct Lane {
    consumer: Consumer<f32>,
    closed: Arc<AtomicBool>,
    primed: bool,
}

/// The control side: hands out voices and frees the lanes the mixer let go of.
pub(crate) struct Desk {
    arrivals: Producer<Lane>,
    departures: Consumer<Lane>,
    ring: usize,
}

impl Desk {
    pub(crate) fn voice(&mut self) -> Result<Voice> {
        while self.departures.pop().is_ok() {}
        let (producer, consumer) = RingBuffer::new(self.ring);
        let closed = Arc::new(AtomicBool::new(false));
        let lane = Lane {
            consumer,
            closed: closed.clone(),
            primed: false,
        };
        self.arrivals.push(lane).map_err(|_| Error::Full)?;
        Ok(Voice {
            producer,
            closed,
            overflow: 0,
        })
    }
}

/// The device side: sums every voice into the buffer the device asks for. Never locks and
/// never allocates, so it may run on the device's own thread.
pub(crate) struct Mixer {
    lanes: Vec<Lane>,
    arrivals: Consumer<Lane>,
    departures: Producer<Lane>,
    prime: usize,
    behind: usize,
    shared: Arc<Shared>,
}

pub(crate) fn pair(rate: u32, shared: Arc<Shared>) -> (Desk, Mixer) {
    let frames = |seconds: f64| (f64::from(rate) * seconds).ceil() as usize;
    let (arrivals, incoming) = RingBuffer::new(MAX_VOICES);
    let (outgoing, departures) = RingBuffer::new(MAX_VOICES);
    let desk = Desk {
        arrivals,
        departures,
        ring: frames(RING_SECONDS).max(1) * CHANNELS,
    };
    let mixer = Mixer {
        lanes: Vec::with_capacity(MAX_VOICES),
        arrivals: incoming,
        departures: outgoing,
        prime: frames(PRIME_SECONDS),
        behind: frames(BEHIND_SECONDS),
        shared,
    };
    (desk, mixer)
}

impl Mixer {
    /// Fills interleaved stereo, silence where no voice has audio.
    pub(crate) fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        self.admit();
        let mut index = 0;
        while index < self.lanes.len() {
            if self.mix_lane(index, out) {
                index += 1;
            } else {
                let lane = self.lanes.swap_remove(index);
                let _ = self.departures.push(lane);
            }
        }
    }

    fn admit(&mut self) {
        while self.lanes.len() < self.lanes.capacity() {
            let Ok(lane) = self.arrivals.pop() else {
                return;
            };
            self.lanes.push(lane);
        }
    }

    /// Adds one lane into `out`; false once the lane is closed and played out.
    fn mix_lane(&mut self, index: usize, out: &mut [f32]) -> bool {
        let (prime, behind) = (self.prime, self.behind);
        let lane = &mut self.lanes[index];
        let closed = lane.closed.load(Ordering::Acquire);
        let held = lane.consumer.slots() / CHANNELS;
        if closed && held == 0 {
            return false;
        }
        if !lane.primed {
            if held < prime && !closed {
                return true;
            }
            lane.primed = true;
        }
        if held > behind {
            skip(&mut lane.consumer, (held - prime) * CHANNELS);
        }
        let wanted = out.len() - out.len() % CHANNELS;
        let take = (lane.consumer.slots() - lane.consumer.slots() % CHANNELS).min(wanted);
        if let Ok(chunk) = lane.consumer.read_chunk(take) {
            let (first, second) = chunk.as_slices();
            add(&mut out[..first.len()], first);
            add(&mut out[first.len()..take], second);
            chunk.commit_all();
        }
        if take < wanted && !closed {
            lane.primed = false;
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
        true
    }
}

fn skip(consumer: &mut Consumer<f32>, samples: usize) {
    if let Ok(chunk) = consumer.read_chunk(samples) {
        chunk.commit_all();
    }
}

fn add(into: &mut [f32], from: &[f32]) {
    for (sum, sample) in into.iter_mut().zip(from) {
        *sum += sample;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 1000;

    fn desk() -> (Desk, Mixer, Arc<Shared>) {
        let shared = Arc::new(Shared::default());
        let (desk, mixer) = pair(RATE, shared.clone());
        (desk, mixer, shared)
    }

    fn stereo(frames: usize, left: f32, right: f32) -> Vec<f32> {
        (0..frames).flat_map(|_| [left, right]).collect()
    }

    fn played(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
        let mut out = vec![9.0; frames * CHANNELS];
        mixer.fill(&mut out);
        out
    }

    #[test]
    fn a_voice_stays_silent_until_it_holds_the_prime() {
        let (mut desk, mut mixer, _) = desk();
        let mut voice = desk.voice().unwrap();
        voice.push(&stereo(mixer.prime - 1, 0.5, -0.5));
        assert!(played(&mut mixer, 10).iter().all(|sample| *sample == 0.0));
        voice.push(&stereo(1, 0.5, -0.5));
        assert_eq!(played(&mut mixer, 2), [0.5, -0.5, 0.5, -0.5]);
    }

    #[test]
    fn voices_sum() {
        let (mut desk, mut mixer, _) = desk();
        let mut one = desk.voice().unwrap();
        let mut two = desk.voice().unwrap();
        one.push(&stereo(mixer.prime, 0.25, 0.0));
        two.push(&stereo(mixer.prime, 0.25, 0.5));
        assert_eq!(played(&mut mixer, 1), [0.5, 0.5]);
    }

    #[test]
    fn running_dry_counts_an_underrun_and_primes_again() {
        let (mut desk, mut mixer, shared) = desk();
        let mut voice = desk.voice().unwrap();
        voice.push(&stereo(mixer.prime, 1.0, 1.0));
        let frames = mixer.prime + 2;
        let out = played(&mut mixer, frames);
        assert_eq!(out[out.len() - 1], 0.0);
        assert_eq!(shared.underruns(), 1);
        voice.push(&stereo(1, 1.0, 1.0));
        assert_eq!(played(&mut mixer, 1), [0.0, 0.0]);
    }

    #[test]
    fn a_voice_too_far_behind_drops_back_to_the_prime() {
        let (mut desk, mut mixer, _) = desk();
        let mut voice = desk.voice().unwrap();
        voice.push(&stereo(mixer.behind + 10, 1.0, 1.0));
        played(&mut mixer, 1);
        assert_eq!(
            voice.producer.buffer().capacity() - voice.producer.slots(),
            (mixer.prime - 1) * CHANNELS
        );
    }

    #[test]
    fn a_dropped_voice_plays_out_and_is_let_go() {
        let (mut desk, mut mixer, shared) = desk();
        let mut voice = desk.voice().unwrap();
        voice.push(&stereo(3, 1.0, 1.0));
        drop(voice);
        assert_eq!(
            played(&mut mixer, 4),
            [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0]
        );
        played(&mut mixer, 1);
        assert!(mixer.lanes.is_empty());
        assert_eq!(shared.underruns(), 0);
    }

    #[test]
    fn a_full_voice_reports_what_it_could_not_keep() {
        let (mut desk, _mixer, _) = desk();
        let mut voice = desk.voice().unwrap();
        let ring = (RATE as usize) * CHANNELS;
        assert_eq!(voice.push(&vec![0.0; ring + 6]), 3);
        assert_eq!(voice.overflow(), 3);
    }

    #[test]
    fn a_desk_refuses_past_its_voices() {
        let (mut desk, _mixer, _) = desk();
        let voices: Vec<Voice> = (0..MAX_VOICES).map(|_| desk.voice().unwrap()).collect();
        assert!(matches!(desk.voice(), Err(Error::Full)));
        drop(voices);
    }
}
