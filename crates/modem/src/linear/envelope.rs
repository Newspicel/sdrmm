mod levels;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, SymbolSync, one_pole_coeff};

use levels::LevelTracker;

use super::params::LinearParams;
use crate::{constellation::Constellation, pulse::assert_unit_energy};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvelopeTiming {
    pub mean_symbols: f64,
    pub warmup_symbols: usize,
    pub dc_symbols: f64,
}

impl EnvelopeTiming {
    pub const CONTINUOUS: Self = Self {
        mean_symbols: 200.0,
        warmup_symbols: 32,
        dc_symbols: 50.0,
    };
}

pub struct EnvelopeDemod {
    matched: Decimator,
    sync: SymbolSync,
    dc: f32,
    dc_alpha: f32,
    dc_primed: bool,
    levels: LevelTracker,
    quietest: f32,
    filtered: Vec<Complex<f32>>,
    magnitude: Vec<Complex<f32>>,
    retimed: Vec<Complex<f32>>,
}

impl EnvelopeDemod {
    #[must_use]
    pub fn new(
        params: &LinearParams,
        receive_filter: &[f32],
        timing_bw: f64,
        timing: EnvelopeTiming,
    ) -> Self {
        assert_unit_energy(receive_filter);
        let (table_mean, table_sd, quietest) = amplitude_spread(params.constellation());
        Self {
            matched: Decimator::new(receive_filter, 1),
            sync: SymbolSync::new(params.sps() as f64, timing_bw),
            dc: 0.0,
            dc_alpha: one_pole_coeff(params.sps() as f64, timing.dc_symbols),
            dc_primed: false,
            levels: LevelTracker::new(timing, table_mean, table_sd),
            quietest,
            filtered: Vec::new(),
            magnitude: Vec::new(),
            retimed: Vec::new(),
        }
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<f32>) {
        self.matched.process(iq, &mut self.filtered);
        self.magnitude.clear();
        self.magnitude.reserve(self.filtered.len());
        for s in &self.filtered {
            let envelope = s.norm();
            if self.dc_primed {
                self.dc += self.dc_alpha * (envelope - self.dc);
            } else {
                self.dc = envelope;
                self.dc_primed = true;
            }
            self.magnitude.push(Complex::new(envelope - self.dc, 0.0));
        }
        self.retimed.clear();
        self.sync.process(&self.magnitude, &mut self.retimed);
        for y in &self.retimed {
            out.push(self.levels.push(y.re).unwrap_or(self.quietest));
        }
    }

    #[must_use]
    pub fn level_snr(&self) -> f32 {
        self.levels.snr()
    }

    pub fn reset(&mut self) {
        self.sync.reset();
        self.levels.reset();
        self.dc = 0.0;
        self.dc_primed = false;
    }
}

