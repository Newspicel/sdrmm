use num_complex::Complex;

use super::{stage::SymbolStage, unstagger::Unstagger};
use crate::linear::{
    LinearParams,
    acquire::{Chirp, FrequencyAcquisition},
    carrier::CarrierLoop,
    equaliser::{Equaliser, EqualiserConfig, EqualiserError, TAPS_PER_SYMBOL},
    frontend::{FrontCorrection, FrontEstimator},
    rotation::derotate,
    timing::{FeedforwardTiming, TimingMetric},
};

/// Demodulates a whole burst at once: front-end correction, feedforward timing, frequency
/// acquisition, then a forward and backward carrier pass.
pub struct LinearBurstDemod {
    front: FrontEstimator,
    correction: FrontCorrection,
    corrected: Vec<Complex<f32>>,
    timing: FeedforwardTiming,
    acquisition: FrequencyAcquisition,
    chirp: Chirp,
    backward: Vec<f64>,
    stage: SymbolStage,
    stagger: Unstagger,
    aligned: Vec<Complex<f32>>,
    retimed: Vec<Complex<f32>>,
    on_time: Vec<Complex<f32>>,
    equalised: Vec<Complex<f32>>,
}

impl LinearBurstDemod {
    #[must_use]
    pub fn new(
        params: &LinearParams,
        receive_filter: &[f32],
        power_symbols: f64,
        carrier: Option<CarrierLoop>,
    ) -> Self {
        Self {
            front: FrontEstimator::for_params(params),
            correction: FrontCorrection::default(),
            corrected: Vec::new(),
            timing: FeedforwardTiming::new(params, receive_filter),
            acquisition: FrequencyAcquisition::new(),
            chirp: Chirp::default(),
            backward: Vec::new(),
            stage: SymbolStage::new(params, power_symbols, carrier),
            stagger: Unstagger::new(params.stagger_samples()),
            aligned: Vec::new(),
            retimed: Vec::new(),
            on_time: Vec::new(),
            equalised: Vec::new(),
        }
    }

    pub fn with_equaliser(mut self, config: EqualiserConfig) -> Result<Self, EqualiserError> {
        self.stage.equalise(config)?;
        Ok(self)
    }

    #[must_use]
    pub fn equaliser(&self) -> Option<&Equaliser> {
        self.stage.equaliser.as_ref()
    }

    #[must_use]
    pub fn equaliser_resets(&self) -> u64 {
        self.stage.equaliser_resets()
    }

    #[must_use]
    pub fn params(&self) -> &LinearParams {
        &self.stage.params
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Complex<f32>>) -> f64 {
        self.correction = self.front.estimate(iq);
        self.corrected.clear();
        self.corrected.extend_from_slice(iq);
        self.correction.apply(&mut self.corrected);
        self.stagger.apply(&self.corrected, &mut self.aligned);
        self.retimed.clear();
        if self.stage.equaliser.is_some() {
            return self.process_equalised(out);
        }
        let offset = self.timing.process(&self.aligned, &mut self.retimed);
        for y in &mut self.retimed {
            *y = self.stage.prepare(*y);
        }
        self.chirp = self
            .stage
            .acquisition_order()
            .and_then(|order| self.acquisition.estimate(&self.retimed, order))
            .unwrap_or_default();
        self.chirp.remove(&mut self.retimed);
        if let Some(carrier) = &mut self.stage.carrier {
            carrier.smooth(
                &mut self.retimed,
                self.stage.params.constellation(),
                &mut self.backward,
            );
        }
        out.extend_from_slice(&self.retimed);
        offset
    }

    fn process_equalised(&mut self, out: &mut Vec<Complex<f32>>) -> f64 {
        let offset = self
            .timing
            .process_spaced(&self.aligned, TAPS_PER_SYMBOL, &mut self.retimed);
        let rotation = self.stage.params.rotation_rad();
        for (i, y) in self.retimed.iter_mut().enumerate() {
            *y = derotate(*y, i as f64 / TAPS_PER_SYMBOL as f64, rotation);
        }
        self.on_time.clear();
        self.on_time
            .extend(self.retimed.iter().step_by(TAPS_PER_SYMBOL));
        self.chirp = self
            .stage
            .acquisition_order()
            .and_then(|order| self.acquisition.estimate(&self.on_time, order))
            .unwrap_or_default();
        self.chirp.remove_spaced(&mut self.retimed, TAPS_PER_SYMBOL);
        self.equalised.clear();
        let table = self.stage.params.constellation();
        if let Some(equaliser) = &mut self.stage.equaliser {
            equaliser.equalise_burst(
                &self.retimed,
                self.stage.carrier.as_ref(),
                table,
                &mut self.equalised,
            );
        }
        if let Some(carrier) = &mut self.stage.carrier {
            carrier.smooth(&mut self.equalised, table, &mut self.backward);
        }
        out.extend_from_slice(&self.equalised);
        offset
    }

    #[must_use]
    pub fn carrier_freq_cycles_per_symbol(&self) -> f64 {
        self.chirp.centre_cycles_per_symbol
            + self
                .stage
                .carrier
                .as_ref()
                .map_or(0.0, CarrierLoop::freq_cycles_per_symbol)
    }

    #[must_use]
    pub fn with_timing_metric(mut self, metric: TimingMetric) -> Self {
        self.timing = self.timing.with_metric(metric);
        self
    }

    #[must_use]
    pub fn front_correction(&self) -> FrontCorrection {
        self.correction
    }

    #[must_use]
    pub fn chirp(&self) -> Chirp {
        self.chirp
    }
}
