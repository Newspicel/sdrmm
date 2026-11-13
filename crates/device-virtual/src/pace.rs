use std::time::{Duration, Instant};

use crate::air::BUFFER_S;

/// How far a receiver may fall behind real time before it gives the lost time up as dropped:
/// as deep as a virtual transmitter's buffer, so catching up never outruns a transmitter that
/// keeps its buffer full.
const MAX_LAG: Duration = Duration::from_millis((BUFFER_S * 1000.0) as u64);

/// The sample clock of a virtual receiver. It runs on real time: a worker that wakes late
/// catches up instead of losing time, and one that falls more than [`MAX_LAG`] behind skips
/// ahead and reports the samples it skipped, like a radio whose buffer overflowed.
pub(crate) struct SampleClock {
    due: Instant,
}

impl SampleClock {
    pub(crate) fn start() -> Self {
        Self::starting_at(Instant::now())
    }

    fn starting_at(due: Instant) -> Self {
        Self { due }
    }

    /// When the next block's first sample was due.
    pub(crate) fn due(&self) -> Instant {
        self.due
    }

    /// Samples at `rate` too late to make up; the clock moves past them.
    pub(crate) fn overrun(&mut self, rate: f64) -> u64 {
        self.overrun_at(Instant::now(), rate)
    }

    fn overrun_at(&mut self, now: Instant, rate: f64) -> u64 {
        let late = now.saturating_duration_since(self.due);
        if late <= MAX_LAG {
            return 0;
        }
        let skipped = (late.as_secs_f64() * rate) as u64;
        self.due += Duration::from_secs_f64(skipped as f64 / rate);
        skipped
    }

    /// Steps past a block of `samples` at `rate` and sleeps until the next one is due.
    pub(crate) fn step(&mut self, samples: usize, rate: f64) {
        self.step_for(Duration::from_secs_f64(samples as f64 / rate));
    }

    /// Steps past `span` and sleeps until the next block is due.
    pub(crate) fn step_for(&mut self, span: Duration) {
        self.due += span;
        let wait = self.due.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 1_000.0;

    #[test]
    fn a_late_worker_keeps_every_sample_up_to_the_lag_limit() {
        let start = Instant::now();
        let mut clock = SampleClock::starting_at(start);
        assert_eq!(clock.overrun_at(start + MAX_LAG, RATE), 0);
        assert_eq!(clock.due(), start);
    }

    #[test]
    fn a_worker_further_behind_skips_and_counts_what_it_lost() {
        let start = Instant::now();
        let mut clock = SampleClock::starting_at(start);
        let now = start + MAX_LAG + Duration::from_millis(500);
        let skipped = clock.overrun_at(now, RATE);
        assert_eq!(skipped, ((MAX_LAG.as_secs_f64() + 0.5) * RATE) as u64);
        assert!(now.saturating_duration_since(clock.due()) < Duration::from_millis(1));
    }

    #[test]
    fn a_late_step_does_not_sleep_and_keeps_the_schedule() {
        let start = Instant::now() - Duration::from_millis(200);
        let mut clock = SampleClock::starting_at(start);
        let before = Instant::now();
        clock.step(100, RATE);
        assert!(before.elapsed() < Duration::from_millis(50));
        assert_eq!(clock.due(), start + Duration::from_millis(100));
    }
}
