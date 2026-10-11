use sdrmm_wire::{PassPoint, RotatorNode, SkyPoint};

const TURN: f64 = 360.0;
const HALF_TURN: f64 = 180.0;
const FLIP_BIAS_DEG: f64 = 45.0;
const SAME_PASS_S: i64 = 60;
const SWINGS: [(bool, bool); 4] = [(false, false), (true, true), (false, true), (true, false)];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Mount {
    pub(super) azimuth_max_deg: f64,
    pub(super) elevation_max_deg: f64,
    pub(super) flips: bool,
}

impl Mount {
    pub(super) fn of(node: &RotatorNode) -> Self {
        Self {
            azimuth_max_deg: node.azimuth_max_deg,
            elevation_max_deg: node.elevation_max_deg(),
            flips: node.flips(),
        }
    }

    pub(super) fn reach(self, point: SkyPoint) -> SkyPoint {
        SkyPoint {
            azimuth_deg: point.azimuth_deg.clamp(0.0, self.azimuth_max_deg),
            elevation_deg: point.elevation_deg.clamp(0.0, self.elevation_max_deg),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Plan {
    mount: Mount,
    culmination: i64,
    flip_before: bool,
    flip_after: bool,
    path: Vec<(i64, f64)>,
}

impl Plan {
    pub(super) fn serves(&self, mount: Mount, culmination: i64) -> bool {
        self.mount == mount && (self.culmination - culmination).abs() <= SAME_PASS_S
    }

    fn flipped(&self, at: i64) -> bool {
        if at < self.culmination {
            self.flip_before
        } else {
            self.flip_after
        }
    }

    fn azimuth_near(&self, at: i64) -> Option<f64> {
        let after = self.path.partition_point(|&(when, _)| when < at);
        let candidates = [after.checked_sub(1), Some(after)];
        candidates
            .into_iter()
            .flatten()
            .filter_map(|index| self.path.get(index))
            .min_by_key(|(when, _)| (when - at).abs())
            .map(|&(_, azimuth)| azimuth)
    }
}

pub(super) fn plan(
    track: &[PassPoint],
    culmination: i64,
    mount: Mount,
    from_deg: f64,
) -> Option<Plan> {
    let swings = if mount.flips {
        &SWINGS[..]
    } else {
        &SWINGS[..1]
    };
    swings
        .iter()
        .filter_map(|&(before, after)| {
            let swing = Swing {
                culmination,
                before,
                after,
            };
            swing.cost(track, mount, from_deg)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, plan)| plan)
}

#[derive(Clone, Copy)]
struct Swing {
    culmination: i64,
    before: bool,
    after: bool,
}

impl Swing {
    fn flipped(self, at: i64) -> bool {
        if at < self.culmination {
            self.before
        } else {
            self.after
        }
    }

    fn cost(self, track: &[PassPoint], mount: Mount, from_deg: f64) -> Option<(f64, Plan)> {
        let mut path: Vec<(i64, f64)> = Vec::with_capacity(track.len());
        let mut travel = 0.0;
        for point in track {
            let raw = point.azimuth_deg
                + if self.flipped(point.at) {
                    HALF_TURN
                } else {
                    0.0
                };
            let azimuth = match path.last() {
                None => raw.rem_euclid(TURN),
                Some(&(_, previous)) => {
                    let step = (raw - previous + HALF_TURN).rem_euclid(TURN) - HALF_TURN;
                    travel += step.abs();
                    previous + step
                }
            };
            path.push((point.at, azimuth));
        }
        let first = path.first()?.1;
        let shift = fitting_shift(&path, mount, first, from_deg)?;
        let cost = travel
            + self.switch_cost(track)
            + (first + shift - from_deg).abs()
            + FLIP_BIAS_DEG * f64::from(u8::from(self.before) + u8::from(self.after));
        let plan = Plan {
            mount,
            culmination: self.culmination,
            flip_before: self.before,
            flip_after: self.after,
            path: path.into_iter().map(|(at, az)| (at, az + shift)).collect(),
        };
        Some((cost, plan))
    }

    fn switch_cost(self, track: &[PassPoint]) -> f64 {
        if self.before == self.after {
            return 0.0;
        }
        track
            .iter()
            .min_by_key(|point| (point.at - self.culmination).abs())
            .map_or(HALF_TURN, |peak| {
                (HALF_TURN - 2.0 * peak.elevation_deg).abs()
            })
    }
}

fn fitting_shift(path: &[(i64, f64)], mount: Mount, first: f64, from_deg: f64) -> Option<f64> {
    let low = path.iter().map(|&(_, az)| az).fold(f64::INFINITY, f64::min);
    let high = path
        .iter()
        .map(|&(_, az)| az)
        .fold(f64::NEG_INFINITY, f64::max);
    (-2..=2)
        .map(|turns: i32| f64::from(turns) * TURN)
        .filter(|shift| low + shift >= 0.0 && high + shift <= mount.azimuth_max_deg)
        .min_by(|a, b| {
            (first + a - from_deg)
                .abs()
                .total_cmp(&(first + b - from_deg).abs())
        })
}

pub(super) fn steer(
    sky: SkyPoint,
    at: i64,
    plan: Option<&Plan>,
    mount: Mount,
    from_deg: f64,
) -> (SkyPoint, bool) {
    let flipped = plan.is_some_and(|plan| plan.flipped(at));
    let (azimuth, elevation) = if flipped {
        (sky.azimuth_deg + HALF_TURN, HALF_TURN - sky.elevation_deg)
    } else {
        (sky.azimuth_deg, sky.elevation_deg)
    };
    let reference = plan
        .and_then(|plan| plan.azimuth_near(at))
        .unwrap_or(from_deg);
    let target = SkyPoint {
        azimuth_deg: nearest_turn(azimuth, reference, mount.azimuth_max_deg),
        elevation_deg: elevation,
    };
    (mount.reach(target), flipped)
}

fn nearest_turn(azimuth: f64, reference: f64, azimuth_max_deg: f64) -> f64 {
    let base = azimuth.rem_euclid(TURN);
    [base + TURN]
        .into_iter()
        .filter(|candidate| *candidate <= azimuth_max_deg)
        .fold(base, |best, candidate| {
            if (candidate - reference).abs() < (best - reference).abs() {
                candidate
            } else {
                best
            }
        })
}

#[cfg(test)]
mod tests;
