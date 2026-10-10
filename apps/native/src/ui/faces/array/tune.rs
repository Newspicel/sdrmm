use std::time::{Duration, Instant};

pub const STALE_REPORT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq)]
pub struct PendingTune {
    pub wanted: f64,
    pub left: Vec<f64>,
    pub settled_at: Option<Instant>,
}

#[must_use]
pub fn shown_center(pending: Option<&PendingTune>, reported: Option<f64>) -> Option<f64> {
    pending.map(|pending| pending.wanted).or(reported)
}

#[must_use]
pub fn retune(pending: Option<PendingTune>, reported: Option<f64>, hz: f64) -> PendingTune {
    let shown = shown_center(pending.as_ref(), reported);
    let mut left = pending.map(|pending| pending.left).unwrap_or_default();
    if let Some(shown) = shown.filter(|shown| *shown != hz) {
        left.push(shown);
    }
    PendingTune {
        wanted: hz,
        left,
        settled_at: None,
    }
}

#[must_use]
pub fn settle(
    pending: Option<PendingTune>,
    reported: Option<f64>,
    tuned: Option<f64>,
    now: Instant,
) -> Option<PendingTune> {
    let (Some(hz), Some(reported)) = (tuned, reported) else {
        return None;
    };
    if reported == hz {
        return None;
    }
    Some(PendingTune {
        wanted: hz,
        left: pending.map_or_else(|| vec![reported], |pending| pending.left),
        settled_at: Some(now),
    })
}

fn reported_stale(pending: &PendingTune, reported: f64, now: Instant) -> bool {
    pending.settled_at.is_none_or(|settled| {
        pending.left.contains(&reported) && now.duration_since(settled) < STALE_REPORT
    })
}

#[must_use]
pub fn confirm(pending: Option<PendingTune>, reported: f64, now: Instant) -> Option<PendingTune> {
    pending.filter(|pending| pending.wanted != reported && reported_stale(pending, reported, now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wanted_centre_shows_until_the_array_reports_it() {
        let asked = retune(None, Some(100e6), 101e6);
        assert_eq!(shown_center(Some(&asked), Some(100e6)), Some(101e6));
        assert_eq!(asked.left, vec![100e6]);
        let now = Instant::now();
        assert_eq!(
            confirm(Some(asked.clone()), 100e6, now),
            Some(asked.clone())
        );
        assert_eq!(confirm(Some(asked), 101e6, now), None);
        assert_eq!(shown_center(None, Some(100e6)), Some(100e6));
    }

    #[test]
    fn quick_turns_remember_every_centre_left_behind() {
        let first = retune(None, Some(100e6), 101e6);
        let second = retune(Some(first), Some(100e6), 102e6);
        assert_eq!(second.wanted, 102e6);
        assert_eq!(second.left, vec![100e6, 101e6]);
        assert_eq!(retune(Some(second.clone()), None, 102e6).left, second.left);
    }

    #[test]
    fn a_settled_tune_drops_once_a_fresh_report_arrives() {
        let now = Instant::now();
        let asked = retune(None, Some(100e6), 101e6);
        let settled = settle(Some(asked), Some(100e6), Some(101e6), now).expect("still pending");
        assert_eq!(settled.settled_at, Some(now));
        assert!(confirm(Some(settled.clone()), 100e6, now).is_some());
        assert!(confirm(Some(settled.clone()), 100e6, now + STALE_REPORT).is_none());
        assert!(confirm(Some(settled), 99e6, now).is_none());
    }

    #[test]
    fn a_failed_or_already_reported_tune_shows_the_report() {
        let now = Instant::now();
        let asked = retune(None, Some(100e6), 101e6);
        assert_eq!(settle(Some(asked.clone()), Some(100e6), None, now), None);
        assert_eq!(
            settle(Some(asked.clone()), Some(101e6), Some(101e6), now),
            None
        );
        assert_eq!(settle(Some(asked), None, Some(101e6), now), None);
        let fresh = settle(None, Some(100e6), Some(101e6), now).expect("pending");
        assert_eq!(fresh.left, vec![100e6]);
    }
}
