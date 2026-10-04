use std::collections::VecDeque;

use sdrmm_wire::{
    DfEstimate, NavMode, NavTarget, NavTargetKind, PositionFix,
    geo::{self, LatLon},
};

use super::observation::Observation;

pub(crate) const RECENT_S: f64 = 20.0;
pub(crate) const NEAR_GUIDE_M: f64 = 300.0;
pub(crate) const RETARGET_M: f64 = 50.0;
pub(crate) const RETARGET_SHARE: f64 = 0.1;
pub(crate) const PROBE_TURN_DEG: f64 = 15.0;
pub(crate) const PROBE_REACHED_SHARE: f64 = 0.5;
pub(crate) const CONCENTRATED_MASS: f32 = 0.6;
pub(crate) const CONCENTRATED_MAJOR_M: f64 = 2_000.0;
pub(crate) const CONCENTRATED_SAMPLES: u32 = 6;
pub(crate) const TO_ESTIMATE_S: f64 = 3.0;
pub(crate) const TO_PROBE_S: f64 = 10.0;

const MAX_RECENT: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RecentBearing {
    at_s: f64,
    bearing_deg: f32,
    confidence: f32,
    lat: f64,
    lon: f64,
    mirrored: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NavFlags {
    pub(crate) no_guide_position: bool,
    pub(crate) no_bearings: bool,
}

pub(crate) struct Nav {
    mode: NavMode,
    probe_km: f64,
    kind: NavTargetKind,
    issued: Option<(NavTargetKind, LatLon)>,
    published: bool,
    concentrated_since_s: Option<f64>,
    loose_since_s: Option<f64>,
    revision: u32,
    recent: VecDeque<RecentBearing>,
}

impl Nav {
    pub(crate) fn new(mode: NavMode, probe_km: f64) -> Self {
        Self {
            mode,
            probe_km,
            kind: NavTargetKind::Probe,
            issued: None,
            published: false,
            concentrated_since_s: None,
            loose_since_s: None,
            revision: 0,
            recent: VecDeque::new(),
        }
    }

    pub(crate) fn configure(&mut self, mode: NavMode, probe_km: f64) {
        if mode != self.mode {
            self.kind = NavTargetKind::Probe;
            self.concentrated_since_s = None;
            self.loose_since_s = None;
        }
        self.mode = mode;
        self.probe_km = probe_km;
    }

    pub(crate) fn clear(&mut self) {
        self.kind = NavTargetKind::Probe;
        self.issued = None;
        self.concentrated_since_s = None;
        self.loose_since_s = None;
        self.recent.clear();
    }

    pub(crate) fn remember(&mut self, observation: &Observation) {
        if self.recent.len() == MAX_RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(RecentBearing {
            at_s: observation.at_s,
            bearing_deg: observation.bearing_deg,
            confidence: observation.confidence,
            lat: observation.lat,
            lon: observation.lon,
            mirrored: observation.mirrored,
        });
    }

    pub(crate) fn update(
        &mut self,
        guided: Option<&PositionFix>,
        estimate: Option<&DfEstimate>,
        now_s: f64,
    ) -> (Option<NavTarget>, NavFlags) {
        while self
            .recent
            .front()
            .is_some_and(|recent| now_s - recent.at_s > RECENT_S)
        {
            self.recent.pop_front();
        }
        self.dwell(estimate, now_s);
        let mut flags = NavFlags::default();
        let Some(here) = guided.map(|fix| LatLon {
            lat: fix.latitude,
            lon: fix.longitude,
        }) else {
            flags.no_guide_position = true;
            return (self.publish(None), flags);
        };
        let kind = match self.mode {
            NavMode::Off => return (self.publish(None), flags),
            NavMode::Direct if estimate.is_some() => NavTargetKind::Estimate,
            NavMode::Direct => NavTargetKind::Probe,
            NavMode::Auto => self.kind,
        };
        let point = match (kind, estimate) {
            (NavTargetKind::Estimate, Some(estimate)) => Some((
                NavTargetKind::Estimate,
                LatLon {
                    lat: estimate.lat,
                    lon: estimate.lon,
                },
            )),
            _ => self
                .probe(here, estimate)
                .map(|point| (NavTargetKind::Probe, point)),
        };
        let Some((kind, point)) = point else {
            flags.no_bearings = true;
            return (self.publish(None), flags);
        };
        let (revision, point) = self.revise(kind, point, here);
        let target = NavTarget {
            lat: point.lat,
            lon: point.lon,
            kind,
            revision,
            distance_m: geo::distance_m(here, point),
            bearing_deg: geo::bearing_deg(here, point),
        };
        (self.publish(Some(target)), flags)
    }

    fn dwell(&mut self, estimate: Option<&DfEstimate>, now_s: f64) {
        let concentrated = estimate.is_some_and(|estimate| {
            estimate.mass >= CONCENTRATED_MASS
                && estimate.ellipse_major_m <= CONCENTRATED_MAJOR_M
                && estimate.samples >= CONCENTRATED_SAMPLES
        });
        if concentrated {
            self.loose_since_s = None;
            self.concentrated_since_s.get_or_insert(now_s);
        } else {
            self.concentrated_since_s = None;
            self.loose_since_s.get_or_insert(now_s);
        }
        if self.mode != NavMode::Auto {
            return;
        }
        let held = |since: Option<f64>, wait_s: f64| since.is_some_and(|at| now_s - at >= wait_s);
        match self.kind {
            NavTargetKind::Probe if held(self.concentrated_since_s, TO_ESTIMATE_S) => {
                self.kind = NavTargetKind::Estimate;
            }
            NavTargetKind::Estimate if held(self.loose_since_s, TO_PROBE_S) => {
                self.kind = NavTargetKind::Probe;
            }
            _ => {}
        }
    }

    fn probe(&self, here: LatLon, estimate: Option<&DfEstimate>) -> Option<LatLon> {
        let (east, north, weight) = self
            .recent
            .iter()
            .filter(|recent| {
                !recent.mirrored
                    && geo::distance_m(
                        here,
                        LatLon {
                            lat: recent.lat,
                            lon: recent.lon,
                        },
                    ) <= NEAR_GUIDE_M
            })
            .fold((0.0f64, 0.0f64, 0.0f64), |(east, north, weight), recent| {
                let angle = f64::from(recent.bearing_deg).to_radians();
                let confidence = f64::from(recent.confidence);
                (
                    confidence.mul_add(angle.sin(), east),
                    confidence.mul_add(angle.cos(), north),
                    weight + confidence,
                )
            });
        let bearing = if weight > 0.0 && east.hypot(north) > f64::EPSILON {
            east.atan2(north).to_degrees()
        } else {
            let estimate = estimate?;
            geo::bearing_deg(
                here,
                LatLon {
                    lat: estimate.lat,
                    lon: estimate.lon,
                },
            )
        };
        Some(geo::destination(
            here,
            geo::wrap_360(bearing),
            self.probe_km * 1_000.0,
        ))
    }

    fn stale(&self, kind: NavTargetKind, issued: LatLon, fresh: LatLon, here: LatLon) -> bool {
        let to_issued = geo::distance_m(here, issued);
        match kind {
            NavTargetKind::Probe => {
                let turned =
                    geo::wrap_180(geo::bearing_deg(here, fresh) - geo::bearing_deg(here, issued))
                        .abs();
                turned > PROBE_TURN_DEG || to_issued < PROBE_REACHED_SHARE * self.probe_km * 1_000.0
            }
            NavTargetKind::Estimate => {
                geo::distance_m(issued, fresh) > RETARGET_M.max(RETARGET_SHARE * to_issued)
            }
        }
    }

    fn revise(&mut self, kind: NavTargetKind, fresh: LatLon, here: LatLon) -> (u32, LatLon) {
        let changed = match self.issued {
            None => self.published,
            Some((issued_kind, issued_at)) => {
                issued_kind != kind || self.stale(kind, issued_at, fresh, here)
            }
        };
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        if changed || self.issued.is_none() {
            self.issued = Some((kind, fresh));
        }
        self.published = true;
        let point = self.issued.map_or(fresh, |(_, at)| at);
        (self.revision, point)
    }

    fn publish(&mut self, target: Option<NavTarget>) -> Option<NavTarget> {
        if target.is_none() {
            self.issued = None;
        }
        target
    }
}
