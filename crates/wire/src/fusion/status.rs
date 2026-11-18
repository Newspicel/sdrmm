use crate::channel::ChannelStatus;
use crate::fusion::{DfEstimate, DfFusionState, NavTarget, NavTargetKind};
use crate::present::{Unit, count, fields, number, position, text_of};

fn distance(metres: f64) -> String {
    if metres >= 1_000.0 {
        format!("{:.1} km", metres / 1_000.0)
    } else {
        format!("{metres:.0} m")
    }
}

fn spread(estimate: &DfEstimate) -> String {
    format!(
        "{} × {}",
        distance(estimate.ellipse_major_m),
        distance(estimate.ellipse_minor_m)
    )
}

fn guidance(nav: &NavTarget) -> String {
    let action = match nav.kind {
        NavTargetKind::Probe => "Drive across",
        NavTargetKind::Estimate => "Drive at it",
    };
    format!(
        "{action} · {:.0}° · {}",
        nav.bearing_deg.rem_euclid(360.0),
        distance(nav.distance_m)
    )
}

impl DfFusionState {
    /// Readouts for a triangulation face; `fade` is the half life Auto settled on.
    #[must_use]
    pub fn status(&self) -> ChannelStatus {
        let estimate = self.estimate.as_ref();
        ChannelStatus::readouts(fields(&[
            (
                "estimate",
                "Estimate",
                estimate.and_then(|fix| position(Some(fix.lat), Some(fix.lon))),
            ),
            ("spread", "Spread", estimate.map(spread).and_then(text_of)),
            (
                "guidance",
                "Guidance",
                self.nav.as_ref().map(guidance).and_then(text_of),
            ),
            ("bearings", "Bearings", count(Some(self.samples))),
            (
                "fade",
                "Half life",
                number(Some(self.half_life_s).filter(|s| *s > 0), Unit::Seconds, 0),
            ),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use crate::fusion::DfFusionState;

    #[test]
    fn a_fusion_reports_the_half_life_auto_found() {
        let state = DfFusionState {
            samples: 4,
            half_life_s: 120,
            ..DfFusionState::default()
        };
        let status = state.status();
        let keys: Vec<&str> = status
            .readouts
            .iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(keys, ["bearings", "fade"]);
    }
}
