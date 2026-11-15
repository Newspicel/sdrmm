use num_complex::Complex;

use super::state::SceneState;
use super::{ArrayScene, HeadingTrack, SceneEcho, SceneSignal, SceneSource};
use crate::array::manifold::MAX_ELEMENTS;
use crate::math::complex::{narrow, widen};
use crate::math::db::Decibels;
use crate::scene::fractional::{DELAY_CENTRE, DelayPlan};

const KEEP_SAMPLES: i64 = 4096;

type C64 = Complex<f64>;

impl ArrayScene {
    pub(super) fn render_with(&self, state: &mut SceneState, len: usize) -> Vec<Vec<Complex<f32>>> {
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
            let gain = f64::from(lane_value(&self.lane_gain_db, lane)).db_to_amplitude();
            let turn = C64::from_polar(
                gain,
                f64::from(lane_value(&self.lane_phase_deg, lane)).to_radians(),
            );
            let dc = widen(self.lane_dc.get(lane).copied().unwrap_or_default());
            let sigma = self
                .noise_db
                .get(lane)
                .map_or(0.0, |&db| f64::from(db).db_to_amplitude());
            for (index, value) in samples.iter_mut().enumerate() {
                let n = start + index as i64;
                let signal = lane_plans[lane].read(&state.lanes[lane], n) * turn;
                let noise = state.noise[lane].complex_normal() * sigma;
                *value = narrow(signal + noise + dc);
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
    f64::from(source.power_db).db_to_amplitude()
}

fn lane_value(values: &[f32], lane: usize) -> f32 {
    values.get(lane).copied().unwrap_or(0.0)
}
