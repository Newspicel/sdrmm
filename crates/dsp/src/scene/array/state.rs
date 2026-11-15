use super::SceneSource;
use crate::math::rng::Rng;
use crate::scene::history::History;
use crate::scene::wave::Wave;

const LANE_PRE_ROLL: i64 = 256;
const NOISE_SALT: u64 = 0xD1B5_4A32_D192_ED03;

/// What a scene carries from one render call to the next.
#[derive(Clone, Debug)]
pub(super) struct SceneState {
    pub(super) cursor: i64,
    pub(super) waves: Vec<Wave>,
    pub(super) lanes: Vec<History>,
    pub(super) noise: Vec<Rng>,
    pub(super) fractions: Vec<Vec<f64>>,
}

impl SceneState {
    pub(super) fn new(lanes: usize, seed: u64) -> Self {
        Self {
            cursor: 0,
            waves: Vec::new(),
            lanes: vec![History::starting_at(-LANE_PRE_ROLL); lanes],
            noise: (0..lanes)
                .map(|lane| Rng::streamed(seed ^ NOISE_SALT, lane as u64))
                .collect(),
            fractions: Vec::new(),
        }
    }

    pub(super) fn sync_waves(&mut self, sources: &[SceneSource], seed: u64, sample_rate: f64) {
        self.waves.truncate(sources.len());
        for (index, source) in sources.iter().enumerate() {
            let stale = self
                .waves
                .get(index)
                .is_none_or(|wave| wave.signal != source.signal);
            if stale {
                let wave = Wave::new(
                    source.signal,
                    Rng::streamed(seed, index as u64),
                    sample_rate,
                );
                if index < self.waves.len() {
                    self.waves[index] = wave;
                } else {
                    self.waves.push(wave);
                }
            }
        }
    }

    pub(super) fn trim(&mut self, keep_from: i64) {
        for wave in &mut self.waves {
            wave.history.trim(keep_from);
        }
        for lane in &mut self.lanes {
            lane.trim(keep_from);
        }
    }
}
