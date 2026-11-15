use super::{Rotation, SweepEstimator};
use crate::math::special::wrap_deg;

pub(super) const START_LAG_S: f64 = 0.1;
const MAX_LAG_S: f64 = 0.5;
const LAG_GAIN: f64 = 0.5;
const PAIR_WINDOW_S: f64 = 30.0;
const PAIR_WINDOW_DEG: f64 = 30.0;

#[derive(Clone, Copy)]
pub(super) struct PastSweep {
    bearing_deg: f64,
    rate_dps: f64,
    lag_s: f64,
    at_s: f64,
}

impl SweepEstimator {
    /// Learns the level lag from two sweeps in opposite directions over the same bearing: a lag
    /// pushes each one's bearing the way it turns, so their split is the lag times the rates.
    pub(super) fn learn_lag(
        &mut self,
        bearing_deg: f64,
        rate_dps: f64,
        rotation: Rotation,
        at_s: f64,
    ) {
        let this = PastSweep {
            bearing_deg,
            rate_dps,
            lag_s: self.lag_s,
            at_s,
        };
        let (mine, other) = match rotation {
            Rotation::Clockwise => (0, 1),
            Rotation::Counterclockwise => (1, 0),
        };
        if let Some(opposite) = self.last[other]
            && (at_s - opposite.at_s).abs() <= PAIR_WINDOW_S
            && wrap_deg(bearing_deg - opposite.bearing_deg).abs() <= PAIR_WINDOW_DEG
        {
            let (cw, ccw) = if mine == 0 {
                (this, opposite)
            } else {
                (opposite, this)
            };
            let rates = cw.rate_dps + ccw.rate_dps;
            if rates > 0.0 {
                let split = wrap_deg(cw.bearing_deg - ccw.bearing_deg);
                let lag = (split + cw.rate_dps * cw.lag_s + ccw.rate_dps * ccw.lag_s) / rates;
                self.lag_s = (self.lag_s + LAG_GAIN * (lag - self.lag_s)).clamp(0.0, MAX_LAG_S);
            }
        }
        self.last[mine] = Some(this);
    }
}
