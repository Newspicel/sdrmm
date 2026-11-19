use std::time::Instant;

use sdrmm_wire::{RotatorAxes, RotatorLink, SkyPoint};

use super::link::{Link, LinkError};

/// Close to a Yaesu G-5500: a full turn takes about a minute, elevation half as fast.
pub(super) const AZIMUTH_SLEW_DEG_S: f64 = 6.0;
pub(super) const ELEVATION_SLEW_DEG_S: f64 = 3.0;

const REST: SkyPoint = SkyPoint {
    azimuth_deg: 0.0,
    elevation_deg: 0.0,
};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Slew {
    from: SkyPoint,
    to: SkyPoint,
    since: Instant,
}

impl Slew {
    const fn resting(at: SkyPoint, since: Instant) -> Self {
        Self {
            from: at,
            to: at,
            since,
        }
    }

    fn at(&self, now: Instant) -> SkyPoint {
        let elapsed_s = now.saturating_duration_since(self.since).as_secs_f64();
        SkyPoint {
            azimuth_deg: toward(
                self.from.azimuth_deg,
                self.to.azimuth_deg,
                AZIMUTH_SLEW_DEG_S * elapsed_s,
            ),
            elevation_deg: toward(
                self.from.elevation_deg,
                self.to.elevation_deg,
                ELEVATION_SLEW_DEG_S * elapsed_s,
            ),
        }
    }
}

fn toward(from_deg: f64, to_deg: f64, step_deg: f64) -> f64 {
    from_deg + (to_deg - from_deg).clamp(-step_deg, step_deg)
}

pub(super) struct VirtualMount {
    axes: RotatorAxes,
    slew: Slew,
}

impl VirtualMount {
    pub(super) const fn new(axes: RotatorAxes, now: Instant) -> Self {
        Self {
            axes,
            slew: Slew::resting(REST, now),
        }
    }

    fn position_at(&self, now: Instant) -> SkyPoint {
        self.slew.at(now)
    }

    fn point_at(&mut self, aim: SkyPoint, now: Instant) {
        self.slew = Slew {
            from: self.slew.at(now),
            to: self.travel(aim),
            since: now,
        };
    }

    fn stop_at(&mut self, now: Instant) {
        self.slew = Slew::resting(self.slew.at(now), now);
    }

    fn travel(&self, aim: SkyPoint) -> SkyPoint {
        let link = RotatorLink::Virtual;
        let elevation_deg = match self.axes {
            RotatorAxes::AzimuthElevation => {
                aim.elevation_deg.clamp(0.0, link.elevation_limit_deg())
            }
            RotatorAxes::Azimuth => REST.elevation_deg,
        };
        SkyPoint {
            azimuth_deg: aim.azimuth_deg.clamp(0.0, link.azimuth_limit_deg()),
            elevation_deg,
        }
    }
}

impl Link for VirtualMount {
    fn position(&mut self) -> Result<SkyPoint, LinkError> {
        Ok(self.position_at(Instant::now()))
    }

    fn point(&mut self, aim: SkyPoint) -> Result<(), LinkError> {
        self.point_at(aim, Instant::now());
        Ok(())
    }

