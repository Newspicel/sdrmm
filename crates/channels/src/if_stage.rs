use num_complex::Complex;
use sdrmm_dsp::{FirC, IqNotch, fir::design_lowpass_kaiser, fir::kaiser_taps};
use sdrmm_wire::{IfFilter, IfNotch};

use crate::ChannelError;

const EDGE_TRANSITION: f64 = 0.03;
const EDGE_ATTENUATION_DB: f64 = 60.0;
const EDGE_MARGIN: f64 = 0.5 - EDGE_TRANSITION / 2.0;

/// The operator's IF shaping, run on a channel's baseband between the down-converter and the
/// decoder's own filter.
pub struct IfStage {
    rate: f64,
    edges: Option<FirC>,
    notches: Vec<TunedNotch>,
    shaped: Vec<Complex<f32>>,
}

struct TunedNotch {
    frequency_hz: f64,
    filter: IqNotch,
    heard: bool,
}

impl IfStage {
    /// `None` when the filter would leave the signal untouched, so an idle stage costs nothing.
    pub fn build(
        filter: &IfFilter,
        rate: f64,
        tuned_hz: f64,
        block: usize,
    ) -> Result<Option<Self>, ChannelError> {
        if !filter.is_active() {
            return Ok(None);
        }
        filter.validate().map_err(ChannelError::InvalidSettings)?;
        let mut stage = Self {
            rate,
            edges: edges(filter, rate)?,
            notches: filter
                .notches
                .iter()
                .map(|notch| TunedNotch::new(notch, rate))
                .collect(),
            shaped: Vec::with_capacity(block),
        };
        stage.retune(tuned_hz);
        Ok(Some(stage))
    }

    /// Follows the frequency the channel's baseband is centred on. Notches keep their radio
    /// frequency; one the baseband no longer holds rests until a retune brings it back.
    pub fn retune(&mut self, tuned_hz: f64) {
        for notch in &mut self.notches {
            notch.follow(tuned_hz, self.rate);
        }
    }

    pub fn reset(&mut self) {
        if let Some(edges) = &mut self.edges {
            edges.reset();
        }
        for notch in &mut self.notches {
            notch.filter.reset();
        }
    }

    pub fn process(&mut self, input: &[Complex<f32>]) -> &[Complex<f32>] {
        match &mut self.edges {
            Some(edges) => edges.process(input, &mut self.shaped),
            None => {
                self.shaped.clear();
                self.shaped.extend_from_slice(input);
            }
        }
        for notch in self.notches.iter_mut().filter(|notch| notch.heard) {
            notch.filter.process(&mut self.shaped);
        }
        &self.shaped
    }
}

impl TunedNotch {
    fn new(notch: &IfNotch, rate: f64) -> Self {
        Self {
            frequency_hz: notch.frequency_hz,
            filter: IqNotch::new(rate, 0.0, notch.width_hz),
            heard: false,
        }
    }

    fn follow(&mut self, tuned_hz: f64, rate: f64) {
        let offset_hz = self.frequency_hz - tuned_hz;
        let heard = offset_hz.abs() < rate / 2.0;
        if heard {
            self.filter.retune(offset_hz);
        }
        if heard != self.heard {
            self.filter.reset();
        }
        self.heard = heard;
    }
}

fn edges(filter: &IfFilter, rate: f64) -> Result<Option<FirC>, ChannelError> {
    if filter.low_hz.is_none() && filter.high_hz.is_none() {
        return Ok(None);
    }
    let low = filter.low_hz.map_or(-EDGE_MARGIN, |hz| hz / rate);
    let high = filter.high_hz.map_or(EDGE_MARGIN, |hz| hz / rate);
    let half = (high - low) / 2.0;
    if !(low >= -EDGE_MARGIN && high <= EDGE_MARGIN && half > EDGE_TRANSITION / 2.0) {
        return Err(ChannelError::InvalidSettings(format!(
            "IF edges {low_hz}..{high_hz} Hz do not fit this decoder's {rate} Hz baseband",
            low_hz = low * rate,
            high_hz = high * rate,
        )));
    }
    let taps = kaiser_taps(EDGE_TRANSITION, EDGE_ATTENUATION_DB);
    let prototype = design_lowpass_kaiser(taps, half, EDGE_ATTENUATION_DB);
    Ok(Some(FirC::from_lowpass(&prototype, (high + low) / 2.0)))
}

