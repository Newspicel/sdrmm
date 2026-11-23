use num_complex::Complex;
use sdrmm_dsp::{Decimator, FastFir, FirC, design_lowpass, design_lowpass_kaiser};
use sdrmm_wire::FilterSpec;

use crate::ChannelError;

/// Taps from which a lowpass that keeps its rate runs as an FFT convolution.
const OVERLAP_SAVE_TAPS: usize = 127;
/// Rates from which a channel's blocks are long enough for that convolution to pay.
const OVERLAP_SAVE_RATE_HZ: f64 = 2_000_000.0;

/// A channel's filter as it runs, built from the [`FilterSpec`] its decoder asks for.
pub enum ChannelFilter {
    Symmetric(Decimator),
    Sideband(FirC),
    /// A long lowpass on a wide channel, where overlap-save costs a fraction of direct taps.
    OverlapSave(FastFir),
    Passthrough,
}

impl ChannelFilter {
    /// The filter `spec` describes, for a decoder fed at `input_rate_hz`. Refuses a spec whose
    /// edges do not fit below the Nyquist rate it runs at.
    pub fn new(spec: &FilterSpec, input_rate_hz: f64) -> Result<Self, ChannelError> {
        match *spec {
            FilterSpec::Passthrough => Ok(Self::Passthrough),
            FilterSpec::Lowpass {
                taps,
                cutoff_hz,
                decimation,
                stopband_db,
            } => {
                check_taps(taps)?;
                if decimation == 0 {
                    return Err(refused("a lowpass decimates by at least 1"));
                }
                let cutoff = normalized(cutoff_hz, input_rate_hz * decimation as f64)?;
                let prototype = match stopband_db {
                    Some(db) if db.is_finite() && db > 0.0 => {
                        design_lowpass_kaiser(taps, cutoff, db)
                    }
                    Some(db) => return Err(refused(&format!("stopband {db} dB"))),
                    None => design_lowpass(taps, cutoff),
                };
                if decimation == 1
                    && taps >= OVERLAP_SAVE_TAPS
                    && input_rate_hz >= OVERLAP_SAVE_RATE_HZ
                {
                    return Ok(Self::OverlapSave(FastFir::real(&prototype)));
                }
                Ok(Self::Symmetric(Decimator::new(&prototype, decimation)))
            }
            FilterSpec::Band {
                taps,
                low_hz,
                high_hz,
            } => {
                check_taps(taps)?;
                if low_hz.partial_cmp(&high_hz) != Some(std::cmp::Ordering::Less) {
                    return Err(refused(&format!("band {low_hz}..{high_hz} Hz")));
                }
                let half = normalized((high_hz - low_hz) / 2.0, input_rate_hz)?;
                let centre = f64::midpoint(high_hz, low_hz) / input_rate_hz;
                Ok(Self::Sideband(FirC::from_lowpass(
                    &design_lowpass(taps, half),
                    centre,
                )))
            }
        }
    }

    /// How far the filter brings the rate down. A filter that decimates takes over the last
    /// step of the downconversion: the converter feeding it runs at the decoder's rate times
    /// this and guards only the decoder's band.
    #[must_use]
    pub fn decimation(&self) -> usize {
        match self {
            Self::Symmetric(filter) => filter.factor(),
            Self::Sideband(_) | Self::OverlapSave(_) | Self::Passthrough => 1,
        }
    }

    pub fn reset(&mut self) {
        match self {
            Self::Symmetric(filter) => filter.reset(),
            Self::Sideband(filter) => filter.reset(),
            Self::OverlapSave(filter) => filter.reset(),
            Self::Passthrough => {}
        }
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        match self {
            Self::Symmetric(f) => f.process(input, out),
            Self::Sideband(f) => f.process(input, out),
            Self::OverlapSave(f) => f.process(input, out),
            Self::Passthrough => {
                out.clear();
                out.extend_from_slice(input);
            }
        }
    }
}

const MAX_TAPS: usize = 4_096;

fn refused(why: &str) -> ChannelError {
    ChannelError::InvalidSettings(format!("channel filter: {why}"))
}

fn check_taps(taps: usize) -> Result<(), ChannelError> {
    if (1..=MAX_TAPS).contains(&taps) {
        Ok(())
    } else {
        Err(refused(&format!("{taps} taps, not 1 to {MAX_TAPS}")))
    }
}

