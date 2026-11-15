use super::{
    Emitter, LaneImpairments, MAX_CLUTTER_DELAY_SAMPLES, MAX_CLUTTER_ECHOES, MAX_PPM,
    MAX_RIPPLE_CYCLES, MAX_RIPPLE_DB, Scene, Waveform,
};

#[must_use]
pub fn all_finite(values: &[f64]) -> bool {
    values.iter().all(|value| value.is_finite())
}

impl Scene {
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        if !self.positions.iter().all(|p| all_finite(p)) {
            return Some("element positions must be finite".to_owned());
        }
        if !all_finite(&[self.thermal_dbfs, self.noise_source_dbfs])
            || !positive(self.noise_bandwidth)
        {
            return Some("noise levels and the noise bandwidth must be finite".to_owned());
        }
        for (index, emitter) in self.emitters.iter().enumerate() {
            if let Some(problem) = emitter.problem() {
                return Some(format!("emitter {index}: {problem}"));
            }
        }
        for (index, echo) in self.echoes.iter().enumerate() {
            if echo.emitter >= self.emitters.len() {
                return Some(format!("echo {index} names a missing emitter"));
            }
            if !all_finite(&[
                echo.azimuth_deg,
                echo.delay_s,
                echo.doppler_hz,
                echo.gain_db,
            ]) {
                return Some(format!("echo {index} must be finite"));
            }
        }
        self.clutter_problem()
    }

    fn clutter_problem(&self) -> Option<String> {
        let clutter = self.clutter;
        if clutter.echoes == 0 {
            return None;
        }
        if self.emitters.is_empty() {
            return Some("clutter needs an emitter to reflect".to_owned());
        }
        let sound = clutter.echoes <= MAX_CLUTTER_ECHOES
            && (1..=MAX_CLUTTER_DELAY_SAMPLES).contains(&clutter.max_delay_samples)
            && clutter.gain_db.is_finite();
        (!sound).then(|| {
            format!(
                "clutter takes at most {MAX_CLUTTER_ECHOES} echoes within 1 to \
                 {MAX_CLUTTER_DELAY_SAMPLES} samples"
            )
        })
    }
}

impl Emitter {
    fn problem(&self) -> Option<&'static str> {
        if !all_finite(&[
            self.azimuth_deg,
            self.elevation_deg,
            self.offset_hz,
            self.power_dbfs,
        ]) {
            return Some("direction, offset and power must be finite");
        }
        let paths_sound = self.paths.iter().all(|path| {
            all_finite(&[
                path.azimuth_deg,
                path.elevation_deg,
                path.delay_s,
                path.gain_db,
                path.phase_deg,
            ])
        });
        if !paths_sound {
            return Some("every path must be finite");
        }
        self.waveform.problem()
    }
}

impl LaneImpairments {
    #[must_use]
    pub fn problem(&self) -> Option<&'static str> {
        let sound = all_finite(&[
            self.ppm,
            self.gain_db,
            self.phase_deg,
            self.phase_per_db,
            self.frac_delay,
            self.dc_phase_deg,
        ]) && self.dc_dbfs.is_none_or(f64::is_finite);
        if !sound {
            return Some("lane impairments must be finite");
        }
        if self.ripple.is_some_and(|ripple| {
            !(0.0..MAX_RIPPLE_DB).contains(&ripple.depth_db) || ripple.cycles > MAX_RIPPLE_CYCLES
        }) {
            return Some("a lane ripple is 0 to 6 dB deep with at most 64 cycles");
        }
        (self.ppm.abs() > MAX_PPM).then_some("a lane clock is off by at most 1000 ppm")
    }
}

impl Waveform {
    fn problem(self) -> Option<&'static str> {
        let sound = match self {
            Self::Tone => true,
            Self::Fm {
                deviation_hz,
                rate_hz,
            } => deviation_hz.is_finite() && deviation_hz >= 0.0 && positive(rate_hz),
            Self::Am { depth, rate_hz } => (0.0..=1.0).contains(&depth) && positive(rate_hz),
            Self::Noise { bandwidth_hz } => positive(bandwidth_hz),
            Self::Bpsk { symbol_rate } => positive(symbol_rate),
        };
        (!sound).then_some("an emitter waveform needs finite, positive rates")
    }
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}
