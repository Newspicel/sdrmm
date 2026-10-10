use crate::runtime::SpectrumSnapshot;

const FLOOR_QUANTILE: f64 = 0.25;

#[derive(Default)]
pub(crate) struct NoiseFloor {
    scratch: Vec<f32>,
}

impl NoiseFloor {
    pub(crate) fn of(&mut self, snapshot: &SpectrumSnapshot) -> Option<f32> {
        let n = snapshot.db.len();
        if n == 0 || !snapshot.span_hz.is_finite() || snapshot.span_hz <= 0.0 {
            return None;
        }
        let guard = snapshot.lo_guard();
        self.scratch.clear();
        self.scratch.extend(
            (0..n)
                .filter(|i| !guard.as_ref().is_some_and(|g| g.contains(i)))
                .map(|i| snapshot.db[i]),
        );
        if self.scratch.is_empty() {
            return None;
        }
        let at = ((self.scratch.len() - 1) as f64 * FLOOR_QUANTILE) as usize;
        let (_, floor, _) = self.scratch.select_nth_unstable_by(at, f32::total_cmp);
        floor.is_finite().then_some(*floor)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn snapshot(db: Vec<f32>) -> SpectrumSnapshot {
        SpectrumSnapshot {
            seq: 1,
            timestamp: 0,
            center_hz: 200e6,
            span_hz: 2_048_000.0,
            db: Arc::from(db.as_slice()),
        }
    }

    #[test]
    fn a_wide_signal_over_most_of_the_span_does_not_become_the_floor() {
        let mut db = vec![-95.0f32; 1024];
        db[200..900].fill(-60.0);
        assert_eq!(NoiseFloor::default().of(&snapshot(db)), Some(-95.0));
    }

    #[test]
    fn an_empty_span_has_no_floor() {
        assert_eq!(NoiseFloor::default().of(&snapshot(Vec::new())), None);
    }
}
