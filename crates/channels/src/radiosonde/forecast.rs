use std::collections::HashMap;

use sdrmm_wire::{
    DEFAULT_SONDE_BURST_M, LatLon, RadiosondeFrame, RadiosondeParams, SondeForecast, geo::offset_m,
};

const LAYER_M: f64 = 250.0;
const LAYERS: usize = 200;
const STEP_S: f64 = 5.0;
const MAX_FLIGHT_S: f64 = 6.0 * 3_600.0;
const PATH_EVERY: u32 = 12;
const SCALE_HEIGHT_M: f64 = 7_200.0;
const WIND_WEIGHT: f64 = 0.2;
const RATE_WEIGHT: f64 = 0.1;
const MOVING_MS: f64 = 0.5;
const FALLEN_M: f64 = 50.0;
const DEFAULT_ASCENT_MS: f64 = 5.0;
const DEFAULT_SEA_DESCENT_MS: f64 = 5.0;
const MAX_FLIGHTS: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Wind {
    east_ms: f64,
    north_ms: f64,
    seen: bool,
}

type Profile = [Wind; LAYERS];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Fix {
    at: LatLon,
    altitude_m: f64,
}

struct Flight {
    winds: Box<Profile>,
    peak: Fix,
    ascended: bool,
    descending: bool,
    ascent_ms: Option<f64>,
    sea_descent_ms: Option<f64>,
    seen: u64,
}

#[derive(Default)]
pub(crate) struct Flights {
    flights: HashMap<String, Flight>,
    clock: u64,
}

impl Flights {
    pub(crate) fn observe(&mut self, frame: &mut RadiosondeFrame, params: &RadiosondeParams) {
        let (Some(lat), Some(lon), Some(altitude_m), Some(climb_ms)) =
            (frame.lat, frame.lon, frame.altitude_m, frame.climb_ms)
        else {
            return;
        };
        if !(lat.is_finite() && lon.is_finite() && altitude_m.is_finite() && climb_ms.is_finite()) {
            return;
        }
        let fix = Fix {
            at: LatLon { lat, lon },
            altitude_m,
        };
        self.clock += 1;
        let clock = self.clock;
        let Some(flight) = self.flight(&frame.serial, fix) else {
            return;
        };
        flight.seen = clock;
        flight.learn(fix, climb_ms, wind_of(frame));
        frame.forecast = flight.forecast(fix, climb_ms, params);
    }

    fn flight(&mut self, serial: &str, fix: Fix) -> Option<&mut Flight> {
        if !self.flights.contains_key(serial) {
            if self.flights.len() >= MAX_FLIGHTS {
                self.evict_oldest();
            }
            self.flights.insert(serial.to_owned(), Flight::new(fix));
        }
        self.flights.get_mut(serial)
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest) = self
            .flights
            .iter()
            .min_by_key(|(_, flight)| flight.seen)
            .map(|(serial, _)| serial.clone())
        {
            self.flights.remove(&oldest);
        }
    }
}

fn wind_of(frame: &RadiosondeFrame) -> Option<(f64, f64)> {
    let (speed, heading) = (frame.speed_ms?, frame.heading_deg?);
    if !(speed.is_finite() && heading.is_finite()) {
        return None;
    }
    let (sin, cos) = heading.to_radians().sin_cos();
    Some((speed * sin, speed * cos))
}

fn layer(altitude_m: f64) -> usize {
    ((altitude_m.max(0.0) / LAYER_M) as usize).min(LAYERS - 1)
}

fn blend(old: Option<f64>, new: f64, weight: f64) -> f64 {
    old.map_or(new, |old| old + weight * (new - old))
}

fn thinning(altitude_m: f64) -> f64 {
    (altitude_m / (2.0 * SCALE_HEIGHT_M)).exp()
}

impl Flight {
    fn new(fix: Fix) -> Self {
        Self {
            winds: Box::new([Wind::default(); LAYERS]),
            peak: fix,
            ascended: false,
            descending: false,
            ascent_ms: None,
            sea_descent_ms: None,
            seen: 0,
        }
    }

    fn learn(&mut self, fix: Fix, climb_ms: f64, wind: Option<(f64, f64)>) {
        if let Some((east_ms, north_ms)) = wind {
            let slot = &mut self.winds[layer(fix.altitude_m)];
            let weight = if slot.seen { WIND_WEIGHT } else { 1.0 };
            *slot = Wind {
                east_ms: blend(Some(slot.east_ms), east_ms, weight),
                north_ms: blend(Some(slot.north_ms), north_ms, weight),
                seen: true,
            };
        }
        if fix.altitude_m > self.peak.altitude_m {
            self.peak = fix;
        }
        if climb_ms > MOVING_MS && !self.descending {
            self.ascended = true;
            self.ascent_ms = Some(blend(self.ascent_ms, climb_ms, RATE_WEIGHT));
        }
        if climb_ms < -MOVING_MS && fix.altitude_m < self.peak.altitude_m - FALLEN_M {
            self.descending = true;
        }
        if self.descending && climb_ms < -MOVING_MS {
            let sea = -climb_ms / thinning(fix.altitude_m);
            self.sea_descent_ms = Some(blend(self.sea_descent_ms, sea, RATE_WEIGHT));
        }
    }

