use num_complex::Complex;

use super::{
    echo::SceneEcho,
    rng::Rng,
    wave::{DELAY_CENTRE, DelayPlan, History, Wave, fraction_table},
};
use crate::manifold::{
    Direction, Geometry, MAX_ELEMENTS, ManifoldError, ManifoldTable, steer, widen,
};

const LANE_PRE_ROLL: i64 = 256;
const KEEP_SAMPLES: i64 = 4096;
const NOISE_SALT: u64 = 0xD1B5_4A32_D192_ED03;
const MAX_TABLE_AZIMUTHS: f64 = 3600.0;

type C64 = Complex<f64>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SceneSignal {
    Tone {
        offset_hz: f64,
    },
    Noise {
        offset_hz: f64,
        bandwidth_hz: f64,
    },
    Fm {
        offset_hz: f64,
        deviation_hz: f64,
        rate_hz: f64,
    },
    NoiseFm {
        offset_hz: f64,
        deviation_hz: f64,
        bandwidth_hz: f64,
    },
    Broadband,
}

impl SceneSignal {
    #[must_use]
    pub const fn offset_hz(self) -> f64 {
        match self {
            Self::Tone { offset_hz }
            | Self::Noise { offset_hz, .. }
            | Self::Fm { offset_hz, .. }
            | Self::NoiseFm { offset_hz, .. } => offset_hz,
            Self::Broadband => 0.0,
        }
    }

    pub(super) const fn is_random(self) -> bool {
        matches!(
            self,
            Self::Noise { .. } | Self::NoiseFm { .. } | Self::Broadband
        )
    }

