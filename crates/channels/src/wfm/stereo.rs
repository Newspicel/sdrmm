use std::f64::consts::{FRAC_1_SQRT_2, TAU};

use num_complex::Complex;
use sdrmm_dsp::{ComplexOnePole, Decimator, Deemphasis, Nco, Pll, one_pole_coeff};

use crate::AUDIO_RATE;

const SUBCARRIER_HZ: f64 = 38_000.0;
const PILOT_CUTOFF_HZ: f64 = 400.0;
const PILOT_STAGES: usize = 3;
const PILOT_LOOP_BW_HZ: f64 = 30.0;
const PILOT_RANGE_HZ: f64 = 120.0;
const LOCK_ON: f32 = 0.6;
const LOCK_OFF: f32 = 0.4;
const BLEND_TAU_S: f64 = 0.05;

/// Recovers L-R from the composite. The 38 kHz subcarrier is mixed to baseband at the full rate
/// by an oscillator held at twice the pilot mixer's phase, and decimated to audio as a complex
/// signal. The pilot loop runs on the decimated pilot, so only its slow phase correction is
/// applied, once per audio sample.
pub(super) struct StereoDemux {
    subcarrier: Nco,
    mixed: Vec<Complex<f32>>,
    decim: Decimator,
    side_bb: Vec<Complex<f32>>,
    side: Vec<f32>,
    filter: ComplexOnePole,
    pll: Pll,
    audio_per_pilot: f64,
    since_pilot: f64,
    reference: Complex<f32>,
    step: Complex<f32>,
    pub(super) deemphasis: Deemphasis,
    blend: f32,
    blend_coeff: f32,
    target: f32,
}

impl StereoDemux {
    pub(super) fn new(
        mpx_rate: f64,
        pilot_rate: f64,
        decim_taps: &[f32],
        decim_factor: usize,
        deemphasis: Deemphasis,
    ) -> Self {
        Self {
            subcarrier: Nco::new(-SUBCARRIER_HZ as f32, mpx_rate as f32),
            mixed: Vec::new(),
            decim: Decimator::new(decim_taps, decim_factor),
            side_bb: Vec::new(),
            side: Vec::new(),
            filter: ComplexOnePole::new(pilot_rate, PILOT_CUTOFF_HZ, PILOT_STAGES),
            pll: Pll::new(
                PILOT_LOOP_BW_HZ / pilot_rate,
                FRAC_1_SQRT_2,
                0.0,
                PILOT_RANGE_HZ / pilot_rate,
            ),
            audio_per_pilot: mpx_rate / decim_factor as f64 / pilot_rate,
            since_pilot: 0.0,
            reference: Complex::new(1.0, 0.0),
            step: Complex::new(1.0, 0.0),
            deemphasis,
            blend: 0.0,
            blend_coeff: one_pole_coeff(f64::from(AUDIO_RATE), BLEND_TAU_S),
            target: 0.0,
        }
    }

    /// Puts the subcarrier mixer at twice the pilot mixer's phase.
    pub(super) fn align(&mut self, pilot_turns: f64) {
        self.subcarrier.set_phase(2.0 * pilot_turns);
    }

    pub(super) fn process(
        &mut self,
        composite: &[f32],
        pilot: &[Complex<f32>],
        sum: &[f32],
        out: &mut Vec<f32>,
    ) {
        self.mixed.resize(composite.len(), Complex::new(0.0, 0.0));
        self.subcarrier.mix_real_into(composite, &mut self.mixed);
        self.decim.process(&self.mixed, &mut self.side_bb);
        self.demodulate(pilot);
        self.deemphasis.process(&mut self.side);
        self.update_blend();
        debug_assert_eq!(sum.len(), self.side.len(), "stereo paths drifted apart");
        out.clear();
        out.reserve(2 * sum.len());
        for (&mid, &side) in sum.iter().zip(&self.side) {
            self.blend += self.blend_coeff * (self.target - self.blend);
            let side = side * self.blend;
            out.push(mid + side);
            out.push(mid - side);
        }
    }

    /// Spreads the block's pilot samples evenly over its audio samples.
    fn demodulate(&mut self, pilot: &[Complex<f32>]) {
        let mut pilots = pilot.iter();
        let side_bb = std::mem::take(&mut self.side_bb);
        self.side.clear();
        for &baseband in &side_bb {
            if self.since_pilot >= self.audio_per_pilot
                && let Some(&next) = pilots.next()
            {
                self.since_pilot -= self.audio_per_pilot;
                self.track(next);
            }
            self.since_pilot += 1.0;
            self.side.push(2.0 * (baseband * self.reference).im);
            self.reference *= self.step;
        }
        self.side_bb = side_bb;
        for &next in pilots {
            self.track(next);
        }
    }

    fn track(&mut self, pilot: Complex<f32>) {
        let _ = self.pll.process(self.filter.process(pilot));
        self.reference = self.pll.harmonic(2.0).conj();
        let turn = -2.0 * TAU * self.pll.increment_norm() / self.audio_per_pilot;
        self.step = Complex::from_polar(1.0, turn as f32);
    }

    fn update_blend(&mut self) {
        let lock = self.pll.lock();
        if lock > LOCK_ON {
            self.target = 1.0;
        } else if lock < LOCK_OFF {
            self.target = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{rms, split_stereo, tone_power};

    const RATE: f64 = 240_000.0;
    const PILOT_RATE: f64 = 9_600.0;
    const FACTOR: usize = 5;

    fn side_amplitude(pilot_offset_hz: f64) -> f64 {
        let taps = sdrmm_dsp::design_lowpass(199, 15_000.0 / RATE);
        let mut demux = StereoDemux::new(
            RATE,
            PILOT_RATE,
            &taps,
            FACTOR,
            Deemphasis::new(f64::from(AUDIO_RATE), 50.0),
        );
        let seconds = 2.0;
        let len = (RATE * seconds) as usize;
        let phase = |n: usize| TAU * pilot_offset_hz * n as f64 / RATE + 0.7;
        let composite: Vec<f32> = (0..len)
            .map(|n| {
                let t = n as f64 / RATE;
                let subcarrier = 2.0 * (TAU * 19_000.0 * t + phase(n));
                (-(TAU * 1_000.0 * t).sin() * subcarrier.sin()) as f32
            })
            .collect();
        let decimation = (RATE / PILOT_RATE) as usize;
        let pilot: Vec<Complex<f32>> = (0..len / decimation)
            .map(|k| Complex::from_polar(0.05, phase(k * decimation) as f32))
            .collect();
        let mut audio = Vec::new();
        let mut out = Vec::new();
        let block = 4_800;
        for (composite, pilot) in composite
            .chunks(block)
            .zip(pilot.chunks(block / decimation))
        {
            let sum = vec![0.0; composite.len() / FACTOR];
            demux.process(composite, pilot, &sum, &mut out);
            audio.extend_from_slice(&out);
        }
        let (left, _) = split_stereo(&audio);
        let settled = &left[left.len() / 2..];
        let share = tone_power(settled, 1_000.0, f64::from(AUDIO_RATE));
        assert!(share > 0.99, "tone share {share}");
        f64::from(rms(settled)) * std::f64::consts::SQRT_2
    }

    #[test]
    fn the_side_signal_follows_an_offset_pilot() {
        let centred = side_amplitude(0.0);
        assert!((0.95..1.02).contains(&centred), "side amplitude {centred}");
        let offset = side_amplitude(2.0);
        assert!(
            (offset / centred - 1.0).abs() < 0.01,
            "a 2 Hz pilot offset moves the side level {centred} to {offset}"
        );
    }
}
