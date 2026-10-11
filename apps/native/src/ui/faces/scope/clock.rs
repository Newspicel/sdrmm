use jiff::{Timestamp, tz::TimeZone};

use super::history::{HISTORY_ROWS, next_ring_row, seed_placement};

const STEPS_S: [f64; 12] = [
    1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0,
];

#[derive(Clone, Debug, PartialEq)]
pub struct TimeTick {
    pub y: f64,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowClock {
    times: Vec<f64>,
    write: usize,
    bins: usize,
}

impl Default for RowClock {
    fn default() -> Self {
        Self {
            times: vec![f64::NAN; HISTORY_ROWS],
            write: 0,
            bins: 0,
        }
    }
}

impl RowClock {
    pub fn push(&mut self, at_ms: f64, bins: usize) {
        if bins != self.bins {
            self.reset(bins);
        }
        self.times[self.write] = at_ms;
        self.write = next_ring_row(self.write, HISTORY_ROWS);
    }

    pub fn seed(&mut self, times: &[f64], count: usize, bins: usize) {
        let place = seed_placement(count, HISTORY_ROWS);
        if place.rows == 0 || bins == 0 {
            return;
        }
        self.reset(bins);
        for (row, slot) in self.times.iter_mut().take(place.rows).enumerate() {
            *slot = times.get(place.skip + row).copied().unwrap_or(f64::NAN);
        }
        self.write = place.write;
    }

    fn reset(&mut self, bins: usize) {
        self.times.fill(f64::NAN);
        self.write = 0;
        self.bins = bins;
    }

    #[must_use]
    pub fn at(&self, back: usize) -> f64 {
        let newest = self.write + HISTORY_ROWS - 1;
        self.times[(newest - back % HISTORY_ROWS) % HISTORY_ROWS]
    }
}

#[must_use]
pub fn time_ticks(
    time_at: impl Fn(usize) -> f64,
    rows: usize,
    height: f64,
    min_gap: f64,
) -> Vec<TimeTick> {
    let Some(deepest) = deepest_known(&time_at, rows) else {
        return Vec::new();
    };
    let span_ms = time_at(0) - time_at(deepest);
    if !(span_ms > 0.0) {
        return Vec::new();
    }
    let row_px = height / rows.saturating_sub(1).max(1) as f64;
    let px_per_second = deepest as f64 * row_px * 1000.0 / span_ms;
    let Some(step) = STEPS_S
        .into_iter()
        .find(|seconds| seconds * px_per_second >= min_gap)
    else {
        return Vec::new();
    };
    let step_ms = step * 1000.0;
    (0..deepest)
        .filter_map(|back| {
            let newer = (time_at(back) / step_ms).floor();
            let older = (time_at(back + 1) / step_ms).floor();
            (newer.is_finite() && older.is_finite() && newer != older).then(|| TimeTick {
                y: back as f64 * row_px,
                label: format_clock(newer * step_ms, step < 60.0),
            })
        })
        .collect()
}

fn deepest_known(time_at: &impl Fn(usize) -> f64, rows: usize) -> Option<usize> {
    if !time_at(0).is_finite() {
        return None;
    }
    (1..rows).rev().find(|back| time_at(*back).is_finite())
}

#[must_use]
pub fn format_clock(at_ms: f64, seconds: bool) -> String {
    format_clock_in(at_ms, seconds, &TimeZone::system())
}

#[must_use]
pub fn format_clock_in(at_ms: f64, seconds: bool, zone: &TimeZone) -> String {
    let Ok(at) = Timestamp::from_millisecond(at_ms as i64) else {
        return String::new();
    };
    let local = at.to_zoned(zone.clone());
    if seconds {
        format!(
            "{:02}:{:02}:{:02}",
            local.hour(),
            local.minute(),
            local.second()
        )
    } else {
        format!("{:02}:{:02}", local.hour(), local.minute())
    }
}

#[must_use]
pub fn wall_ms() -> f64 {
    Timestamp::now().as_millisecond() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_700_000_000_000.0;

    fn steady(back: usize) -> f64 {
        NOW - back as f64 * 100.0
    }

    #[test]
    fn a_tick_marks_each_row_where_the_clock_passes_a_step() {
        let ticks = time_ticks(steady, 101, 100.0, 20.0);
        let ys: Vec<f64> = ticks.iter().map(|tick| tick.y).collect();
        assert_eq!(ys, [0.0, 20.0, 40.0, 60.0, 80.0]);
        assert_eq!(ticks[1].label, format_clock(NOW - 2_000.0, true));
    }

    #[test]
    fn a_tick_rides_down_with_its_row() {
        let before = time_ticks(steady, 101, 100.0, 20.0);
        let after = time_ticks(|back| steady(back) + 700.0, 101, 100.0, 20.0);
        assert_eq!(after[1].y, before[1].y + 7.0);
        assert_eq!(after[1].label, before[1].label);
    }

    #[test]
    fn minute_steps_drop_the_seconds() {
        let ticks = time_ticks(|back| NOW - back as f64 * 10_000.0, 61, 600.0, 40.0);
        assert_eq!(ticks[0].label, format_clock(NOW - 20_000.0, false));
    }

    #[test]
    fn rows_without_times_draw_nothing() {
        assert!(time_ticks(|_| f64::NAN, 100, 100.0, 20.0).is_empty());
        let only_newest = |back: usize| if back == 0 { NOW } else { f64::NAN };
        assert!(time_ticks(only_newest, 100, 100.0, 20.0).is_empty());
    }

    #[test]
    fn the_clock_reads_wall_time_in_the_given_zone() {
        let utc = TimeZone::UTC;
        assert_eq!(format_clock_in(NOW, true, &utc), "22:13:20");
        assert_eq!(format_clock_in(NOW, false, &utc), "22:13");
    }

    #[test]
    fn the_clock_follows_the_ring_and_resets_on_a_new_resolution() {
        let mut clock = RowClock::default();
        clock.push(1.0, 4);
        clock.push(2.0, 4);
        assert_eq!((clock.at(0), clock.at(1)), (2.0, 1.0));
        assert!(clock.at(2).is_nan());
        clock.push(3.0, 8);
        assert_eq!(clock.at(0), 3.0);
        assert!(clock.at(1).is_nan());
    }

    #[test]
    fn a_seed_keeps_the_newest_times_on_top() {
        let mut clock = RowClock::default();
        clock.seed(&[10.0, 20.0, 30.0], 3, 4);
        assert_eq!((clock.at(0), clock.at(2)), (30.0, 10.0));
        clock.seed(&[], 0, 4);
        assert_eq!(clock.at(0), 30.0);
    }
}
