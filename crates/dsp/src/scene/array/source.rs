use super::SceneSignal;
use crate::array::manifold::Direction;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneCopy {
    pub source: usize,
    pub amplitude: f32,
    pub phase_deg: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneSource {
    pub direction: Direction,
    pub power_db: f32,
    pub signal: SceneSignal,
    pub copy_of: Option<SceneCopy>,
    pub delay_samples: f32,
}

impl SceneSource {
    #[must_use]
    pub const fn new(direction: Direction, power_db: f32, signal: SceneSignal) -> Self {
        Self {
            direction,
            power_db,
            signal,
            copy_of: None,
            delay_samples: 0.0,
        }
    }
}