    fn stop(&mut self) -> Result<(), LinkError> {
        self.stop_at(Instant::now());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sdrmm_wire::{FLIP_ELEVATION_DEG, MAX_ROTATOR_AZIMUTH_DEG};

    use super::*;

    const BOTH: RotatorAxes = RotatorAxes::AzimuthElevation;

    fn sky(azimuth_deg: f64, elevation_deg: f64) -> SkyPoint {
        SkyPoint {
            azimuth_deg,
            elevation_deg,
        }
    }

    fn after(start: Instant, seconds: f64) -> Instant {
        start + Duration::from_secs_f64(seconds)
    }

    fn near(got: SkyPoint, want: SkyPoint) -> bool {
        got.off_by(want) < 1e-9
    }

    #[test]
    fn a_step_never_overshoots_and_works_both_ways() {
        assert!((toward(10.0, 100.0, 6.0) - 16.0).abs() < 1e-12);
        assert!((toward(100.0, 10.0, 6.0) - 94.0).abs() < 1e-12);
        assert!((toward(10.0, 12.0, 6.0) - 12.0).abs() < 1e-12);
        assert!((toward(10.0, 10.0, 0.0) - 10.0).abs() < 1e-12);
    }

    #[test]
    fn a_new_mount_rests_at_its_stop() {
        let start = Instant::now();
        let mount = VirtualMount::new(BOTH, start);
        assert!(near(mount.position_at(after(start, 30.0)), REST));
    }

    #[test]
    fn each_axis_slews_at_its_own_rate_and_settles_on_the_target() {
        let start = Instant::now();
        let mut mount = VirtualMount::new(BOTH, start);
        mount.point_at(sky(90.0, 45.0), start);
        assert!(near(
            mount.position_at(after(start, 5.0)),
            sky(5.0 * AZIMUTH_SLEW_DEG_S, 5.0 * ELEVATION_SLEW_DEG_S)
        ));
        let azimuth_done = 90.0 / AZIMUTH_SLEW_DEG_S;
        assert!(near(
            mount.position_at(after(start, azimuth_done)),
            sky(90.0, azimuth_done * ELEVATION_SLEW_DEG_S)
        ));
        assert!(near(mount.position_at(after(start, 60.0)), sky(90.0, 45.0)));
    }

    #[test]
    fn a_new_target_starts_from_where_the_mount_is() {
        let start = Instant::now();
        let mut mount = VirtualMount::new(BOTH, start);
        mount.point_at(sky(120.0, 0.0), start);
        let turn = after(start, 10.0);
        mount.point_at(sky(0.0, 0.0), turn);
        let here = 10.0 * AZIMUTH_SLEW_DEG_S;
        assert!(near(mount.position_at(turn), sky(here, 0.0)));
        assert!(near(
            mount.position_at(after(turn, 2.0)),
            sky(here - 2.0 * AZIMUTH_SLEW_DEG_S, 0.0)
        ));
    }

    #[test]
    fn stopping_holds_the_mount_where_it_is() {
        let start = Instant::now();
        let mut mount = VirtualMount::new(BOTH, start);
        mount.point_at(sky(180.0, 60.0), start);
        let halt = after(start, 4.0);
        mount.stop_at(halt);
        let held = sky(4.0 * AZIMUTH_SLEW_DEG_S, 4.0 * ELEVATION_SLEW_DEG_S);
        assert!(near(mount.position_at(after(halt, 100.0)), held));
    }

    #[test]
    fn a_target_past_the_travel_stops_at_the_end_of_it() {
        let start = Instant::now();
        let mut mount = VirtualMount::new(BOTH, start);
        mount.point_at(sky(900.0, 400.0), start);
        let settled = mount.position_at(after(start, 3_600.0));
        assert!(near(
            settled,
            sky(MAX_ROTATOR_AZIMUTH_DEG, FLIP_ELEVATION_DEG)
        ));
        mount.point_at(sky(-20.0, -5.0), after(start, 3_600.0));
        assert!(near(mount.position_at(after(start, 7_200.0)), REST));
    }

    #[test]
    fn a_turntable_never_tilts() {
        let start = Instant::now();
        let mut mount = VirtualMount::new(RotatorAxes::Azimuth, start);
        mount.point_at(sky(30.0, 70.0), start);
        assert!(near(mount.position_at(after(start, 60.0)), sky(30.0, 0.0)));
    }

    #[test]
    fn the_link_reports_points_and_stops_without_hardware() {
        let mut link = super::super::link::open(&RotatorLink::Virtual, BOTH).expect("opens");
        assert!(near(link.position().expect("reads"), REST));
        link.point(sky(200.0, 30.0)).expect("points");
        link.stop().expect("stops");
        let held = link.position().expect("reads");
        assert!(held.azimuth_deg < 200.0 && held.elevation_deg < 30.0);
    }
}
