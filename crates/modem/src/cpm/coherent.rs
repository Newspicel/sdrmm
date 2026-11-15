use num_complex::Complex;

use super::{
    laurent::{LaurentError, QUARTER_TURN, laurent_main_pulse, polarity, whole_sps},
    params::CpmParams,
};
use crate::{
    constellation::tables,
    linear::{CarrierLoop, LinearBurstDemod, LinearParams, PhaseDetector, TimingMetric},
};

const POWER_SYMBOLS: f64 = 64.0;

pub struct CoherentCpmDemod {
    receiver: LinearBurstDemod,
    polarity: f32,
    symbols: Vec<Complex<f32>>,
}

impl CoherentCpmDemod {
    pub fn new(params: &CpmParams, loop_bw: f64) -> Result<Self, LaurentError> {
        let linear = linear_params(params)?;
        let receiver = LinearBurstDemod::new(
            &linear,
            linear.pulse(),
            POWER_SYMBOLS,
            Some(carrier(loop_bw)),
        )
        .with_timing_metric(TimingMetric::RotatedPower {
            rotation_rad: QUARTER_TURN,
            order: 2,
        });
        Ok(Self {
            receiver,
            polarity: polarity(params),
            symbols: Vec::new(),
        })
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<f32>) {
        self.symbols.clear();
        let _ = self.receiver.process(iq, &mut self.symbols);
        out.extend(
            self.symbols
                .windows(2)
                .map(|pair| self.polarity * pair[1].re * pair[0].re),
        );
    }

    #[must_use]
    pub fn carrier_freq_cycles_per_symbol(&self) -> f64 {
        self.receiver.carrier_freq_cycles_per_symbol()
    }
}

fn linear_params(params: &CpmParams) -> Result<LinearParams, LaurentError> {
    let sps = whole_sps(params)?;
    let pulse = laurent_main_pulse(params)?;
    let table = tables::pam(2).map_err(|_| LaurentError::NotBinary(2))?;
    LinearParams::new(table, pulse, sps)
        .and_then(|p| p.with_rotation(QUARTER_TURN))
        .map_err(|_| LaurentError::FractionalSps(params.sps()))
}

fn carrier(loop_bw: f64) -> CarrierLoop {
    CarrierLoop::new(PhaseDetector::MthPower { m: 2 }, loop_bw)
}
#[cfg(test)]
mod tests {
    use super::{super::testutil::transmit, *};
    use crate::testutil::draws;
    use crate::{
        cpm::Mapping,
        pulse::{self, Norm},
    };

    const SPS: f64 = 8.0;

    fn gmsk(bt: f64) -> CpmParams {
        CpmParams::from_h(
            Mapping::natural(2),
            0.5,
            pulse::gaussian_freq(SPS, bt, 4, Norm::Area),
            SPS,
        )
    }

    fn msk() -> CpmParams {
        CpmParams::from_h(Mapping::natural(2), 0.5, pulse::rect(SPS, Norm::Area), SPS)
    }

    fn best_alignment(soft: &[f32], sent: &[u8]) -> (usize, isize) {
        (-8isize..8)
            .map(|lag| {
                let errors = (64..sent.len() - 64)
                    .filter(|&i| {
                        soft.get((i as isize + lag) as usize)
                            .is_none_or(|&s| (s > 0.0) != (sent[i] == 1))
                    })
                    .count();
                (errors, lag)
            })
            .min()
            .unwrap_or((usize::MAX, 0))
    }
    #[test]
    fn a_noiseless_burst_decodes_clean() {
        for (name, params) in [
            ("msk", msk()),
            ("gmsk 0.5", gmsk(0.5)),
            ("gmsk 0.3", gmsk(0.3)),
        ] {
            let sent = draws(2_000, 2, 0x1a0);
            let mut demod = CoherentCpmDemod::new(&params, 0.005).unwrap();
            let mut soft = Vec::new();
            demod.process(&transmit(&params, &sent), &mut soft);
            let (errors, _) = best_alignment(&soft, &sent);
            assert_eq!(errors, 0, "{name}");
        }
    }

    #[test]
    fn a_carrier_offset_is_acquired_on_a_burst() {
        let params = gmsk(0.3);
        let sent = draws(3_000, 2, 0x2b1);
        let mut w = transmit(&params, &sent);
        for (k, s) in w.iter_mut().enumerate() {
            let theta = std::f64::consts::TAU * 0.004 * k as f64;
            *s *= Complex::new(theta.cos() as f32, theta.sin() as f32);
        }
        let mut demod = CoherentCpmDemod::new(&params, 0.005).unwrap();
        let mut soft = Vec::new();
        demod.process(&w, &mut soft);
        let (errors, _) = best_alignment(&soft, &sent);
        assert_eq!(errors, 0);
        let read = demod.carrier_freq_cycles_per_symbol() / SPS;
        assert!((read - 0.004).abs() < 1e-4, "read {read} cycles/sample");
    }
}
