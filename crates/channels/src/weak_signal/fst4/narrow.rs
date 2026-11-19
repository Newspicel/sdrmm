//! Narrows the audio of the long periods: the band around the search moves to a quarter of a
//! rate some times lower, as real samples, so a 30-minute window holds 1.35 million samples
//! rather than 21.6 million.

use num_complex::Complex;
use sdrmm_dsp::filter::fir::{design_lowpass_kaiser, kaiser_taps};
use sdrmm_dsp::{Decimator, Nco};

/// Half the flat band around the centre, as a share of the narrowed rate.
const PASS: f64 = 0.2;
/// Where the stopband starts, as a share of the narrowed rate. Anything past it would fold onto
/// the band when the stream is made real, and audio below it would meet its own mirror image.
const STOP: f64 = 0.3;
const STOPBAND_DB: f64 = 80.0;
/// Room left between the search band and the edge of the flat band.
const MARGIN_HZ: f64 = 10.0;

pub(crate) struct Narrow {
    nco: Nco,
    decimator: Decimator,
    mixed: Vec<Complex<f32>>,
    narrowed: Vec<Complex<f32>>,
    quarter: u8,
    delay_s: f64,
}

impl Narrow {
    /// Moves `center_hz` of complex samples at `input_rate` to a quarter of `input_rate / by`.
    pub(crate) fn new(input_rate: f64, by: usize, center_hz: f64) -> Self {
        let share = 1.0 / by as f64;
        let taps = kaiser_taps((STOP - PASS) * share, STOPBAND_DB);
        let cutoff = f64::midpoint(PASS, STOP) * share;
        Self {
            nco: Nco::new(-center_hz as f32, input_rate as f32),
            decimator: Decimator::new(&design_lowpass_kaiser(taps, cutoff, STOPBAND_DB), by),
            mixed: Vec::new(),
            narrowed: Vec::new(),
            quarter: 0,
            delay_s: (taps - 1) as f64 / 2.0 / input_rate,
        }
    }

    /// How far the narrowed stream runs behind its input.
    pub(crate) fn delay_s(&self) -> f64 {
        self.delay_s
    }

    pub(crate) fn reset(&mut self) {
        self.nco.reset();
        self.decimator.reset();
        self.quarter = 0;
    }

    /// Appends what `input` narrows to onto `out`.
    pub(crate) fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<f32>) {
        self.mixed.resize(input.len(), Complex::default());
        self.nco.mix_into(input, &mut self.mixed);
        self.decimator.process(&self.mixed, &mut self.narrowed);
        out.extend(self.narrowed.iter().map(|sample| {
            let value = match self.quarter {
                0 => sample.re,
                1 => -sample.im,
                2 => -sample.re,
                _ => sample.im,
            };
            self.quarter = (self.quarter + 1) % 4;
            2.0 * value
        }));
    }
}

/// The span of a narrowed stream at `rate` that carries flat audio.
pub(crate) fn usable_hz(rate: f32) -> (f32, f32) {
    let rate = f64::from(rate);
    (((0.25 - PASS) * rate) as f32, ((0.25 + PASS) * rate) as f32)
}

/// The audio frequency a narrowed stream at `rate` centred on `center_hz` holds at 0 Hz.
pub(crate) fn shift_hz(rate: f32, center_hz: f32) -> f32 {
    center_hz - rate / 4.0
}

/// Whether a search from `low_hz` to `high_hz` fits a stream narrowed to `rate`, else the widest
/// band and the lowest centre it takes.
pub(crate) fn fits(rate: f32, low_hz: f32, high_hz: f32) -> Result<(), (f32, f32)> {
    let rate = f64::from(rate);
    let widest = 2.0 * (PASS * rate - MARGIN_HZ);
    let lowest_center = STOP * rate;
    let width = f64::from(high_hz - low_hz);
    let center = f64::from(f32::midpoint(low_hz, high_hz));
    if width <= widest && center >= lowest_center {
        Ok(())
    } else {
        Err((widest as f32, lowest_center as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_power(stream: &[f32], rate: f32, hz: f32) -> f32 {
        let (re, im) =
            stream
                .iter()
                .enumerate()
                .fold((0.0f64, 0.0f64), |(re, im), (index, &value)| {
                    let phase =
                        std::f64::consts::TAU * f64::from(hz) * index as f64 / f64::from(rate);
                    (
                        re + f64::from(value) * phase.cos(),
                        im + f64::from(value) * phase.sin(),
                    )
                });
        (4.0 * (re * re + im * im) / (stream.len() as f64).powi(2)) as f32
    }

    #[test]
    fn a_tone_keeps_its_level_a_quarter_of_the_rate_from_where_it_moved() {
        let input: Vec<Complex<f32>> = (0..120_000)
            .map(|index| {
                let phase = std::f32::consts::TAU * 1_520.0 * index as f32 / 12_000.0;
                Complex::new(phase.cos(), 0.0)
            })
            .collect();
        let mut narrow = Narrow::new(12_000.0, 16, 1_500.0);
        let mut stream = Vec::new();
        narrow.process(&input, &mut stream);
        assert_eq!(stream.len(), 7_500);
        let settled = &stream[1_000..];
        let at = 1_520.0 - shift_hz(750.0, 1_500.0);
        assert!((tone_power(settled, 750.0, at) - 1.0).abs() < 0.02);
        assert!(tone_power(settled, 750.0, 750.0 / 2.0 - at) < 1e-6);
    }

    #[test]
    fn a_band_too_wide_or_too_low_is_refused() {
        assert!(fits(750.0, 1_400.0, 1_600.0).is_ok());
        assert_eq!(fits(750.0, 1_200.0, 1_600.0), Err((280.0, 225.0)));
        assert!(fits(750.0, 50.0, 200.0).is_err());
    }
}
