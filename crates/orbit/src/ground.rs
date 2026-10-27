use crate::{
    OrbitError, Satellite, State,
    observer::{WGS84_A_KM, WGS84_F, earth_fixed},
    time::{SECONDS_PER_DAY, julian_date, sidereal_angle},
};

const MEAN_EARTH_RADIUS_KM: f64 = 6_371.0;
const LATITUDE_ITERATIONS: usize = 6;
const MAX_TRACK_STEPS: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Subpoint {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_km: f64,
}

impl Subpoint {
    #[must_use]
    pub fn of(state: &State, julian_date: f64) -> Self {
        let (position, _) = earth_fixed(state, sidereal_angle(julian_date));
        geodetic(position)
    }

    #[must_use]
    pub fn footprint_km(&self) -> f64 {
        let radius = MEAN_EARTH_RADIUS_KM;
        radius * (radius / (radius + self.altitude_km.max(0.0))).acos()
    }
}

impl Satellite {
    pub fn subpoint(&self, unix_seconds: f64) -> Result<Subpoint, OrbitError> {
        let jd = julian_date(unix_seconds);
        Ok(Subpoint::of(&self.state_at(jd)?, jd))
    }

    pub fn ground_track(
        &self,
        from_unix: f64,
        to_unix: f64,
        step_s: f64,
    ) -> Result<Vec<Subpoint>, OrbitError> {
        if !step_s.is_finite()
            || step_s <= 0.0
            || !from_unix.is_finite()
            || !to_unix.is_finite()
            || to_unix < from_unix
        {
            return Err(OrbitError::Elements("an empty or reversed ground track"));
        }
        let span = ((to_unix - from_unix) / step_s).ceil();
        if span >= MAX_TRACK_STEPS as f64 {
            return Err(OrbitError::Elements("a ground track of too many points"));
        }
        let steps = span as usize;
        (0..=steps)
            .map(|step| self.subpoint((from_unix + step as f64 * step_s).min(to_unix)))
            .collect()
    }

    #[must_use]
    pub fn period_s(&self) -> f64 {
        SECONDS_PER_DAY / self.tle.mean_motion_rev_per_day.max(f64::MIN_POSITIVE)
    }
}

fn geodetic([x, y, z]: [f64; 3]) -> Subpoint {
    let e2 = WGS84_F * (2.0 - WGS84_F);
    let p = x.hypot(y);
    let mut latitude = z.atan2(p * (1.0 - e2));
    let mut n = WGS84_A_KM;
    for _ in 0..LATITUDE_ITERATIONS {
        let sin = latitude.sin();
        n = WGS84_A_KM / (1.0 - e2 * sin * sin).sqrt();
        latitude = (z + e2 * n * sin).atan2(p);
    }
    let (sin, cos) = latitude.sin_cos();
    Subpoint {
        latitude_deg: latitude.to_degrees(),
        longitude_deg: y.atan2(x).to_degrees(),
        altitude_km: p * cos + z * sin - WGS84_A_KM * WGS84_A_KM / n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Observer, Tle, time::unix_seconds};

    const ISS: &str = "ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000-0  30306-3 0  9999
2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.50377579432041";

    fn iss() -> Satellite {
        Satellite::new(Tle::parse(ISS).expect("valid")).expect("usable")
    }

    #[test]
    fn a_point_on_the_equator_reads_back() {
        let ground = geodetic([WGS84_A_KM + 400.0, 0.0, 0.0]);
        assert!(ground.latitude_deg.abs() < 1e-9, "{ground:?}");
        assert!(ground.longitude_deg.abs() < 1e-9, "{ground:?}");
        assert!((ground.altitude_km - 400.0).abs() < 1e-9, "{ground:?}");
    }

    #[test]
    fn a_point_over_the_pole_reads_back() {
        let polar = WGS84_A_KM * (1.0 - WGS84_F);
        let ground = geodetic([0.0, 0.0, polar + 800.0]);
        assert!((ground.latitude_deg - 90.0).abs() < 1e-9, "{ground:?}");
        assert!((ground.altitude_km - 800.0).abs() < 1e-6, "{ground:?}");
    }

    #[test]
    fn the_satellite_sits_at_the_zenith_of_its_subpoint() {
        let iss = iss();
        let now = unix_seconds(iss.tle.epoch_jd) + 1_234.0;
        let ground = iss.subpoint(now).expect("propagates");
        let observer = Observer {
            latitude_deg: ground.latitude_deg,
            longitude_deg: ground.longitude_deg,
            altitude_m: 0.0,
        };
        let look = iss.look(&observer, now).expect("propagates");
        assert!(look.elevation_deg > 89.9, "{look:?}");
        assert!(
            (look.range_km - ground.altitude_km).abs() < 0.01,
            "{look:?} {ground:?}"
        );
    }

    #[test]
    fn the_station_stays_inside_its_inclination_and_low_orbit() {
        let iss = iss();
        let start = unix_seconds(iss.tle.epoch_jd);
        let track = iss
            .ground_track(start, start + iss.period_s(), 60.0)
            .expect("propagates");
        assert!(track.len() > 90, "{}", track.len());
        assert!(track.iter().all(|point| point.latitude_deg.abs() < 52.0));
        assert!(track.iter().any(|point| point.latitude_deg > 50.0));
        assert!(
            track
                .iter()
                .all(|point| (350.0..450.0).contains(&point.altitude_km))
        );
    }

    #[test]
    fn a_low_orbit_sees_a_couple_of_thousand_kilometres() {
        let ground = Subpoint {
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            altitude_km: 420.0,
        };
        assert!((2_200.0..2_300.0).contains(&ground.footprint_km()));
    }

    #[test]
    fn a_reversed_track_is_refused() {
        let iss = iss();
        assert!(iss.ground_track(10.0, 0.0, 60.0).is_err());
        assert!(iss.ground_track(0.0, 10.0, 0.0).is_err());
    }

    #[test]
    fn a_track_that_would_never_end_is_refused() {
        let iss = iss();
        let start = unix_seconds(iss.tle.epoch_jd);
        for (from, to, step) in [
            (start, f64::INFINITY, 60.0),
            (f64::NEG_INFINITY, start, 60.0),
            (start, f64::NAN, 60.0),
            (start, start + 1e12, 60.0),
            (start, start + 60.0, f64::INFINITY),
        ] {
            assert!(
                iss.ground_track(from, to, step).is_err(),
                "{from} {to} {step}"
            );
        }
    }
}
