use num_complex::Complex;

use crate::{filter::iir::one_pole_coeff, math::db::Decibels, tuning::ddc::flat_bandwidth_hz};

mod guard;

use guard::GuardMeter;

const POWER_TAU_S: f64 = 3e-3;

const FLOOR_FALL_TAU_S: f64 = 0.3;
const FLOOR_RISE_TAU_S: f64 = 10.0;
const FLOOR_WARMUP_S: f64 = 50e-3;
const AUTO_MIN_LIN: f32 = 1e-12;
const AUTO_MAX_LIN: f32 = 1.0;

#[derive(Clone, Debug)]
pub struct Squelch {
    rate: f64,
    power: f32,
    coeff: f32,
    threshold_db: f32,
    open_lin: f32,
    close_lin: f32,
    hysteresis_lin: f32,
    hold_samples: u64,
    quiet: u64,
    open: bool,
    floor: f32,
    floor_fall: f32,
    floor_rise: f32,
    warmup: u64,
    warmup_samples: u64,
    auto_margin_db: Option<f32>,
    auto_margin_lin: f32,
    guard: Option<GuardMeter>,
    guard_quiet: f32,
}

impl Squelch {
    #[must_use]
    pub fn new(rate: f64, threshold_db: f32, hysteresis_db: f32, hold_s: f32) -> Self {
        assert!(
            rate > 0.0 && hysteresis_db >= 0.0 && hold_s >= 0.0,
            "invalid squelch parameters"
        );
        let mut s = Self {
            rate,
            power: 0.0,
            coeff: one_pole_coeff(rate, POWER_TAU_S),
            threshold_db,
            open_lin: 0.0,
            close_lin: 0.0,
            hysteresis_lin: (-hysteresis_db).db_to_power(),
            hold_samples: (f64::from(hold_s) * rate).round() as u64,
            quiet: 0,
            open: false,
            floor: 0.0,
            floor_fall: one_pole_coeff(rate, FLOOR_FALL_TAU_S),
            floor_rise: one_pole_coeff(rate, FLOOR_RISE_TAU_S),
            warmup: (FLOOR_WARMUP_S * rate).round() as u64,
            warmup_samples: (FLOOR_WARMUP_S * rate).round() as u64,
            auto_margin_db: None,
            auto_margin_lin: 1.0,
            guard: None,
            guard_quiet: 0.0,
        };
        s.recompute_thresholds();
        s
    }

    #[must_use]
    pub fn with_guard_band(self, band_low_hz: f64, band_high_hz: f64) -> Self {
        let rate = self.rate;
        self.with_guard_band_at(rate, band_low_hz, band_high_hz)
    }

    /// [`Self::with_guard_band`] for a wide signal at `wide_rate` that is clean only within this
    /// squelch's own flat band, as a converter guarding a narrower band delivers it.
    pub fn with_guard_band_at(
        mut self,
        wide_rate: f64,
        band_low_hz: f64,
        band_high_hz: f64,
    ) -> Self {
        let edge_hz = flat_bandwidth_hz(self.rate) / 2.0;
        self.guard = GuardMeter::new(wide_rate, edge_hz, band_low_hz, band_high_hz);
        self
    }

    pub fn set_threshold_db(&mut self, db: f32) {
        self.threshold_db = db;
        self.recompute_thresholds();
    }

    pub fn set_auto_margin_db(&mut self, margin_db: Option<f32>) {
        self.auto_margin_db = margin_db;
        self.auto_margin_lin = margin_db.map_or(1.0, Decibels::db_to_power);
        self.recompute_thresholds();
    }

    #[must_use]
    pub fn threshold_db(&self) -> f32 {
        match self.auto_margin_db {
            Some(_) => self.open_lin.max(AUTO_MIN_LIN).power_to_db(),
            None => self.threshold_db,
        }
    }

    pub fn reset(&mut self) {
        self.power = 0.0;
        self.floor = 0.0;
        self.guard_quiet = 0.0;
        if let Some(guard) = &mut self.guard {
            guard.reset();
        }
        self.warmup = self.warmup_samples;
        self.quiet = 0;
        self.open = false;
        self.recompute_thresholds();
    }

    fn recompute_thresholds(&mut self) {
        self.open_lin = match self.auto_margin_db {
            Some(_) => self.auto_open_lin(),
            None => self.threshold_db.db_to_power(),
        };
        self.close_lin = self.open_lin * self.hysteresis_lin;
    }

    fn auto_open_lin(&self) -> f32 {
        (self.floor * self.auto_margin_lin * self.guard_lift()).clamp(AUTO_MIN_LIN, AUTO_MAX_LIN)
    }

    fn guard_lift(&self) -> f32 {
        match &self.guard {
            Some(guard) if self.guard_quiet > 0.0 => (guard.level() / self.guard_quiet).max(1.0),
            _ => 1.0,
        }
    }

    #[must_use]
    pub fn process(&mut self, iq: &[Complex<f32>], wide: &[Complex<f32>]) -> bool {
        if !self.power.is_finite() || !self.floor.is_finite() || !self.guard_quiet.is_finite() {
            self.reset();
        }
        let auto = self.auto_margin_db.is_some();
        if auto && let Some(guard) = &mut self.guard {
            guard.process(wide);
        }
        for &x in iq {
            self.power += self.coeff * (x.norm_sqr() - self.power);
            if auto {
                self.track_floor();
            }
            if self.power >= self.open_lin {
                self.open = true;
                self.quiet = 0;
            } else {
                self.quiet = self.quiet.saturating_add(1);
                if self.power < self.close_lin && self.quiet >= self.hold_samples {
                    self.open = false;
                }
            }
        }
        self.open
    }

    fn track_floor(&mut self) {
        let guard = self.guard.as_ref().map_or(0.0, GuardMeter::level);
        if self.warmup > 0 || self.guard_quiet == 0.0 {
            self.guard_quiet = guard;
        }
        if self.warmup > 0 {
            self.warmup -= 1;
            self.floor = self.power;
        } else {
            self.floor = self.follow(self.floor, self.power);
            self.guard_quiet = self.follow(self.guard_quiet, guard);
        }
        self.open_lin = self.auto_open_lin();
        self.close_lin = self.open_lin * self.hysteresis_lin;
    }

    fn follow(&self, quiet: f32, now: f32) -> f32 {
        if now < quiet {
            quiet + self.floor_fall * (now - quiet)
        } else if self.open {
            quiet
        } else {
            quiet + self.floor_rise * (now - quiet)
        }
    }
}

#[cfg(test)]
mod tests;
