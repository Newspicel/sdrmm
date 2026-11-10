use num_complex::Complex;
use sdrmm_dsp::Ddc;
use sdrmm_wire::ChannelParams;

use crate::{ChannelError, ChannelFilter, channel_filter, input_rate};

/// Everything between IQ at some rate and a channel's decoder: a converter to the rate the
/// channel's filter takes and the filter, which leaves the decoder's input rate. A filter that
/// decimates takes over the converter's last step, so the converter guards only the decoder's
/// band.
pub struct ChannelFront {
    pub ddc: Ddc,
    pub filter: ChannelFilter,
    /// The converter's output rate, at which a stage between the two runs.
    pub wide_rate: f64,
    wide: Vec<Complex<f32>>,
}

impl ChannelFront {
    pub fn new(params: &ChannelParams, rate: f64, offset_hz: f64) -> Result<Self, ChannelError> {
        let band_rate = input_rate(params);
        let filter = channel_filter(params)?;
        let wide_rate = band_rate * filter.decimation() as f64;
        let ddc = Ddc::for_band(rate, wide_rate, band_rate, offset_hz)
            .map_err(|e| ChannelError::InvalidSettings(e.to_string()))?;
        Ok(Self {
            ddc,
            filter,
            wide_rate,
            wide: Vec::new(),
        })
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        self.ddc.process(iq, &mut self.wide);
        self.filter.process(&self.wide, out);
    }

    pub fn reset(&mut self) {
        self.ddc.reset();
        self.filter.reset();
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;
    use crate::{nfm::NfmParams, wfm::WfmParams};

    fn tone(hz: f64, rate: f64, len: usize) -> Vec<Complex<f32>> {
        (0..len)
            .map(|n| Complex::from_polar(1.0, (TAU * (hz * n as f64 / rate).fract()) as f32))
            .collect()
    }

    fn level_db(front: &mut ChannelFront, iq: &[Complex<f32>]) -> f64 {
        let mut out = Vec::new();
        let mut all = Vec::new();
        for block in iq.chunks(4_096) {
            front.process(block, &mut out);
            all.extend_from_slice(&out);
        }
        let settled = &all[all.len() / 2..];
        let power = settled.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>();
        10.0 * (power / settled.len() as f64).log10()
    }

    #[test]
    fn a_decimating_filter_takes_the_last_halving_and_still_stops_what_folds_back() {
        let params = ChannelParams::of(&WfmParams::default()).expect("wfm settings");
        let rate = 1_920_000.0;
        let mut front = ChannelFront::new(&params, rate, 0.0).expect("front");
        assert_eq!(front.filter.decimation(), 2);
        assert_eq!(front.wide_rate, 480_000.0);
        let len = 960_000;
        let mut out = Vec::new();
        front.process(&tone(0.0, rate, len), &mut out);
        assert!(
            (out.len() as f64 - len as f64 / 8.0).abs() <= 2.0,
            "{} out",
            out.len()
        );
        for (hz, passes) in [
            (20_000.0, true),
            (-85_000.0, true),
            (130_000.0, false),
            (-200_000.0, false),
            (250_000.0, false),
            (455_000.0, false),
        ] {
            let mut front = ChannelFront::new(&params, rate, 0.0).expect("front");
            let db = level_db(&mut front, &tone(hz, rate, len));
            if passes {
                assert!(db.abs() < 0.05, "{hz} Hz passes at {db} dB");
            } else {
                assert!(db < -99.0, "{hz} Hz leaks at {db} dB");
            }
        }
    }

    #[test]
    fn a_filter_at_the_decoders_rate_leaves_the_converter_whole() {
        let params = ChannelParams::of(&NfmParams::default()).expect("nfm settings");
        let front = ChannelFront::new(&params, 1_920_000.0, 0.0).expect("front");
        assert_eq!(front.filter.decimation(), 1);
        assert_eq!(front.wide_rate, input_rate(&params));
    }
}
