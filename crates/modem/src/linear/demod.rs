mod burst;
mod decision_timing;
mod stage;
mod unstagger;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, SymbolSync};

pub use burst::LinearBurstDemod;
use stage::SymbolStage;
use unstagger::Unstagger;

use super::{
    carrier::CarrierLoop,
    equaliser::{Equaliser, EqualiserConfig, EqualiserError},
    params::LinearParams,
};
use crate::pulse::assert_unit_energy;

pub const TIMING_BW_CONTINUOUS: f64 = 0.003;

pub const TIMING_BW_BURST: f64 = 0.015;

/// Symbols the continuous level average spans.
const CONTINUOUS_POWER_SYMBOLS: f64 = 1_000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearTiming {
    pub timing_bw: f64,
    pub power_symbols: f64,
}

impl LinearTiming {
    pub const CONTINUOUS: Self = Self {
        timing_bw: TIMING_BW_CONTINUOUS,
        power_symbols: CONTINUOUS_POWER_SYMBOLS,
    };

    pub const BURST: Self = Self {
        timing_bw: TIMING_BW_BURST,
        power_symbols: f64::INFINITY,
    };
}

pub struct LinearDemod {
    matched: Decimator,
    sync: SymbolSync,
    stage: SymbolStage,
    stagger: Unstagger,
    aligned: Vec<Complex<f32>>,
    filtered: Vec<Complex<f32>>,
}

impl LinearDemod {
    #[must_use]
    pub fn new(
        params: &LinearParams,
        receive_filter: &[f32],
        timing: LinearTiming,
        carrier: Option<CarrierLoop>,
    ) -> Self {
        assert_unit_energy(receive_filter);
        Self {
            matched: Decimator::new(receive_filter, 1),
            sync: SymbolSync::new(params.sps() as f64, timing.timing_bw),
            stage: SymbolStage::new(params, timing.power_symbols, carrier),
            stagger: Unstagger::new(params.stagger_samples()),
            aligned: Vec::new(),
            filtered: Vec::new(),
        }
    }

    #[must_use]
    pub fn params(&self) -> &LinearParams {
        &self.stage.params
    }

    #[must_use]
    pub fn carrier_freq_cycles_per_symbol(&self) -> f64 {
        self.stage
            .carrier
            .as_ref()
            .map_or(0.0, CarrierLoop::freq_cycles_per_symbol)
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

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        self.stagger.apply(iq, &mut self.aligned);
        self.matched.process(&self.aligned, &mut self.filtered);
        let stage = &mut self.stage;
        if stage.equaliser.is_none() {
            self.sync.process_steered(&self.filtered, |y| {
                let (z, err) = stage.push_steering(y);
                out.push(z);
                err
            });
            return;
        }
        self.sync.process_spaced(&self.filtered, |y, at_symbol| {
            let (z, err) = stage.push_spaced(y, at_symbol)?;
            out.push(z);
            err
        });
    }

