pub(crate) mod bfield;
pub(crate) mod burst;
mod capabilities;
pub(crate) mod g726;
mod identity;
pub(crate) mod mac;
mod station;
mod voice;
pub(crate) mod wideband;

use std::sync::LazyLock;

use burst::{Burst, Detector, INPUT_RATE_HZ, OCCUPIED_BANDWIDTH_HZ};
use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_lowpass};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, DectParams,
    DectSpan,
};
use wideband::Wideband;

use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_rate};

const CHANNEL_TAPS: usize = 63;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "dect".to_owned(),
    name: "DECT".to_owned(),
    summary: "DECT cordless phone survey and voice".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: OCCUPIED_BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    decoder_kind: Some("dect".to_owned()),
    ..ChannelDescriptor::default()
});

enum Front {
    Carrier(Box<Detector>),
    Band(Box<Wideband>),
}

impl Front {
    fn new(params: DectParams, frequency_hz: f64) -> Result<Self, ChannelError> {
        let (rfp, pp) = (params.sides.accepts_rfp(), params.sides.accepts_pp());
        Ok(match params.span {
            DectSpan::Carrier => Self::Carrier(Box::new(Detector::new(
                rfp,
                pp,
                params.band.carrier_at(frequency_hz),
            ))),
            DectSpan::Band => {
                Self::Band(Box::new(Wideband::new(params.band, frequency_hz, rfp, pp)?))
            }
        })
    }

    fn set_sides(&mut self, rfp: bool, pp: bool) {
        match self {
            Self::Carrier(detector) => detector.set_sides(rfp, pp),
            Self::Band(wideband) => wideband.set_sides(rfp, pp),
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Burst>) {
        match self {
            Self::Carrier(detector) => detector.process(iq, out),
            Self::Band(wideband) => wideband.process(iq, out),
        }
    }
}

pub struct DectChannel {
    front: Front,
    tracker: station::Tracker,
    playout: voice::Playout,
    bursts: Vec<Burst>,
    params: DectParams,
    frequency_hz: f64,
}

fn params(settings: &ChannelSettings) -> Result<&DectParams, ChannelError> {
    match &settings.params {
        ChannelParams::Dect(p) => Ok(p),
        other => Err(ChannelError::InvalidSettings(format!(
            "dect channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn input_rate(params: &DectParams) -> f64 {
    match params.span {
        DectSpan::Carrier => INPUT_RATE_HZ,
        DectSpan::Band => wideband::input_rate(params.band),
    }
}

pub(crate) fn occupied_band(params: &DectParams) -> (f64, f64) {
    let half = match params.span {
        DectSpan::Carrier => OCCUPIED_BANDWIDTH_HZ / 2.0,
        DectSpan::Band => wideband::half_span(params.band),
    };
    (-half, half)
}

pub(crate) fn channel_filter(params: &DectParams) -> ChannelFilter {
    match params.span {
        DectSpan::Carrier => ChannelFilter::Symmetric(Decimator::new(
            &design_lowpass(CHANNEL_TAPS, OCCUPIED_BANDWIDTH_HZ / 2.0 / INPUT_RATE_HZ),
            1,
        )),
        DectSpan::Band => ChannelFilter::Passthrough,
    }
}

impl ChannelRx for DectChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = *params(&settings)?;
        let rate = input_rate(&params);
        check_rate(ctx, &DESCRIPTOR, rate)?;
        Ok(Self {
            front: Front::new(params, settings.frequency_hz)?,
            tracker: station::Tracker::new(params.band),
            playout: voice::Playout::new(rate),
            bursts: Vec::new(),
            params,
            frequency_hz: settings.frequency_hz,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = *params(&settings)?;
        let rebuilt = params.span != self.params.span
            || params.band != self.params.band
            || settings.frequency_hz != self.frequency_hz;
        if rebuilt {
            if input_rate(&params) != input_rate(&self.params) {
                return Err(ChannelError::InvalidSettings(
                    "DECT span changes the input rate; rebuild the channel".to_owned(),
                ));
            }
            self.front = Front::new(params, settings.frequency_hz)?;
            self.tracker.clear();
            self.playout.reset();
        } else if params.sides != self.params.sides {
            self.front
                .set_sides(params.sides.accepts_rfp(), params.sides.accepts_pp());
            self.tracker.clear();
        }
        self.tracker.set_band(params.band);
        self.params = params;
        self.frequency_hz = settings.frequency_hz;
        Ok(())
    }

    fn retuned(&mut self) {
        self.tracker.clear();
        self.playout.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.bursts.clear();
        self.front.process(iq, &mut self.bursts);
        for burst in &self.bursts {
            if let Some(frame) = self.tracker.apply(burst, &mut self.playout) {
                out.events.push(DecoderEvent::Dect(frame));
            }
        }
        self.playout.advance(iq.len(), out);
    }
}

#[cfg(test)]
mod tests;
