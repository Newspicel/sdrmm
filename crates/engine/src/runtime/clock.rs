use std::time::{Duration, SystemTime};

use crate::capture_ring::Arrival;

/// How long one bucket of arrivals is kept before it is rolled over.
const BUCKET: Duration = Duration::from_secs(10);
/// Arrivals further from the estimate than this mean the stream restarted.
const JUMP: Duration = Duration::from_secs(1);

/// The wall-clock time each device sample was captured at.
///
/// A span arrives some transfer latency after its last sample was captured, so `arrival - index
/// / rate` overestimates the time of sample zero by that latency. The smallest value over recent
/// arrivals is the best estimate. It is taken over two rolling buckets so a radio whose crystal
/// drifts against the system clock is followed within a bucket or two.
pub(crate) struct SampleClock {
    rate: f64,
    current: Option<Bucket>,
    previous: Option<f64>,
}

#[derive(Clone, Copy)]
struct Bucket {
    opened: SystemTime,
    origin: f64,
}

fn seconds(time: SystemTime) -> Option<f64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|since| since.as_secs_f64())
}

impl SampleClock {
    pub(crate) fn new() -> Self {
        Self {
            rate: 0.0,
            current: None,
            previous: None,
        }
    }

    pub(crate) fn observe(&mut self, arrival: Arrival, rate: f64) {
        let Some(arrived) = seconds(arrival.at) else {
            return;
        };
        if rate <= 0.0 {
            return;
        }
        let origin = arrived - arrival.end as f64 / rate;
        let restarted = rate != self.rate
            || self
                .origin()
                .is_some_and(|known| (origin - known).abs() > JUMP.as_secs_f64());
        if restarted {
            self.rate = rate;
            self.previous = None;
            self.current = Some(Bucket {
                opened: arrival.at,
                origin,
            });
            return;
        }
        let Some(bucket) = &mut self.current else {
            self.current = Some(Bucket {
                opened: arrival.at,
                origin,
            });
            return;
        };
        if arrival
            .at
            .duration_since(bucket.opened)
            .is_ok_and(|age| age >= BUCKET)
        {
            self.previous = Some(bucket.origin);
            *bucket = Bucket {
                opened: arrival.at,
                origin,
            };
        } else {
            bucket.origin = bucket.origin.min(origin);
        }
    }

    fn origin(&self) -> Option<f64> {
        let current = self.current?.origin;
        Some(
            self.previous
                .map_or(current, |previous| previous.min(current)),
        )
    }

    /// When the sample at `index` was captured.
    pub(crate) fn at(&self, index: u64) -> Option<SystemTime> {
        let seconds = self.origin()? + index as f64 / self.rate;
        SystemTime::UNIX_EPOCH.checked_add(Duration::try_from_secs_f64(seconds).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 1_000_000.0;

    fn at(seconds: f64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs_f64(1_700_000_000.0 + seconds)
    }

    fn arrival(end_s: f64, latency_s: f64) -> Arrival {
        Arrival {
            end: (end_s * RATE) as u64,
            at: at(end_s + latency_s),
        }
    }

    fn error_s(clock: &SampleClock, index_s: f64) -> f64 {
        let estimate = clock.at((index_s * RATE) as u64).unwrap();
        match estimate.duration_since(at(index_s)) {
            Ok(late) => late.as_secs_f64(),
            Err(early) => -early.duration().as_secs_f64(),
        }
    }

    #[test]
    fn the_quickest_arrival_sets_the_time() {
        let mut clock = SampleClock::new();
        for (index, latency) in [0.080, 0.012, 0.050, 0.140, 0.030].into_iter().enumerate() {
            clock.observe(arrival(index as f64 * 0.1 + 0.1, latency), RATE);
        }
        assert!((error_s(&clock, 0.25) - 0.012).abs() < 1e-4);
    }

    #[test]
    fn a_restarted_stream_starts_a_new_estimate() {
        let mut clock = SampleClock::new();
        clock.observe(arrival(1.0, 0.010), RATE);
        clock.observe(arrival(1.1, 5.0), RATE);
        assert!((error_s(&clock, 1.0) - 5.0).abs() < 1e-4);
        clock.observe(arrival(1.2, 0.02), 2.0 * RATE);
        assert!(clock.at(0).is_some());
    }

    #[test]
    fn a_drifting_crystal_is_followed() {
        let mut clock = SampleClock::new();
        let slow = 1.0 + 20e-6;
        for step in 0..600 {
            let true_s = step as f64 * 0.1;
            clock.observe(
                Arrival {
                    end: (true_s * RATE / slow) as u64,
                    at: at(true_s + 0.01),
                },
                RATE,
            );
        }
        let index = (59.0 * RATE / slow) as u64;
        let estimate = clock.at(index).unwrap().duration_since(at(59.0)).unwrap();
        assert!(estimate.as_secs_f64() < 0.02, "{estimate:?}");
    }
}
