pub(crate) const MIN_SNR_DB: f32 = 6.0;
pub(crate) const BASELINE_S: f64 = 1_800.0;
pub(crate) const CONFIRM_S: f64 = 30.0;
pub(crate) const MAX_EVENT_S: f64 = 7_200.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reading {
    pub baseline_db: Option<f32>,
    pub disturbed: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Tracker {
    baseline: Option<f64>,
    reports: u32,
    beyond: u32,
    disturbed_for: u32,
}

impl Tracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn update(
        &mut self,
        level_db: f32,
        snr_db: f32,
        threshold_db: f32,
        report_s: f64,
    ) -> Reading {
        let reading = Reading {
            baseline_db: self.baseline.map(|baseline| baseline as f32),
            disturbed: false,
        };
        if snr_db < MIN_SNR_DB {
            self.beyond = 0;
            self.disturbed_for = 0;
            return reading;
        }
        let level = f64::from(level_db);
        let Some(baseline) = self.baseline else {
            self.baseline = Some(level);
            self.reports = 1;
            return reading;
        };
        let deviation = (level - baseline).abs();
        let threshold = f64::from(threshold_db);
        if self.disturbed_for > 0 {
            self.follow_event(deviation < threshold / 2.0, level, report_s);
        } else if deviation >= threshold {
            self.beyond += 1;
            if f64::from(self.beyond) * report_s >= CONFIRM_S.max(2.0 * report_s) {
                self.disturbed_for = 1;
            }
        } else {
            self.beyond = 0;
            self.learn(level, report_s);
        }
        Reading {
            disturbed: self.disturbed_for > 0,
            ..reading
        }
    }

    fn follow_event(&mut self, calmed: bool, level: f64, report_s: f64) {
        if calmed {
            self.disturbed_for = 0;
            self.beyond = 0;
        } else if f64::from(self.disturbed_for) * report_s >= MAX_EVENT_S {
            self.disturbed_for = 0;
            self.beyond = 0;
            self.baseline = Some(level);
            self.reports = 1;
        } else {
            self.disturbed_for += 1;
        }
    }

    fn learn(&mut self, level: f64, report_s: f64) {
        let Some(baseline) = self.baseline.as_mut() else {
            return;
        };
        self.reports += 1;
        let weight = (1.0 / f64::from(self.reports)).max(report_s / BASELINE_S);
        *baseline += weight * (level - *baseline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT_S: f64 = 5.0;

    fn run(tracker: &mut Tracker, level: f32, seconds: f64) -> Reading {
        let mut last = None;
        for _ in 0..(seconds / REPORT_S) as usize {
            last = Some(tracker.update(level, 30.0, 3.0, REPORT_S));
        }
        last.expect("at least one report")
    }

    #[test]
    fn a_steady_station_is_quiet_and_learns_its_baseline() {
        let mut tracker = Tracker::default();
        let reading = run(&mut tracker, -50.0, 600.0);
        assert!(!reading.disturbed);
        assert_eq!(reading.baseline_db, Some(-50.0));
    }

    #[test]
    fn a_sudden_rise_is_flagged_after_confirmation_and_clears_on_decay() {
        let mut tracker = Tracker::default();
        run(&mut tracker, -50.0, 600.0);
        let first = tracker.update(-44.0, 30.0, 3.0, REPORT_S);
        assert!(!first.disturbed);
        let held = run(&mut tracker, -44.0, CONFIRM_S);
        assert!(held.disturbed);
        assert_eq!(held.baseline_db, Some(-50.0));
        let decayed = run(&mut tracker, -49.0, REPORT_S);
        assert!(!decayed.disturbed);
    }

    #[test]
    fn a_dip_counts_as_a_disturbance_too() {
        let mut tracker = Tracker::default();
        run(&mut tracker, -50.0, 600.0);
        assert!(run(&mut tracker, -56.0, 60.0).disturbed);
    }

    #[test]
    fn a_weak_station_is_never_disturbed() {
        let mut tracker = Tracker::default();
        for level in [-50.0, -40.0, -40.0, -40.0, -40.0, -40.0, -40.0, -40.0] {
            let reading = tracker.update(level, 3.0, 3.0, REPORT_S);
            assert!(!reading.disturbed);
            assert_eq!(reading.baseline_db, None);
        }
    }

    #[test]
    fn an_endless_event_rebaselines() {
        let mut tracker = Tracker::default();
        run(&mut tracker, -50.0, 600.0);
        assert!(run(&mut tracker, -40.0, MAX_EVENT_S).disturbed);
        let after = run(&mut tracker, -40.0, 2.0 * CONFIRM_S);
        assert!(!after.disturbed);
        assert_eq!(after.baseline_db, Some(-40.0));
    }
}
