use num_complex::Complex;

use crate::{fft::Transform, window::hann};

const BINS: usize = 1_024;
const OCCUPIED_SHARE: f64 = 0.99;

/// Measures how wide a signal is on the air: the band that holds 99 % of its power, averaged
/// over every block it is shown.
pub struct EmissionMeter {
    rate: f64,
    fft: Transform,
    window: Vec<f32>,
    block: Vec<Complex<f32>>,
    filled: usize,
    power: Vec<f64>,
    blocks: usize,
}

impl EmissionMeter {
    #[must_use]
    pub fn new(rate: f64) -> Self {
        Self {
            rate,
            fft: Transform::forward(BINS),
            window: hann(BINS),
            block: vec![Complex::new(0.0, 0.0); BINS],
            filled: 0,
            power: vec![0.0; BINS],
            blocks: 0,
        }
    }

    pub fn reset(&mut self) {
        self.filled = 0;
        self.blocks = 0;
        self.power.fill(0.0);
    }

    /// Whole blocks measured since the last reset.
    #[must_use]
    pub const fn blocks(&self) -> usize {
        self.blocks
    }

    pub fn push(&mut self, samples: &[Complex<f32>]) {
        for sample in samples {
            self.block[self.filled] = sample * self.window[self.filled];
            self.filled += 1;
            if self.filled == BINS {
                self.transform();
            }
        }
    }

    fn transform(&mut self) {
        self.fft.process(&mut self.block);
        for (sum, bin) in self.power.iter_mut().zip(&self.block) {
            *sum += f64::from(bin.norm_sqr());
        }
        self.filled = 0;
        self.blocks += 1;
    }

    /// The width in hertz that holds 99 % of the power, the rest split evenly below and above.
    /// `None` until a block of signal has been measured.
    #[must_use]
    pub fn occupied_hz(&self) -> Option<f64> {
        let total: f64 = self.power.iter().sum();
        if self.blocks == 0 || total <= 0.0 {
            return None;
        }
        let spare = total * (1.0 - OCCUPIED_SHARE) / 2.0;
        let ascending = |index: usize| self.power[(index + BINS / 2) % BINS];
        let edge = |bins: &mut dyn Iterator<Item = usize>| {
            let mut outside = 0.0;
            bins.take_while(|&index| {
                outside += ascending(index);
                outside <= spare
            })
            .count()
        };
        let below = edge(&mut (0..BINS));
        let above = edge(&mut (0..BINS).rev());
        let held = BINS.saturating_sub(below + above).max(1);
        Some(held as f64 * self.rate / BINS as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn carrier(offset_hz: f64, deviation_hz: f64, tone_hz: f64, len: usize) -> Vec<Complex<f32>> {
        let mut phase = 0.0f64;
        (0..len)
            .map(|n| {
                let audio = (std::f64::consts::TAU * tone_hz * n as f64 / RATE).sin();
                phase += std::f64::consts::TAU * (offset_hz + deviation_hz * audio) / RATE;
                Complex::new(phase.cos() as f32, phase.sin() as f32)
            })
            .collect()
    }

    fn measured(samples: &[Complex<f32>]) -> f64 {
        let mut meter = EmissionMeter::new(RATE);
        meter.push(samples);
        meter.occupied_hz().expect("a measurement")
    }

    #[test]
    fn nothing_is_reported_before_a_block_of_signal() {
        let mut meter = EmissionMeter::new(RATE);
        assert_eq!(meter.occupied_hz(), None);
        meter.push(&carrier(0.0, 0.0, 0.0, 500));
        assert_eq!(meter.occupied_hz(), None);
        let mut silent = EmissionMeter::new(RATE);
        silent.push(&vec![Complex::new(0.0, 0.0); 4_096]);
        assert_eq!(silent.blocks(), 4);
        assert_eq!(silent.occupied_hz(), None, "silence has no width");
    }

    #[test]
    fn a_bare_carrier_is_a_few_bins_wide_wherever_it_sits() {
        for offset in [0.0, 7_000.0, -11_000.0] {
            let width = measured(&carrier(offset, 0.0, 0.0, 48_000));
            assert!(width < 300.0, "carrier at {offset} Hz measured {width} Hz");
        }
    }

    #[test]
    fn frequency_modulation_widens_by_about_carsons_rule() {
        let narrow = measured(&carrier(0.0, 2_500.0, 1_000.0, 96_000));
        assert!(
            (5_000.0..9_000.0).contains(&narrow),
            "2.5 kHz deviation measured {narrow} Hz"
        );
        let wide = measured(&carrier(0.0, 5_000.0, 3_000.0, 96_000));
        assert!(
            (12_000.0..20_000.0).contains(&wide),
            "5 kHz deviation measured {wide} Hz"
        );
    }

    #[test]
    fn ragged_blocks_measure_the_same_and_reset_starts_over() {
        let signal = carrier(1_000.0, 2_500.0, 700.0, 20_480);
        let whole = measured(&signal);
        let mut ragged = EmissionMeter::new(RATE);
        for chunk in signal.chunks(333) {
            ragged.push(chunk);
        }
        assert_eq!(ragged.blocks(), 20);
        assert_eq!(ragged.occupied_hz(), Some(whole));
        ragged.reset();
        assert_eq!(ragged.occupied_hz(), None);
    }
}
