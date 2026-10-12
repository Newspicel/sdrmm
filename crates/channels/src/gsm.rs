pub(crate) mod burst;
pub(crate) mod coding;
mod dedicated;
mod equalizer;
mod fcch;
pub(crate) mod frames;
mod freq_list;
mod identity;
mod monitor;
mod receiver;
mod rr;

use std::sync::LazyLock;

use burst::INPUT_RATE_HZ;
use monitor::Monitor;
use num_complex::Complex;
use receiver::Receiver;
use sdrmm_dsp::{Decimator, design_lowpass};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily,
    GSM_CHANNEL_SPACING_HZ, GsmParams,
};

use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};

const FILTER_TAPS: usize = 47;
const FILTER_CUTOFF_HZ: f64 = 120_000.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "gsm".to_owned(),
    name: "GSM".to_owned(),
    summary: "GSM cell broadcast, paging and signalling".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: GSM_CHANNEL_SPACING_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    decoder_kind: Some("gsm".to_owned()),
    ..ChannelDescriptor::default()
});

pub(crate) fn occupied_band() -> (f64, f64) {
    (-GSM_CHANNEL_SPACING_HZ / 2.0, GSM_CHANNEL_SPACING_HZ / 2.0)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(FILTER_TAPS, FILTER_CUTOFF_HZ / INPUT_RATE_HZ),
        1,
    ))
}

fn params(settings: &ChannelSettings) -> Result<GsmParams, ChannelError> {
    match &settings.params {
        ChannelParams::Gsm(p) => Ok(*p),
        other => Err(ChannelError::InvalidSettings(format!(
            "gsm channel got {} params",
            other.type_id()
        ))),
    }
}

pub struct GsmChannel {
    receiver: Receiver,
    monitor: Monitor,
    frequency_hz: f64,
}

impl ChannelRx for GsmChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Ok(Self {
            receiver: Receiver::new(),
            monitor: Monitor::new(params(&settings)?, settings.frequency_hz),
            frequency_hz: settings.frequency_hz,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.monitor.set_params(params(&settings)?);
        if settings.frequency_hz != self.frequency_hz {
            self.frequency_hz = settings.frequency_hz;
            self.retuned();
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.receiver.reset();
        self.monitor.retune(self.frequency_hz);
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.receiver.process(iq, &mut self.monitor);
        out.events
            .extend(self.monitor.drain().map(DecoderEvent::Gsm));
    }

    fn rejected(&self) -> u32 {
        self.monitor.rejected()
    }
}

#[cfg(test)]
mod tests;