#[must_use]
pub fn slice_amplitude(table: &Constellation, amplitude: f32) -> u32 {
    let mut best = 0usize;
    let mut best_d = f64::INFINITY;
    for (i, p) in table.points().iter().enumerate() {
        let d = (f64::from(amplitude) - f64::from(p.norm())).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    table.labels()[best]
}

/// Mean, standard deviation and smallest of the table's point amplitudes.
fn amplitude_spread(table: &Constellation) -> (f32, f32, f32) {
    let amplitudes: Vec<f32> = table.points().iter().map(|p| p.norm()).collect();
    let largest = amplitudes.iter().copied().fold(0.0f32, f32::max);
    assert!(
        largest > 0.0,
        "an all-origin table has no amplitude to scale to"
    );
    let n = amplitudes.len() as f32;
    let mean = amplitudes.iter().sum::<f32>() / n;
    let sd = (amplitudes
        .iter()
        .map(|a| (a - mean) * (a - mean))
        .sum::<f32>()
        / n)
        .sqrt();
    assert!(
        sd > 0.0,
        "a constant-modulus table carries no amplitude for an envelope tier to detect"
    );
    let quietest = amplitudes.iter().copied().fold(f32::MAX, f32::min);
    (mean, sd, quietest)
}

#[cfg(test)]
mod tests {
    use crate::testutil::draws;
    use sdrmm_modem_test_support::ber::{
        impair::{Awgn, Impairment},
        perf::assert_no_alloc,
        rng::Rng,
    };

    use super::*;
    use crate::{
        constellation::tables,
        linear::{
            LinearMod, TIMING_BW_CONTINUOUS,
            rotation::phasor,
            testutil::{SPS, rotate, rrc},
        },
    };

    fn demodulate(p: &LinearParams, wave: &[Complex<f32>]) -> Vec<f32> {
        let mut demod =
            EnvelopeDemod::new(p, &rrc(), TIMING_BW_CONTINUOUS, EnvelopeTiming::CONTINUOUS);
        let mut out = Vec::new();
        demod.process(wave, &mut out);
        out
    }

    fn alignment(out: &[f32], sent: &[u32], table: &Constellation) -> usize {
        let window = 400..1_200;
        (0..40)
            .filter(|off| off + window.end <= out.len())
            .min_by_key(|off| {
                window
                    .clone()
                    .filter(|&k| slice_amplitude(table, out[k + off]) != sent[k])
                    .count()
            })
            .expect("the demodulator returned too few symbols to align")
    }

    fn errors(out: &[f32], sent: &[u32], table: &Constellation, from: usize) -> (usize, usize) {
        let off = alignment(out, sent, table);
        let last = sent.len().min(out.len() - off);
        let wrong = (from..last)
            .filter(|&k| slice_amplitude(table, out[k + off]) != sent[k])
            .count();
        (wrong, last - from)
    }

    #[test]
    fn noiseless_ook_recovers_every_symbol() {
        let table = tables::ook().unwrap();
        let sent = draws(2_000, 2, 0x0009);
        let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let out = demodulate(&p, &LinearMod::transmission(&p, &sent));
        let (wrong, total) = errors(&out, &sent, &table, 400);
        assert_eq!(wrong, 0, "{wrong} of {total} mis-sliced");
    }

    #[test]
    fn noiseless_4ask_carries_the_tiers_own_self_noise() {
        let table = tables::ask(4).unwrap();
        let sent = draws(2_000, 4, 0x4a54);
        let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let out = demodulate(&p, &LinearMod::transmission(&p, &sent));
        let (wrong, total) = errors(&out, &sent, &table, 400);
        assert!(
            wrong * 100 < total,
            "{wrong} of {total} mis-sliced: past the measured self-noise floor"
        );
        assert!(
            wrong > 0,
            "the self-noise floor vanished; re-measure the bound"
        );
    }

    #[test]
    fn an_unknown_carrier_phase_costs_the_envelope_tier_nothing() {
        let table = tables::ook().unwrap();
        let sent = draws(1_500, 2, 0x0009);
        let p = LinearParams::new(table, rrc(), SPS).unwrap();
        let clean = LinearMod::transmission(&p, &sent);
        let rot = phasor(0.9);
        let rotated: Vec<Complex<f32>> = clean.iter().map(|&s| s * rot).collect();
        let a = demodulate(&p, &clean);
        let b = demodulate(&p, &rotated);
        let worst = a
            .iter()
            .zip(&b)
            .map(|(x, y)| f64::from((x - y).abs()))
            .fold(0.0, f64::max);
        assert!(worst < 1e-5, "a static phase moved the envelope by {worst}");
    }

    #[test]
    fn a_frequency_offset_costs_the_envelope_tier_only_the_filter_skirt() {
        let table = tables::ook().unwrap();
        let sent = draws(1_500, 2, 0x0009);
        let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let clean = LinearMod::transmission(&p, &sent);
        let mut shifted = clean.clone();
        rotate(&mut shifted, 3e-3, 0.0);
        let a = demodulate(&p, &clean);
        let b = demodulate(&p, &shifted);
        let worst = a
            .iter()
            .zip(&b)
            .map(|(x, y)| f64::from((x - y).abs()))
            .fold(0.0, f64::max);
        let margin = f64::from(table.points()[1].norm()) / 2.0;
        assert!(worst < 0.1 * margin, "offset moved the envelope by {worst}");
        let (wrong, total) = errors(&b, &sent, &table, 400);
        assert_eq!(wrong, 0, "{wrong} of {total} mis-sliced under the offset");
    }

    #[test]
    fn the_threshold_follows_a_level_step() {
        let table = tables::ook().unwrap();
        let sent = draws(4_000, 2, 0x57e9);
        let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let mut wave = LinearMod::transmission(&p, &sent);
        let step_at = wave.len() / 2;
        for s in &mut wave[step_at..] {
            *s *= 0.25;
        }
        let out = demodulate(&p, &wave);
        let (wrong, total) = errors(&out, &sent, &table, sent.len() / 2 + 400);
        assert_eq!(wrong, 0, "{wrong} of {total} mis-sliced after the step");
    }

    #[test]
    fn the_fitted_pedestal_survives_noise() {
        let table = tables::ook().unwrap();
        let sent = draws(4_000, 2, 0x0f100);
        let p = LinearParams::new(table.clone(), rrc(), SPS).unwrap();
        let mut wave = LinearMod::transmission(&p, &sent);
        Awgn::with_sigma((0.5 * 10f64.powf(-1.3)).sqrt()).apply(&mut wave, &mut Rng::new(0x9e));
        let out = demodulate(&p, &wave);
        let (wrong, total) = errors(&out, &sent, &table, 400);
        assert!(
            wrong * 200 < total,
            "{wrong} of {total} mis-sliced at 13 dB"
        );
    }

    #[test]
    fn a_constant_modulus_table_is_refused() {
        let p = LinearParams::new(tables::psk(4).unwrap(), rrc(), SPS).unwrap();
        let built = std::panic::catch_unwind(|| {
            EnvelopeDemod::new(&p, &rrc(), TIMING_BW_CONTINUOUS, EnvelopeTiming::CONTINUOUS)
        });
        assert!(built.is_err());
    }

    #[test]
    fn slicing_reads_the_table_by_amplitude() {
        let table = tables::ask(4).unwrap();
        let mut amplitudes: Vec<f32> = table.points().iter().map(|p| p.norm()).collect();
        amplitudes.sort_by(f32::total_cmp);
        for (i, &a) in amplitudes.iter().enumerate() {
            assert_eq!(slice_amplitude(&table, a), tables::gray(i as u32));
        }
        let mid = 0.5 * (amplitudes[0] + amplitudes[1]);
        assert_eq!(slice_amplitude(&table, mid - 0.01), tables::gray(0));
        assert_eq!(slice_amplitude(&table, mid + 0.01), tables::gray(1));
        assert_eq!(slice_amplitude(&table, 99.0), tables::gray(3));
    }

    #[test]
    fn steady_state_allocates_nothing() {
        let p = LinearParams::new(tables::ask(4).unwrap(), rrc(), SPS).unwrap();
        let wave = LinearMod::transmission(&p, &draws(2_048, 4, 0x0a12));
        let mut demod =
            EnvelopeDemod::new(&p, &rrc(), TIMING_BW_CONTINUOUS, EnvelopeTiming::CONTINUOUS);
        let mut out = Vec::with_capacity(wave.len());
        demod.process(&wave, &mut out);
        out.clear();
        demod.process(&wave, &mut out);
        out.clear();
        assert_no_alloc("EnvelopeDemod::process", || {
            demod.process(&wave, &mut out);
        });
        assert!(!out.is_empty());
    }
}
