use num_complex::Complex;

use super::decision_timing::DecisionTiming;
use crate::linear::{
    LinearParams,
    carrier::CarrierLoop,
    equaliser::{Equaliser, EqualiserConfig, EqualiserError},
    rotation::derotate,
};

const INITIAL_POWER: f32 = 1.0;

/// What both demodulators do once a symbol is timed: level it, undo the rotation schedule,
/// equalise and track the carrier.
pub(super) struct SymbolStage {
    pub(super) carrier: Option<CarrierLoop>,
    pub(super) equaliser: Option<Equaliser>,
    pub(super) params: LinearParams,
    power: f32,
    power_alpha: f32,
    symbols_out: u64,
    timing: DecisionTiming,
}

impl SymbolStage {
    pub(super) fn new(
        params: &LinearParams,
        power_symbols: f64,
        carrier: Option<CarrierLoop>,
    ) -> Self {
        Self {
            carrier,
            equaliser: None,
            params: params.clone(),
            power: INITIAL_POWER,
            power_alpha: if power_symbols.is_finite() {
                (1.0 / power_symbols) as f32
            } else {
                0.0
            },
            symbols_out: 0,
            timing: DecisionTiming::new(params),
        }
    }

    pub(super) fn push_steering(&mut self, y: Complex<f32>) -> (Complex<f32>, Option<f64>) {
        let z = self.push(y);
        if self.carrier.is_none() {
            return (z, None);
        }
        let decision = self.params.constellation().nearest(z);
        (z, self.timing.error(z, decision))
    }

    pub(super) fn prepare(&mut self, y: Complex<f32>) -> Complex<f32> {
        self.power += self.power_alpha * (y.norm_sqr() - self.power);
        let scale = self.power.max(f32::MIN_POSITIVE).sqrt().recip();
        let symbol = derotate(
            y * scale,
            self.symbols_out as f64,
            self.params.rotation_rad(),
        );
        self.symbols_out += 1;
        symbol
    }

    pub(super) fn push_spaced(
        &mut self,
        y: Complex<f32>,
        at_symbol: bool,
    ) -> Option<(Complex<f32>, Option<f64>)> {
        let Some(equaliser) = &mut self.equaliser else {
            return at_symbol.then(|| self.push_steering(y));
        };
        let rotation = self.params.rotation_rad();
        if !at_symbol {
            equaliser.push(derotate(y, self.symbols_out as f64 - 0.5, rotation));
            return None;
        }
        equaliser.push(derotate(y, self.symbols_out as f64, rotation));
        self.symbols_out += 1;
        let z = equaliser.symbol(self.carrier.as_mut(), self.params.constellation());
        Some((z, None))
    }

    pub(super) fn equalise(&mut self, config: EqualiserConfig) -> Result<(), EqualiserError> {
        self.equaliser = Some(Equaliser::new(config, self.params.constellation())?);
        Ok(())
    }

    pub(super) fn equaliser_resets(&self) -> u64 {
        self.equaliser.as_ref().map_or(0, Equaliser::resets)
    }

    pub(super) fn acquisition_order(&self) -> Option<u32> {
        self.carrier
            .as_ref()
            .map(|c| c.acquisition_order(self.params.constellation()))
    }

    pub(super) fn reset(&mut self) {
        if let Some(carrier) = &mut self.carrier {
            carrier.reset();
        }
        if let Some(equaliser) = &mut self.equaliser {
            equaliser.reset();
        }
        self.power = INITIAL_POWER;
        self.symbols_out = 0;
        self.timing.reset();
    }

    fn push(&mut self, y: Complex<f32>) -> Complex<f32> {
        let symbol = self.prepare(y);
        self.track(symbol)
    }

    fn track(&mut self, symbol: Complex<f32>) -> Complex<f32> {
        match &mut self.carrier {
            Some(carrier) => carrier.advance(symbol, self.params.constellation()),
            None => symbol,
        }
    }
}