    pub fn reset(&mut self) {
        self.sync.reset();
        self.stage.reset();
        self.stagger.reset();
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::draws;
    use sdrmm_modem_test_support::ber::perf::assert_no_alloc;

    use super::*;
    use crate::{
        constellation::tables,
        linear::{
            LinearMod, PhaseDetector,
            testutil::{SPS, evm, mis_sliced, rotate, rrc},
        },
    };

    fn settled(
        params: &LinearParams,
        wave: &[Complex<f32>],
        carrier: Option<CarrierLoop>,
    ) -> Vec<Complex<f32>> {
        let mut demod = LinearDemod::new(params, &rrc(), LinearTiming::CONTINUOUS, carrier);
        let mut out = Vec::new();
        demod.process(wave, &mut out);
        let settled_from = out.len() * 3 / 4;
        out.split_off(settled_from)
    }

    #[test]
    fn a_noiseless_loopback_lands_on_the_table() {
        for (name, table) in [
            ("bpsk", tables::pam(2).unwrap()),
            ("qpsk", tables::qam_square(4).unwrap()),
            ("16-qam", tables::qam_square(16).unwrap()),
            ("8-psk", tables::psk(8).unwrap()),
            ("cross-32", tables::qam_cross(32).unwrap()),
        ] {
            let m = table.len() as u32;
            let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
            let sent = draws(12_000, m as usize, 0xd0d0);
            let wave = LinearMod::transmission(&p, &sent);
            let rms = evm(&table, &settled(&p, &wave, None));
            let margin = table.min_distance() / 2.0;
            assert!(
                rms < 0.1 * margin,
                "{name}: residual {rms} RMS, slicing margin {margin}"
            );
        }
    }

    #[test]
    fn every_noiseless_symbol_slices_back_to_its_label() {
        let table = tables::qam_square(16).unwrap();
        let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let sent = draws(4_000, 16, 0x51ce);
        let wave = LinearMod::transmission(&p, &sent);
        let got = settled(&p, &wave, None);
        let wrong = mis_sliced(&table, &got, &sent);
        assert_eq!(wrong, 0, "{wrong} of {} symbols mis-sliced", got.len());
    }

    #[test]
    fn oqpsk_recovers_through_the_unstagger() {
        let table = tables::qam_square(4).unwrap();
        let sent = draws(3_000, 4, 0x0954);
        let plain = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let offset = plain.clone().with_offset(true).unwrap();
        for (name, p) in [("qpsk", plain), ("oqpsk", offset)] {
            let wave = LinearMod::transmission(&p, &sent);
            let wrong = mis_sliced(&table, &settled(&p, &wave, None), &sent);
            assert_eq!(wrong, 0, "{name}: {wrong} mis-sliced");
        }
    }

    #[test]
    fn ignoring_the_stagger_wrecks_the_constellation() {
        let table = tables::qam_square(4).unwrap();
        let sent = draws(2_000, 4, 0x0954);
        let staggered = LinearParams::new(table.clone(), rrc(), SPS)
            .unwrap()
            .with_offset(true)
            .unwrap();
        let wave = LinearMod::transmission(&staggered, &sent);
        let plain = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let got = settled(&plain, &wave, None);
        let wrong = mis_sliced(&table, &got, &sent);
        assert!(
            wrong > got.len() / 5,
            "only {wrong} of {} mis-sliced without the unstagger",
            got.len()
        );
    }

    #[test]
    fn the_rotation_schedule_is_undone_at_the_receiver() {
        let table = tables::pam(2).unwrap();
        let p = LinearParams::new(table.clone(), rrc(), SPS)
            .unwrap()
            .with_rotation(tables::PI_2_ROTATION)
            .unwrap();
        let sent = draws(3_000, 2, 0x9020);
        let wave = LinearMod::transmission(&p, &sent);
        let wrong = mis_sliced(&table, &settled(&p, &wave, None), &sent);
        assert_eq!(wrong, 0, "{wrong} mis-sliced");
    }

    #[test]
    fn the_carrier_loop_recovers_an_offset_the_open_chain_cannot() {
        let table = tables::psk(4).unwrap();
        let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let sent = draws(6_000, 4, 0x0ff5);
        let mut wave = LinearMod::transmission(&p, &sent);
        rotate(&mut wave, 1e-4, 0.0);
        let spread = |symbols: &[Complex<f32>]| -> f64 {
            symbols
                .iter()
                .map(|&y| f64::from((y - table.nearest(y)).norm()))
                .sum::<f64>()
                / symbols.len() as f64
        };
        let open = settled(&p, &wave, None);
        let locked = settled(
            &p,
            &wave,
            Some(CarrierLoop::new(PhaseDetector::MthPower { m: 4 }, 0.01)),
        );
        assert!(spread(&locked) < 0.05, "locked spread {}", spread(&locked));
        assert!(spread(&open) > 0.3, "open-loop spread {}", spread(&open));
    }

    #[test]
    fn any_block_split_gives_the_same_symbols() {
        let table = tables::qam_square(16).unwrap();
        let p = LinearParams::new(table, rrc(), SPS)
            .unwrap()
            .with_offset(true)
            .unwrap();
        let wave = LinearMod::transmission(&p, &draws(1_500, 16, 0x5b17));
        let carrier = || Some(CarrierLoop::new(PhaseDetector::DecisionDirected, 0.01));
        let mut whole = LinearDemod::new(&p, &rrc(), LinearTiming::CONTINUOUS, carrier());
        let mut a = Vec::new();
        whole.process(&wave, &mut a);
        let mut split = LinearDemod::new(&p, &rrc(), LinearTiming::CONTINUOUS, carrier());
        let mut b = Vec::new();
        for chunk in wave.chunks(311) {
            split.process(chunk, &mut b);
        }
        assert_eq!(a, b);
    }

    #[test]
    fn steady_state_allocates_nothing() {
        for (name, offset) in [("plain", false), ("staggered", true)] {
            let p = LinearParams::new(tables::qam_square(16).unwrap(), rrc(), SPS)
                .unwrap()
                .with_offset(offset)
                .unwrap();
            let wave = LinearMod::transmission(&p, &draws(2_048, 16, 0x0a11));
            let mut demod = LinearDemod::new(
                &p,
                &rrc(),
                LinearTiming::CONTINUOUS,
                Some(CarrierLoop::new(PhaseDetector::DecisionDirected, 0.01)),
            );
            let mut out = Vec::with_capacity(wave.len());
            demod.process(&wave, &mut out);
            out.clear();
            demod.process(&wave, &mut out);
            out.clear();
            assert_no_alloc(&format!("LinearDemod::process ({name})"), || {
                demod.process(&wave, &mut out);
            });
            assert!(
                !out.is_empty(),
                "{name}: the measured call recovered nothing"
            );
        }
    }
}
