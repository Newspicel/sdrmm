mod heading;
mod render;
mod signal;
mod source;
mod state;
mod validate;

use num_complex::Complex;

pub use heading::HeadingTrack;
pub use signal::SceneSignal;
pub use source::{SceneCopy, SceneSource};
use state::SceneState;

use super::echo::SceneEcho;
use super::fractional::fraction_table;
use crate::array::manifold::{Direction, Geometry, ManifoldError, ManifoldTable, steer};

const MAX_TABLE_AZIMUTHS: f64 = 3600.0;

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum SceneError {
    #[error(transparent)]
    Manifold(#[from] ManifoldError),
    #[error("sample rate must be positive and finite")]
    SampleRate,
    #[error("{0} needs one value per lane or none")]
    LaneSetting(&'static str),
    #[error("source {0} copies a missing source or another copy")]
    Copy(usize),
    #[error("source {0} has a non-finite or empty setting")]
    Source(usize),
    #[error("heading must be finite")]
    Heading,
    #[error("echo {0} copies a missing source or another copy, or has a non-finite setting")]
    Echo(usize),
}

/// Gain and phase error of an element, by element index and body azimuth in degrees.
pub type Distortion = fn(usize, f64) -> Complex<f32>;

/// Multi-element receive scene for tests: sources, echoes and lane impairments rendered
/// sample by sample.
#[derive(Clone, Debug)]
pub struct ArrayScene {
    pub geometry: Geometry,
    pub center_hz: f64,
    pub sample_rate: f64,
    pub noise_db: Vec<f32>,
    pub sources: Vec<SceneSource>,
    pub lane_phase_deg: Vec<f32>,
    pub lane_gain_db: Vec<f32>,
    pub lane_delay_samples: Vec<f32>,
    pub lane_dc: Vec<Complex<f32>>,
    pub echoes: Vec<SceneEcho>,
    pub distortion: Option<Distortion>,
    pub heading: HeadingTrack,
    pub seed: u64,
    state: Option<SceneState>,
}

impl ArrayScene {
    #[must_use]
    pub const fn new(geometry: Geometry, center_hz: f64, sample_rate: f64) -> Self {
        Self {
            geometry,
            center_hz,
            sample_rate,
            noise_db: Vec::new(),
            sources: Vec::new(),
            lane_phase_deg: Vec::new(),
            lane_gain_db: Vec::new(),
            lane_delay_samples: Vec::new(),
            lane_dc: Vec::new(),
            echoes: Vec::new(),
            distortion: None,
            heading: HeadingTrack::Fixed(0.0),
            seed: 1,
            state: None,
        }
    }

    #[must_use]
    pub fn with_source(mut self, source: SceneSource) -> Self {
        self.sources.push(source);
        self
    }

    #[must_use]
    pub fn with_echo(mut self, echo: SceneEcho) -> Self {
        self.echoes.push(echo);
        self
    }

    #[must_use]
    pub fn with_noise_db(mut self, db: f32) -> Self {
        self.noise_db = vec![db; self.geometry.len()];
        self
    }

    #[must_use]
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    #[must_use]
    pub const fn with_heading(mut self, heading: HeadingTrack) -> Self {
        self.heading = heading;
        self
    }

    #[must_use]
    pub const fn lanes(&self) -> usize {
        self.geometry.len()
    }

    #[must_use]
    pub fn rendered(&self) -> i64 {
        self.state.as_ref().map_or(0, |state| state.cursor)
    }

    pub fn element_response(
        &self,
        freq_hz: f64,
        true_direction: Direction,
        t_s: f64,
        out: &mut [Complex<f32>],
    ) {
        let body = Direction::new(
            true_direction.azimuth_deg - self.heading.at(t_s),
            true_direction.elevation_deg,
        );
        steer(self.geometry.positions(), freq_hz, body, out);
        if let Some(distortion) = self.distortion {
            for (element, value) in out.iter_mut().enumerate().take(self.lanes()) {
                *value *= distortion(element, body.azimuth_deg);
            }
        }
    }

    pub fn render(&mut self, len: usize) -> Result<Vec<Vec<Complex<f32>>>, SceneError> {
        self.validate()?;
        let mut state = match self.state.take() {
            Some(state) if state.lanes.len() == self.lanes() => state,
            _ => SceneState::new(self.lanes(), self.seed),
        };
        state.sync_waves(&self.sources, self.seed, self.sample_rate);
        if !self.echoes.is_empty() && state.fractions.is_empty() {
            state.fractions = fraction_table();
        }
        let result = self.render_with(&mut state, len);
        self.state = Some(state);
        Ok(result)
    }

    pub fn distortion_table(
        &self,
        freqs_hz: &[f64],
        step_deg: f64,
    ) -> Result<ManifoldTable, SceneError> {
        let n = self.lanes();
        let azimuths = (360.0 / step_deg).round();
        if !(1.0..=MAX_TABLE_AZIMUTHS).contains(&azimuths) {
            return Err(ManifoldError::Table.into());
        }
        let azimuths = azimuths as usize;
        let mut data = vec![Complex::new(0.0f32, 0.0); freqs_hz.len() * azimuths * n];
        let fixed = Self {
            heading: HeadingTrack::Fixed(0.0),
            state: None,
            ..self.clone()
        };
        for (f, &freq) in freqs_hz.iter().enumerate() {
            for a in 0..azimuths {
                let at = (f * azimuths + a) * n;
                let direction = Direction::horizon(a as f64 * step_deg);
                fixed.element_response(freq, direction, 0.0, &mut data[at..at + n]);
            }
        }
        Ok(ManifoldTable::new(
            n,
            freqs_hz.to_vec(),
            step_deg,
            vec![0.0],
            data,
        )?)
    }
}

#[cfg(test)]
mod tests;