/// `hz` as a fraction of `rate`, refused unless it lies strictly between zero and Nyquist.
fn normalized(hz: f64, rate: f64) -> Result<f64, ChannelError> {
    let cutoff = hz / rate;
    if cutoff.is_finite() && cutoff > 0.0 && cutoff < 0.5 {
        Ok(cutoff)
    } else {
        Err(refused(&format!("{hz} Hz does not fit a {rate} Hz rate")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_lowpass_convolves_by_fft_and_matches_direct_taps_across_blocks() {
        let rate = 64_000_000.0 / 7.0;
        let mut fast = ChannelFilter::new(&FilterSpec::lowpass(127, 4_000_000.0), rate).unwrap();
        assert!(matches!(fast, ChannelFilter::OverlapSave(_)));
        let mut direct = Decimator::new(&design_lowpass(127, 4_000_000.0 / rate), 1);
        let input: Vec<_> = (0..5_000)
            .map(|i| Complex::new((i as f32 * 0.31).sin(), (i as f32 * 0.77).cos()))
            .collect();
        let (mut actual, mut expected) = (Vec::new(), Vec::new());
        for block in input.chunks(1_237) {
            fast.process(block, &mut actual);
            direct.process(block, &mut expected);
            assert_eq!(actual.len(), expected.len());
            for (a, b) in actual.iter().zip(&expected) {
                assert!((a - b).norm() < 1e-4, "{a} {b}");
            }
        }
    }

    fn rms(filter: &mut ChannelFilter, offset_hz: f64, rate: f64) -> f32 {
        let tone: Vec<Complex<f32>> = (0..4_800)
            .map(|n| {
                let phase = std::f64::consts::TAU * offset_hz * f64::from(n) / rate;
                Complex::new(phase.cos() as f32, phase.sin() as f32)
            })
            .collect();
        let mut out = Vec::new();
        filter.process(&tone, &mut out);
        let tail = &out[out.len() / 2..];
        (tail.iter().map(Complex::norm_sqr).sum::<f32>() / tail.len() as f32).sqrt()
    }

    #[test]
    fn a_lowpass_passes_its_band_and_stops_the_rest() {
        let mut filter =
            ChannelFilter::new(&FilterSpec::lowpass(129, 6_000.0), 48_000.0).expect("a lowpass");
        assert!(rms(&mut filter, 1_000.0, 48_000.0) > 0.9);
        filter.reset();
        assert!(rms(&mut filter, 15_000.0, 48_000.0) < 0.01);
    }

    #[test]
    fn a_band_passes_one_side_of_the_channel_only() {
        let spec = FilterSpec::band(255, 300.0, 2_700.0);
        let mut filter = ChannelFilter::new(&spec, 48_000.0).expect("a band");
        assert!(rms(&mut filter, 1_500.0, 48_000.0) > 0.9);
        filter.reset();
        assert!(rms(&mut filter, -1_500.0, 48_000.0) < 0.01);
    }

    #[test]
    fn a_decimating_lowpass_runs_at_its_wide_rate() {
        let spec = FilterSpec::Lowpass {
            taps: 63,
            cutoff_hz: 100_000.0,
            decimation: 4,
            stopband_db: Some(80.0),
        };
        let filter = ChannelFilter::new(&spec, 250_000.0).expect("a decimator");
        assert_eq!(filter.decimation(), 4);
        assert_eq!(spec.decimation(), 4);
    }

    #[test]
    fn a_spec_past_nyquist_or_without_taps_is_refused() {
        for spec in [
            FilterSpec::lowpass(65, 30_000.0),
            FilterSpec::lowpass(0, 1_000.0),
            FilterSpec::lowpass(65, f64::NAN),
            FilterSpec::band(65, 2_000.0, 1_000.0),
            FilterSpec::Lowpass {
                taps: 65,
                cutoff_hz: 1_000.0,
                decimation: 0,
                stopband_db: None,
            },
        ] {
            assert!(
                matches!(
                    ChannelFilter::new(&spec, 48_000.0),
                    Err(ChannelError::InvalidSettings(_))
                ),
                "{spec:?} must be refused"
            );
        }
        assert!(matches!(
            ChannelFilter::new(&FilterSpec::Passthrough, 48_000.0),
            Ok(ChannelFilter::Passthrough)
        ));
    }
}
