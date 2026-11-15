mod resample;
mod rotated;
mod square_law;
mod track;

use num_complex::Complex;
use sdrmm_dsp::Decimator;

pub use resample::resample_at;
pub use rotated::rotated_power_offset;
pub use square_law::square_law_offset;
pub use track::{TimingTrack, square_law_track};

use super::{acquire::FrequencyAcquisition, params::LinearParams};
use crate::pulse::assert_unit_energy;

pub const MIN_SPS: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TimingMetric {
    #[default]
    SquareLaw,
    RotatedPower {
        rotation_rad: f64,
        order: u32,
    },
}

pub struct FeedforwardTiming {
    matched: Decimator,
    sps: usize,
    metric: TimingMetric,
    filtered: Vec<Complex<f32>>,
    lines: Vec<Complex<f64>>,
    acquisition: FrequencyAcquisition,
    candidates: Vec<Complex<f32>>,
}

impl FeedforwardTiming {
    #[must_use]
    pub fn new(params: &LinearParams, receive_filter: &[f32]) -> Self {
        assert_unit_energy(receive_filter);
        assert!(
            params.sps() >= MIN_SPS,
            "the square-law line needs at least {MIN_SPS} samples per symbol, got {}",
            params.sps()
        );
        Self {
            matched: Decimator::new(receive_filter, 1),
            sps: params.sps(),
            metric: TimingMetric::SquareLaw,
            filtered: Vec::new(),
            lines: Vec::new(),
            acquisition: FrequencyAcquisition::new(),
            candidates: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_metric(mut self, metric: TimingMetric) -> Self {
        self.metric = metric;
        self
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Complex<f32>>) -> f64 {
        self.process_spaced(iq, 1, out)
    }

    pub fn process_spaced(
        &mut self,
        iq: &[Complex<f32>],
        per_symbol: usize,
        out: &mut Vec<Complex<f32>>,
    ) -> f64 {
        self.matched.process(iq, &mut self.filtered);
        let track = match self.metric {
            TimingMetric::SquareLaw => square_law_track(&self.filtered, self.sps, &mut self.lines),
            TimingMetric::RotatedPower {
                rotation_rad,
                order,
            } => TimingTrack {
                offset_samples: rotated_power_offset(
                    &self.filtered,
                    self.sps,
                    rotation_rad,
                    order,
                    &mut self.acquisition,
                    &mut self.candidates,
                ),
                at_sample: 0.0,
                rate: 0.0,
            },
        };
        track.resample_spaced(&self.filtered, self.sps, per_symbol, out);
        track.offset_at(0.0, self.sps)
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::draws;
    use sdrmm_modem_test_support::ber::{
        impair::{Awgn, Impairment},
        rng::Rng,
    };

    use super::*;
    use crate::{
        constellation::tables,
        linear::{
            LinearMod,
            testutil::{SPS, evm, rrc, rrc_at},
        },
    };

    #[test]
    fn the_estimate_reads_back_an_injected_fractional_delay() {
        let fine = 8usize;
        let table = tables::qam_square(16).unwrap();
        for shift in 0..fine {
            let dense = LinearParams::new(table.clone(), rrc_at(SPS * fine), SPS * fine).unwrap();
            let wave = LinearMod::transmission(&dense, &draws(400, 16, 0x71));
            let coarse: Vec<Complex<f32>> =
                wave.iter().skip(shift).step_by(fine).copied().collect();
            let mut matched = Decimator::new(&rrc(), 1);
            let mut filtered = Vec::new();
            matched.process(&coarse, &mut filtered);
            let measured = square_law_offset(&filtered, SPS);
            let want = (-(shift as f64) / fine as f64).rem_euclid(SPS as f64);
            let error = ((measured - want + SPS as f64 / 2.0).rem_euclid(SPS as f64)
                - SPS as f64 / 2.0)
                .abs();
            assert!(
                error < 0.05,
                "shift {shift}/{fine}: read {measured}, want {want}"
            );
        }
    }

    #[test]
    fn the_tier_recovers_every_table_well_inside_its_margin() {
        for (name, table) in [
            ("qam16", tables::qam_square(16).unwrap()),
            ("qam64", tables::qam_square(64).unwrap()),
            ("qam256", tables::qam_square(256).unwrap()),
            ("qam1024", tables::qam_square(1024).unwrap()),
            ("cross128", tables::qam_cross(128).unwrap()),
        ] {
            let m = table.len() as u32;
            let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
            let sent = draws(4_000, m as usize, 0x7e);
            let wave = LinearMod::transmission(&p, &sent);
            let mut tier = FeedforwardTiming::new(&p, &rrc());
            let mut symbols = Vec::new();
            tier.process(&wave, &mut symbols);
            let rms = evm(&table, &symbols[8..symbols.len() - 8]);
            let margin = table.min_distance() / 2.0;
            assert!(
                rms < 0.1 * margin,
                "{name}: EVM {rms} against a slicing margin of {margin}"
            );
        }
    }

    #[test]
    fn the_feedforward_estimate_beats_the_tracking_loop_on_a_burst() {
        use crate::linear::{LinearDemod, LinearTiming};
        let table = tables::qam_square(256).unwrap();
        let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let sent = draws(4_000, 256, 0x7e);
        let wave = LinearMod::transmission(&p, &sent);

        let mut tier = FeedforwardTiming::new(&p, &rrc());
        let mut feedforward = Vec::new();
        tier.process(&wave, &mut feedforward);

        let mut demod = LinearDemod::new(
            &p,
            &rrc(),
            LinearTiming {
                timing_bw: 0.005,
                power_symbols: 1_000.0,
            },
            None,
        );
        let mut tracked = Vec::new();
        demod.process(&wave, &mut tracked);

        let a = evm(&table, &feedforward[8..feedforward.len() - 8]);
        let b = evm(&table, &tracked[8..tracked.len() - 8]);
        assert!(a * 3.0 < b, "feedforward {a} vs tracked {b}");
    }

    #[test]
    fn the_estimate_survives_noise_over_a_long_burst() {
        let table = tables::qam_square(256).unwrap();
        let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let mut wave = LinearMod::transmission(&p, &draws(4_000, 256, 0x7e));
        Awgn::with_sigma((0.5 * 10f64.powf(-3.0)).sqrt()).apply(&mut wave, &mut Rng::new(0x91));
        let mut tier = FeedforwardTiming::new(&p, &rrc());
        let mut symbols = Vec::new();
        let offset = tier.process(&wave, &mut symbols);
        assert!((0.0..SPS as f64).contains(&offset), "offset {offset}");
        let clean = LinearMod::transmission(&p, &draws(4_000, 256, 0x7e));
        let mut reference = FeedforwardTiming::new(&p, &rrc());
        let mut ignored = Vec::new();
        let truth = reference.process(&clean, &mut ignored);
        let error = ((offset - truth + 4.0).rem_euclid(8.0) - 4.0).abs();
        assert!(error < 0.05, "noise moved the estimate by {error} samples");
    }

    #[test]
    fn a_track_whose_clock_never_advances_resamples_nothing() {
        let filtered = vec![Complex::new(1.0f32, 0.0); 256];
        for rate in [1.0f64, 2.0, f64::NAN, f64::INFINITY] {
            let track = TimingTrack {
                offset_samples: 0.5,
                at_sample: 0.0,
                rate,
            };
            let mut out = Vec::new();
            track.resample(&filtered, SPS, &mut out);
            assert!(out.is_empty(), "rate {rate} produced {} symbols", out.len());
        }
        let slow = TimingTrack {
            offset_samples: 0.5,
            at_sample: 0.0,
            rate: -1.0,
        };
        let mut out = Vec::new();
        slow.resample(&filtered, SPS, &mut out);
        assert_eq!(out.len(), 63);
    }

    #[test]
    fn resampling_from_an_unusable_offset_reads_no_samples() {
        let filtered = vec![Complex::new(1.0f32, 0.0); 256];
        let mut out = Vec::new();
        resample_at(&filtered, SPS, f64::NAN, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn an_empty_burst_estimates_nothing_rather_than_a_nan() {
        assert_eq!(square_law_offset(&[], SPS), 0.0);
        assert_eq!(square_law_offset(&[Complex::new(0.0, 0.0); 64], SPS), 0.0);
    }
}