    fn is_valid(self) -> bool {
        match self {
            Self::Tone { offset_hz } => offset_hz.is_finite(),
            Self::Noise {
                offset_hz,
                bandwidth_hz,
            } => offset_hz.is_finite() && bandwidth_hz.is_finite() && bandwidth_hz > 0.0,
            Self::Fm {
                offset_hz,
                deviation_hz,
                rate_hz,
            } => offset_hz.is_finite() && deviation_hz.is_finite() && rate_hz.is_finite(),
            Self::NoiseFm {
                offset_hz,
                deviation_hz,
                bandwidth_hz,
            } => {
                offset_hz.is_finite()
                    && deviation_hz.is_finite()
                    && bandwidth_hz.is_finite()
                    && bandwidth_hz > 0.0
            }
            Self::Broadband => true,
        }
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeadingTrack {
    Fixed(f64),
    Rotating { start_deg: f64, rate_dps: f64 },
}

impl HeadingTrack {
    #[must_use]
    pub fn at(self, t_s: f64) -> f64 {
        match self {
            Self::Fixed(heading) => heading,
            Self::Rotating {
                start_deg,
                rate_dps,
            } => start_deg + rate_dps * t_s,
        }
    }

    const fn is_finite(self) -> bool {
        match self {
            Self::Fixed(heading) => heading.is_finite(),
            Self::Rotating {
                start_deg,
                rate_dps,
            } => start_deg.is_finite() && rate_dps.is_finite(),
        }
    }
}

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

pub type Distortion = fn(usize, f64) -> Complex<f32>;

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

    fn validate(&self) -> Result<(), SceneError> {
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

    fn render_with(&self, state: &mut SceneState, len: usize) -> Vec<Vec<Complex<f32>>> {
        let lanes = self.lanes();
        let start = state.cursor;
        let end = start + len as i64;
        let source_plans: Vec<DelayPlan> = self
            .sources
            .iter()
            .map(|source| DelayPlan::new(f64::from(source.delay_samples)))
            .collect();
        let lane_plans: Vec<DelayPlan> = (0..lanes)
            .map(|lane| DelayPlan::new(lane_value(&self.lane_delay_samples, lane).into()))
            .collect();
        let needed = lane_plans
            .iter()
            .map(|plan| plan.reach(end - 1))
            .max()
            .unwrap_or(end - 1);
        self.fill_lanes(state, &source_plans, needed + 1);
        let mut out = vec![vec![Complex::new(0.0f32, 0.0); len]; lanes];
        for (lane, samples) in out.iter_mut().enumerate() {
            let gain = 10f64.powf(f64::from(lane_value(&self.lane_gain_db, lane)) / 20.0);
            let turn = C64::from_polar(
                gain,
                f64::from(lane_value(&self.lane_phase_deg, lane)).to_radians(),
            );
            let dc = widen(self.lane_dc.get(lane).copied().unwrap_or_default());
            let sigma = self
                .noise_db
                .get(lane)
                .map_or(0.0, |&db| 10f64.powf(f64::from(db) / 20.0));
            for (index, value) in samples.iter_mut().enumerate() {
                let n = start + index as i64;
                let signal = lane_plans[lane].read(&state.lanes[lane], n) * turn;
                let noise = state.noise[lane].complex_normal() * sigma;
                let total = signal + noise + dc;
                *value = Complex::new(total.re as f32, total.im as f32);
            }
        }
        state.cursor = end;
        let longest = source_plans
            .iter()
            .chain(&lane_plans)
            .map(|plan| plan.whole.abs())
            .chain(self.echoes.iter().map(|echo| {
                let delay = self.echo_delay(echo, end).abs().ceil() as i64;
                delay + DELAY_CENTRE
            }))
            .max()
            .unwrap_or(0);
        state.trim(end - KEEP_SAMPLES - longest);
        out
    }

    fn echo_carrier(&self, echo: &SceneEcho) -> f64 {
        self.center_hz
            + self
                .sources
                .get(echo.source)
                .map_or(0.0, |source| source.signal.offset_hz())
    }

    fn echo_delay(&self, echo: &SceneEcho, n: i64) -> f64 {
        let direct = self
            .sources
            .get(echo.source)
            .map_or(0.0, |source| f64::from(source.delay_samples));
        direct + echo.delay_at(n, self.echo_carrier(echo))
    }

    fn echo_value(&self, state: &SceneState, echo: &SceneEcho, n: i64) -> C64 {
        let source = &self.sources[echo.source];
        let delay = self.echo_delay(echo, n);
        state.waves[echo.source].at(n, delay, self.sample_rate, &state.fractions)
            * amplitude(source)
            * echo.rotation(n, self.sample_rate)
    }

    fn ensure_echo_history(&self, state: &mut SceneState, from: i64, upto: i64) {
        for echo in &self.echoes {
            let reach = [from, upto - 1]
                .iter()
                .map(|&n| n - self.echo_delay(echo, n).floor() as i64 + DELAY_CENTRE + 1)
                .max()
                .unwrap_or(upto);
            state.waves[echo.source].ensure(reach, self.sample_rate);
        }
    }

    fn fill_lanes(&self, state: &mut SceneState, plans: &[DelayPlan], upto: i64) {
        let from = state.lanes[0].end();
        if from >= upto {
            return;
        }
        for (index, source) in self.sources.iter().enumerate() {
            let wave = source.copy_of.map_or(index, |copy| copy.source);
            state.waves[wave].ensure(plans[index].reach(upto - 1), self.sample_rate);
        }
        self.ensure_echo_history(state, from, upto);
        let lanes = self.lanes();
        let rotating = matches!(self.heading, HeadingTrack::Rotating { .. });
        let mut responses = vec![[Complex::new(0.0f32, 0.0); MAX_ELEMENTS]; self.sources.len()];
        let mut echo_responses = vec![[Complex::new(0.0f32, 0.0); MAX_ELEMENTS]; self.echoes.len()];
        let fs = self.sample_rate;
        for n in from..upto {
            let t_s = n as f64 / fs;
            if rotating || n == from {
                for (response, source) in responses.iter_mut().zip(&self.sources) {
                    let freq = self.center_hz + self.source_signal(source).offset_hz();
                    self.element_response(freq, source.direction, t_s, &mut response[..lanes]);
                }
                for (response, echo) in echo_responses.iter_mut().zip(&self.echoes) {
                    let freq = self.echo_carrier(echo);
                    self.element_response(freq, echo.direction, t_s, &mut response[..lanes]);
                }
            }
            let mut sums = [C64::new(0.0, 0.0); MAX_ELEMENTS];
            for ((index, source), response) in self.sources.iter().enumerate().zip(&responses) {
                let value = self.source_value(state, plans, index, source, n);
                for (sum, element) in sums.iter_mut().zip(&response[..lanes]) {
                    *sum += widen(*element) * value;
                }
            }
            for (echo, response) in self.echoes.iter().zip(&echo_responses) {
                let value = self.echo_value(state, echo, n);
                for (sum, element) in sums.iter_mut().zip(&response[..lanes]) {
                    *sum += widen(*element) * value;
                }
            }
            for (history, sum) in state.lanes.iter_mut().zip(&sums) {
                history.data.push(*sum);
            }
        }
    }

    fn source_signal(&self, source: &SceneSource) -> SceneSignal {
        source
            .copy_of
            .and_then(|copy| self.sources.get(copy.source))
            .map_or(source.signal, |original| original.signal)
    }

    fn source_value(
        &self,
        state: &SceneState,
        plans: &[DelayPlan],
        index: usize,
        source: &SceneSource,
        n: i64,
    ) -> C64 {
        match source.copy_of {
            None => {
                state.waves[index].value(n, &plans[index], self.sample_rate) * amplitude(source)
            }
            Some(copy) => {
                let original = &self.sources[copy.source];
                let echo = C64::from_polar(
                    f64::from(copy.amplitude),
                    f64::from(copy.phase_deg).to_radians(),
                );
                state.waves[copy.source].value(n, &plans[index], self.sample_rate)
                    * amplitude(original)
                    * echo
            }
        }
    }
}

fn amplitude(source: &SceneSource) -> f64 {
    10f64.powf(f64::from(source.power_db) / 20.0)
}

fn lane_value(values: &[f32], lane: usize) -> f32 {
    values.get(lane).copied().unwrap_or(0.0)
}

#[derive(Clone, Debug)]
struct SceneState {
    cursor: i64,
    waves: Vec<Wave>,
    lanes: Vec<History>,
    noise: Vec<Rng>,
    fractions: Vec<Vec<f64>>,
}

impl SceneState {
    fn new(lanes: usize, seed: u64) -> Self {
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

    fn sync_waves(&mut self, sources: &[SceneSource], seed: u64, sample_rate: f64) {
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

    fn trim(&mut self, keep_from: i64) {
        for wave in &mut self.waves {
            wave.history.trim(keep_from);
        }
        for lane in &mut self.lanes {
            lane.trim(keep_from);
        }
    }
}

#[cfg(test)]
mod tests;
