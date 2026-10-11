use std::{collections::VecDeque, time::Instant};

pub const MAX_PENDING: usize = 32;
pub const MAX_PENDING_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_DELAY_MS: f64 = 500.0;

pub struct Pacer<T> {
    pending: VecDeque<(Instant, usize, T)>,
    bytes: usize,
}

impl<T> Default for Pacer<T> {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            bytes: 0,
        }
    }
}

#[must_use]
pub fn clamp_delay_ms(delay_ms: f64) -> f64 {
    if delay_ms.is_finite() {
        delay_ms.clamp(0.0, MAX_DELAY_MS)
    } else {
        0.0
    }
}

impl<T> Pacer<T> {
    pub fn push(&mut self, due: Instant, bytes: usize, item: T) -> usize {
        self.pending.push_back((due, bytes, item));
        self.bytes += bytes;
        let mut dropped = 0;
        while self.pending.len() > MAX_PENDING || self.bytes > MAX_PENDING_BYTES {
            if let Some((_, size, _)) = self.pending.pop_front() {
                self.bytes -= size;
                dropped += 1;
            }
        }
        dropped
    }

    pub fn take_due(&mut self, now: Instant) -> Option<T> {
        let mut latest = None;
        while self.pending.front().is_some_and(|(due, _, _)| *due <= now) {
            if let Some((_, size, item)) = self.pending.pop_front() {
                self.bytes -= size;
                latest = Some(item);
            }
        }
        latest
    }

    #[must_use]
    pub fn next_due(&self) -> Option<Instant> {
        self.pending.front().map(|(due, _, _)| *due)
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_frame_waits_until_it_is_due_and_the_newest_due_one_wins() {
        let start = Instant::now();
        let mut pacer = Pacer::default();
        pacer.push(start, 1, 'a');
        pacer.push(start + Duration::from_millis(10), 1, 'b');
        pacer.push(start + Duration::from_millis(50), 1, 'c');
        assert_eq!(pacer.take_due(start + Duration::from_millis(20)), Some('b'));
        assert_eq!(pacer.next_due(), Some(start + Duration::from_millis(50)));
        assert_eq!(pacer.take_due(start + Duration::from_millis(20)), None);
        pacer.clear();
        assert_eq!(pacer.next_due(), None);
    }

    #[test]
    fn the_queue_drops_its_oldest_frames_past_its_limits() {
        let start = Instant::now();
        let mut pacer = Pacer::default();
        let dropped: usize = (0..MAX_PENDING + 3)
            .map(|at| pacer.push(start, 1, at))
            .sum();
        assert_eq!(dropped, 3);
        assert_eq!(pacer.take_due(start), Some(MAX_PENDING + 2));
        assert_eq!(pacer.push(start, MAX_PENDING_BYTES + 1, 0), 1);
    }

    #[test]
    fn the_delay_follows_audio_but_never_runs_away() {
        assert_eq!(clamp_delay_ms(120.0), 120.0);
        assert_eq!(clamp_delay_ms(-5.0), 0.0);
        assert_eq!(clamp_delay_ms(9_000.0), MAX_DELAY_MS);
        assert_eq!(clamp_delay_ms(f64::NAN), 0.0);
    }
}
