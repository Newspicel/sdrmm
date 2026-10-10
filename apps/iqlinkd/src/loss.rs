use std::time::{Duration, Instant};

const CAUGHT_UP: f64 = 0.25;
const CLOCK_TOLERANCE: f64 = 100e-6;

#[derive(Clone, Copy, Debug)]
pub struct Chunk {
    pub frames: usize,
    pub done: Instant,
    pub waited: Duration,
    pub rate: f64,
}

impl Chunk {
    fn caught_up(&self) -> bool {
        self.waited.as_secs_f64() * self.rate >= self.frames as f64 * CAUGHT_UP
    }
}

#[derive(Debug)]
pub struct Loss {
    queued_chunks: f64,
    rate: f64,
    origin: Option<Instant>,
    received: f64,
    lost: u64,
}

impl Loss {
    pub const fn new(queued_chunks: usize) -> Self {
        Self {
            queued_chunks: queued_chunks as f64,
            rate: 0.0,
            origin: None,
            received: 0.0,
            lost: 0,
        }
    }

    pub const fn lost(&self) -> u64 {
        self.lost
    }

    pub fn book(&mut self, chunk: &Chunk) -> u64 {
        if chunk.rate.to_bits() != self.rate.to_bits() {
            self.rate = chunk.rate;
            self.origin = None;
        }
        if self.rate <= 0.0 {
            return 0;
        }
        let Some(origin) = self.origin else {
            self.origin = Some(chunk.done);
            self.received = 0.0;
            return 0;
        };
        self.received += chunk.frames as f64;
        let expected = chunk.done.duration_since(origin).as_secs_f64() * self.rate;
        let missing = expected - self.received - self.lost as f64;
        let unexplained = missing - self.queued_at_most(chunk) - expected * CLOCK_TOLERANCE;
        if unexplained < 1.0 {
            return 0;
        }
        let booked = unexplained.round() as u64;
        self.lost += booked;
        booked
    }

    fn queued_at_most(&self, chunk: &Chunk) -> f64 {
        let chunks = if chunk.caught_up() {
            1.0
        } else {
            self.queued_chunks + 1.0
        };
        chunks * chunk.frames as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 1_000_000.0;
    const FRAMES: usize = 10_000;
    const QUEUED: usize = 4;

    fn chunk(start: Instant, ms: u64, waited_ms: u64) -> Chunk {
        Chunk {
            frames: FRAMES,
            done: start + Duration::from_millis(ms),
            waited: Duration::from_millis(waited_ms),
            rate: RATE,
        }
    }

    #[test]
    fn a_reader_keeping_pace_loses_nothing() {
        let start = Instant::now();
        let mut loss = Loss::new(QUEUED);
        for step in 0..1_000 {
            assert_eq!(loss.book(&chunk(start, step * 10, 9)), 0);
        }
        assert_eq!(loss.lost(), 0);
    }

    #[test]
    fn a_late_chunk_within_one_chunk_is_jitter() {
        let start = Instant::now();
        let mut loss = Loss::new(QUEUED);
        loss.book(&chunk(start, 0, 9));
        assert_eq!(loss.book(&chunk(start, 19, 9)), 0);
    }

    #[test]
    fn a_gap_is_booked_once_the_reader_catches_up() {
        let start = Instant::now();
        let mut loss = Loss::new(QUEUED);
        for step in 0..=10 {
            loss.book(&chunk(start, step * 10, 9));
        }
        assert_eq!(loss.book(&chunk(start, 150, 0)), 0, "the queue explains it");
        let booked = loss.book(&chunk(start, 160, 9));
        assert!((29_900..=30_000).contains(&booked), "{booked}");
        assert_eq!(loss.book(&chunk(start, 170, 9)), 0);
    }

    #[test]
    fn a_reader_that_never_catches_up_books_what_the_queue_cannot_hold() {
        let start = Instant::now();
        let mut loss = Loss::new(QUEUED);
        loss.book(&chunk(start, 0, 0));
        let mut booked = 0;
        for step in 1..=100 {
            booked += loss.book(&chunk(start, step * 20, 0));
        }
        let truth = 2_000_000 - 100 * FRAMES as u64;
        let bound = (QUEUED as u64 + 2) * FRAMES as u64;
        assert!(booked > 0, "silent loss");
        assert!(
            booked <= truth && truth - booked <= bound,
            "{booked} vs {truth}"
        );
    }

    #[test]
    fn a_new_rate_starts_over() {
        let start = Instant::now();
        let mut loss = Loss::new(QUEUED);
        loss.book(&chunk(start, 0, 9));
        let mut faster = chunk(start, 500, 9);
        faster.rate = RATE * 2.0;
        assert_eq!(loss.book(&faster), 0);
        let mut next = chunk(start, 505, 9);
        next.rate = RATE * 2.0;
        assert_eq!(loss.book(&next), 0);
    }

    #[test]
    fn an_unknown_rate_is_not_judged() {
        let start = Instant::now();
        let mut loss = Loss::new(QUEUED);
        let mut silent = chunk(start, 0, 0);
        silent.rate = 0.0;
        loss.book(&silent);
        silent.done = start + Duration::from_secs(10);
        assert_eq!(loss.book(&silent), 0);
    }
}
