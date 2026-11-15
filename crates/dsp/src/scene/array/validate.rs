use super::{ArrayScene, SceneError};
use crate::array::manifold::ManifoldError;

impl ArrayScene {
    pub(super) fn validate(&self) -> Result<(), SceneError> {
        if !(self.sample_rate.is_finite() && self.sample_rate > 0.0) {
            return Err(SceneError::SampleRate);
        }
        if !(self.center_hz.is_finite() && self.center_hz > 0.0) {
            return Err(ManifoldError::Frequency.into());
        }
        let lanes = self.lanes();
        let fits = |len: usize| len == 0 || len == lanes;
        for (name, len) in [
            ("noise_db", self.noise_db.len()),
            ("lane_phase_deg", self.lane_phase_deg.len()),
            ("lane_gain_db", self.lane_gain_db.len()),
            ("lane_delay_samples", self.lane_delay_samples.len()),
            ("lane_dc", self.lane_dc.len()),
        ] {
            if !fits(len) {
                return Err(SceneError::LaneSetting(name));
            }
        }
        for (name, values) in [
            ("noise_db", &self.noise_db),
            ("lane_phase_deg", &self.lane_phase_deg),
            ("lane_gain_db", &self.lane_gain_db),
            ("lane_delay_samples", &self.lane_delay_samples),
        ] {
            if !values.iter().all(|value| value.is_finite()) {
                return Err(SceneError::LaneSetting(name));
            }
        }
        if !self.lane_dc.iter().all(|dc| dc.is_finite()) {
            return Err(SceneError::LaneSetting("lane_dc"));
        }
        if !self.heading.is_finite() {
            return Err(SceneError::Heading);
        }
        for (index, source) in self.sources.iter().enumerate() {
            let sound = source.delay_samples.is_finite()
                && source.power_db.is_finite()
                && source.signal.is_valid()
                && source.direction.azimuth_deg.is_finite()
                && source.direction.elevation_deg.is_finite();
            if !sound {
                return Err(SceneError::Source(index));
            }
            if let Some(copy) = source.copy_of {
                let original = self.sources.get(copy.source);
                if copy.source == index || original.is_none_or(|o| o.copy_of.is_some()) {
                    return Err(SceneError::Copy(index));
                }
            }
        }
        for (index, echo) in self.echoes.iter().enumerate() {
            let original = self.sources.get(echo.source);
            if !echo.is_valid() || original.is_none_or(|o| o.copy_of.is_some()) {
                return Err(SceneError::Echo(index));
            }
        }
        Ok(())
    }
}