    fn forecast(
        &self,
        fix: Fix,
        climb_ms: f64,
        params: &RadiosondeParams,
    ) -> Option<SondeForecast> {
        let ground_m = f64::from(params.ground_m.unwrap_or(0));
        if fix.altitude_m <= ground_m {
            return None;
        }
        let profile = filled(&self.winds);
        let mut run = Run::new(fix, &profile);
        let burst = if self.descending {
            self.peak
        } else if climb_ms > MOVING_MS {
            let burst_m = f64::from(params.burst_m.unwrap_or(DEFAULT_SONDE_BURST_M));
            let rate = self.ascent_ms.unwrap_or(DEFAULT_ASCENT_MS).max(MOVING_MS);
            run.climb(burst_m.max(fix.altitude_m), rate)?
        } else {
            return None;
        };
        let sea_descent = self.sea_descent_ms.unwrap_or(DEFAULT_SEA_DESCENT_MS);
        let landing = run.fall(ground_m, sea_descent)?;
        Some(SondeForecast {
            burst: burst.at,
            burst_altitude_m: burst.altitude_m,
            burst_seen: self.descending && self.ascended,
            landing: landing.at,
            landing_in_s: run.elapsed_s,
            path: run.path,
        })
    }
}

fn filled(winds: &Profile) -> Profile {
    let mut profile = *winds;
    let Some(first) = profile.iter().position(|wind| wind.seen) else {
        return profile;
    };
    let lowest = profile[first];
    for slot in &mut profile[..first] {
        *slot = lowest;
    }
    let mut last = lowest;
    for slot in &mut profile[first..] {
        if slot.seen {
            last = *slot;
        } else {
            *slot = last;
        }
    }
    profile
}

struct Run<'a> {
    fix: Fix,
    profile: &'a Profile,
    elapsed_s: f64,
    steps: u32,
    path: Vec<LatLon>,
}

impl<'a> Run<'a> {
    fn new(fix: Fix, profile: &'a Profile) -> Self {
        Self {
            fix,
            profile,
            elapsed_s: 0.0,
            steps: 0,
            path: vec![fix.at],
        }
    }

    fn climb(&mut self, burst_m: f64, rate_ms: f64) -> Option<Fix> {
        while self.fix.altitude_m < burst_m {
            self.step((rate_ms * STEP_S).min(burst_m - self.fix.altitude_m))?;
        }
        self.path.push(self.fix.at);
        Some(self.fix)
    }

    fn fall(&mut self, ground_m: f64, sea_descent_ms: f64) -> Option<Fix> {
        while self.fix.altitude_m > ground_m {
            let rate = sea_descent_ms * thinning(self.fix.altitude_m);
            self.step(-(rate * STEP_S).min(self.fix.altitude_m - ground_m))?;
        }
        self.path.push(self.fix.at);
        Some(self.fix)
    }