#[cfg(test)]
mod offair;

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;

    const RATE: f64 = 240_000.0;
    const TUNED_HZ: f64 = 99_900_000.0;
    const BLOCK: usize = 4_096;

    fn tone(offset_hz: f64, len: usize) -> Vec<Complex<f32>> {
        (0..len)
            .map(|k| {
                let phase = TAU * offset_hz * k as f64 / RATE;
                Complex::new(phase.cos() as f32, phase.sin() as f32)
            })
            .collect()
    }

    fn kept(stage: &mut IfStage, offset_hz: f64) -> f32 {
        stage.reset();
        let input = tone(offset_hz, 48_000);
        let mut out = Vec::new();
        for block in input.chunks(BLOCK) {
            out.extend_from_slice(stage.process(block));
        }
        let settled = &out[12_000..];
        (settled.iter().map(|s| s.norm_sqr()).sum::<f32>() / settled.len() as f32).sqrt()
    }

    fn stage(filter: IfFilter) -> IfStage {
        IfStage::build(&filter, RATE, TUNED_HZ, BLOCK)
            .unwrap()
            .expect("an active filter")
    }

    fn enabled() -> IfFilter {
        IfFilter {
            enabled: true,
            ..IfFilter::default()
        }
    }

    #[test]
    fn an_idle_filter_builds_no_stage() {
        assert!(
            IfStage::build(&IfFilter::default(), RATE, TUNED_HZ, BLOCK)
                .unwrap()
                .is_none()
        );
        assert!(
            IfStage::build(&enabled(), RATE, TUNED_HZ, BLOCK)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn edges_cut_one_side_and_keep_the_other() {
        let mut stage = stage(IfFilter {
            low_hz: Some(-90_000.0),
            high_hz: Some(30_000.0),
            ..enabled()
        });
        for passed in [-80_000.0, -20_000.0, 0.0, 20_000.0] {
            let gain = kept(&mut stage, passed);
            assert!((0.97..1.03).contains(&gain), "{passed} Hz kept {gain}");
        }
        for cut in [45_000.0, 80_000.0, 110_000.0] {
            let gain = kept(&mut stage, cut);
            assert!(gain < 0.002, "{cut} Hz leaked {gain}");
        }
    }

    #[test]
    fn a_single_edge_leaves_the_other_side_at_the_decoder() {
        let mut stage = stage(IfFilter {
            high_hz: Some(30_000.0),
            ..enabled()
        });
        assert!(kept(&mut stage, -100_000.0) > 0.97);
        assert!(kept(&mut stage, 60_000.0) < 0.002);
    }

    #[test]
    fn edges_beyond_the_baseband_are_refused() {
        let wide = IfFilter {
            high_hz: Some(200_000.0),
            ..enabled()
        };
        assert!(IfStage::build(&wide, RATE, TUNED_HZ, BLOCK).is_err());
    }

    #[test]
    fn a_notch_stays_on_its_radio_frequency_across_a_retune() {
        let interferer_hz = TUNED_HZ + 30_000.0;
        let mut stage = stage(IfFilter {
            notches: vec![IfNotch {
                frequency_hz: interferer_hz,
                width_hz: 1_000.0,
            }],
            ..enabled()
        });
        assert!(kept(&mut stage, 30_000.0) < 0.01);
        stage.retune(TUNED_HZ + 50_000.0);
        assert!(kept(&mut stage, -20_000.0) < 0.01);
        assert!(kept(&mut stage, 30_000.0) > 0.97);
    }

    #[test]
    fn a_notch_outside_the_baseband_rests_until_it_comes_back() {
        let far_hz = TUNED_HZ + 500_000.0;
        let mut stage = stage(IfFilter {
            notches: vec![IfNotch {
                frequency_hz: far_hz,
                width_hz: 1_000.0,
            }],
            ..enabled()
        });
        assert!(kept(&mut stage, 10_000.0) > 0.97);
        stage.retune(far_hz - 10_000.0);
        assert!(kept(&mut stage, 10_000.0) < 0.01);
    }
}
