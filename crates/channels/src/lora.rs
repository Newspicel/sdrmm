mod crypto;
pub(crate) mod lorawan;
pub(crate) mod meshcore;
pub(crate) mod meshtastic;
pub(crate) mod phy;
mod protocol;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, LoraFrame,
    LoraParams,
};

use self::phy::{Decoder, Frame, Layout};
use self::protocol::Keyring;
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_rate, datalink,
};

pub(crate) const OVERSAMPLING: f64 = 2.0;
const PASSBAND: f64 = 0.75;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    let bandwidth = LoraParams::default().bandwidth.hz();
    ChannelDescriptor {
        type_id: "lora".to_owned(),
        name: "LoRa".to_owned(),
        summary: "LoRa, LoRaWAN, Meshtastic and MeshCore".to_owned(),
        family: DecoderFamily::Utility,
        bandwidth_hz: bandwidth,
        input_rate_hz: bandwidth * OVERSAMPLING,
        has_audio: false,
        decoder_kind: Some("lora".to_owned()),
        ..ChannelDescriptor::default()
    }
});

pub struct LoraChannel {
    params: LoraParams,
    decoder: Decoder,
    keys: Keyring,
    frames: Vec<Frame>,
}

fn params(settings: &ChannelSettings) -> Result<&LoraParams, ChannelError> {
    match &settings.params {
        ChannelParams::Lora(params) => Ok(params),
        other => Err(ChannelError::InvalidSettings(format!(
            "LoRa channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn input_rate(params: &LoraParams) -> f64 {
    params.bandwidth.hz() * OVERSAMPLING
}

pub(crate) fn occupied_band(params: &LoraParams) -> (f64, f64) {
    let half = params.bandwidth.hz() / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(params: &LoraParams) -> ChannelFilter {
    datalink::channel_filter(input_rate(params), params.bandwidth.hz() * PASSBAND)
}

fn layout(params: &LoraParams) -> Layout {
    Layout {
        bandwidth_hz: params.bandwidth.hz(),
        spreading_factors: params.spreading_factor.factors(),
        iq: params.iq,
        implicit_header: params.implicit_header,
    }
}

fn event(frame: &Frame, params: &LoraParams, keys: &Keyring) -> DecoderEvent {
    DecoderEvent::Lora(LoraFrame {
        spreading_factor: frame.spreading_factor,
        bandwidth_hz: params.bandwidth.hz(),
        coding_rate: frame.coding_rate,
        sync_word: frame.sync_word,
        implicit_header: frame.implicit_header,
        low_data_rate: frame.low_data_rate,
        inverted_iq: frame.inverted_iq,
        integrity: frame.integrity,
        fec_corrected: frame.fec_corrected,
        snr_db: frame.snr_db,
        frequency_error_hz: frame.frequency_error_hz,
        payload: datalink::hex(&frame.payload),
        decoded: frame
            .readable()
            .then(|| protocol::interpret(params.protocol, frame.sync_word, &frame.payload, keys))
            .flatten(),
    })
}

impl ChannelRx for LoraChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?.clone();
        params.validate().map_err(ChannelError::InvalidSettings)?;
        check_rate(ctx, &DESCRIPTOR, input_rate(&params))?;
        Ok(Self {
            decoder: Decoder::new(&layout(&params), settings.frequency_hz),
            keys: Keyring::new(&params.keys).map_err(ChannelError::InvalidSettings)?,
            frames: Vec::new(),
            params,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let next = params(&settings)?;
        next.validate().map_err(ChannelError::InvalidSettings)?;
        if layout(next) != layout(&self.params) {
            return Err(ChannelError::InvalidSettings(
                "LoRa radio settings need a rebuilt channel".to_owned(),
            ));
        }
        self.keys = Keyring::new(&next.keys).map_err(ChannelError::InvalidSettings)?;
        self.decoder.retune(settings.frequency_hz);
        self.params = next.clone();
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.frames.clear();
        self.decoder.process(iq, &mut self.frames);
        out.events.extend(
            self.frames
                .iter()
                .map(|frame| event(frame, &self.params, &self.keys)),
        );
    }
}