    fn step(&mut self, rise_m: f64) -> Option<()> {
        if self.elapsed_s >= MAX_FLIGHT_S {
            return None;
        }
        let wind = self.profile[layer(self.fix.altitude_m)];
        self.fix = Fix {
            at: offset_m(self.fix.at, wind.east_ms * STEP_S, wind.north_ms * STEP_S),
            altitude_m: self.fix.altitude_m + rise_m,
        };
        self.elapsed_s += STEP_S;
        self.steps += 1;
        if self.steps.is_multiple_of(PATH_EVERY) {
            self.path.push(self.fix.at);
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{SondeType, geo::distance_m};

    use super::*;
    use crate::radiosonde::fields::empty_frame;

    const LAUNCH: LatLon = LatLon {
        lat: 48.0,
        lon: 11.0,
    };

    fn frame(at: LatLon, altitude_m: f64, climb_ms: f64, wind: (f64, f64)) -> RadiosondeFrame {
        let mut frame = empty_frame(SondeType::Rs41, "S1234567".to_owned());
        frame.lat = Some(at.lat);
        frame.lon = Some(at.lon);
        frame.altitude_m = Some(altitude_m);
        frame.climb_ms = Some(climb_ms);
        frame.speed_ms = Some(wind.0);
        frame.heading_deg = Some(wind.1);
        frame
    }

    fn ascend(flights: &mut Flights, params: &RadiosondeParams, to_m: f64) -> RadiosondeFrame {
        let mut last = frame(LAUNCH, 0.0, 5.0, (10.0, 90.0));
        let mut altitude = 0.0;
        let mut at = LAUNCH;
        while altitude < to_m {
            last = frame(at, altitude, 5.0, (10.0, 90.0));
            flights.observe(&mut last, params);
            altitude += 5.0;
            at = offset_m(at, 10.0, 0.0);
        }
        last
    }

    #[test]
    fn an_ascent_in_an_east_wind_bursts_and_lands_downwind() {
        let mut flights = Flights::default();
        let params = RadiosondeParams::default();
        let last = ascend(&mut flights, &params, 1_000.0);
        let forecast = last.forecast.expect("an ascending sonde is forecast");
        let here = LatLon {
            lat: last.lat.expect("lat"),
            lon: last.lon.expect("lon"),
        };
        let rise = f64::from(DEFAULT_SONDE_BURST_M) - last.altitude_m.expect("altitude");
        let drift = distance_m(here, forecast.burst);
        assert!(
            (drift - 10.0 * rise / 5.0).abs() < 1_000.0,
            "{drift} {forecast:?}"
        );
        assert!(forecast.burst.lon > here.lon && forecast.landing.lon > forecast.burst.lon);
        assert!((forecast.burst_altitude_m - f64::from(DEFAULT_SONDE_BURST_M)).abs() < 1e-6);
        assert!(!forecast.burst_seen);
        assert!(forecast.path.len() > 10);
    }

    #[test]
    fn a_falling_sonde_lands_where_the_measured_winds_carry_it() {
        let mut flights = Flights::default();
        let params = RadiosondeParams::default();
        let top = ascend(&mut flights, &params, 3_000.0);
        let peak = LatLon {
            lat: top.lat.expect("lat"),
            lon: top.lon.expect("lon"),
        };
        let mut falling = frame(peak, 2_000.0, -6.0, (10.0, 90.0));
        flights.observe(&mut falling, &params);
        let forecast = falling.forecast.expect("a falling sonde is forecast");
        assert!(forecast.burst_seen);
        assert!(distance_m(forecast.burst, peak) < 1.0);
        assert!(forecast.landing.lon > peak.lon);
        assert!(
            (250.0..450.0).contains(&forecast.landing_in_s),
            "{forecast:?}"
        );
    }

    #[test]
    fn higher_ground_lands_sooner() {
        let landing_in = |ground_m| {
            let mut flights = Flights::default();
            let params = RadiosondeParams {
                ground_m: Some(ground_m),
                ..RadiosondeParams::default()
            };
            ascend(&mut flights, &params, 3_000.0);
            let mut falling = frame(LAUNCH, 2_000.0, -6.0, (0.0, 0.0));
            flights.observe(&mut falling, &params);
            falling.forecast.expect("forecast").landing_in_s
        };
        assert!(landing_in(800) < landing_in(0));
    }

    #[test]
    fn a_sonde_on_the_ground_has_no_forecast() {
        let mut flights = Flights::default();
        let params = RadiosondeParams::default();
        let mut idle = frame(LAUNCH, 500.0, 0.1, (0.0, 0.0));
        flights.observe(&mut idle, &params);
        assert_eq!(idle.forecast, None);
        let mut below = frame(LAUNCH, -5.0, -3.0, (0.0, 0.0));
        flights.observe(&mut below, &params);
        assert_eq!(below.forecast, None);
    }

    #[test]
    fn a_crawling_ascent_is_not_forecast() {
        let mut flights = Flights::default();
        let params = RadiosondeParams::default();
        let mut slow = frame(LAUNCH, 100.0, 0.6, (0.0, 0.0));
        flights.observe(&mut slow, &params);
        assert_eq!(slow.forecast, None);
    }

    #[test]
    fn only_the_most_recent_flights_are_kept() {
        let mut flights = Flights::default();
        let params = RadiosondeParams::default();
        for index in 0..=MAX_FLIGHTS {
            let mut next = frame(LAUNCH, 1_000.0, 5.0, (0.0, 0.0));
            next.serial = format!("S{index}");
            flights.observe(&mut next, &params);
        }
        assert_eq!(flights.flights.len(), MAX_FLIGHTS);
        assert!(!flights.flights.contains_key("S0"));
    }

    #[test]
    fn gaps_in_the_wind_profile_take_the_nearest_layer_below() {
        let mut winds = [Wind::default(); LAYERS];
        winds[4] = Wind {
            east_ms: 3.0,
            north_ms: 0.0,
            seen: true,
        };
        winds[10] = Wind {
            east_ms: 7.0,
            north_ms: 0.0,
            seen: true,
        };
        let profile = filled(&winds);
        assert_eq!(profile[0].east_ms, 3.0);
        assert_eq!(profile[8].east_ms, 3.0);
        assert_eq!(profile[LAYERS - 1].east_ms, 7.0);
    }
}
