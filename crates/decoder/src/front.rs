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
