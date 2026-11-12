use std::sync::Arc;

use sdrmm_channels::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelLayout, ChannelRx, ChannelTx, Decoder,
    TxPayload,
};
use sdrmm_dsp::{Decimator, design_lowpass};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DEFAULT_FREQUENCY_HZ, EventKind, KindCodec,
    KindSpec,
    plugin::{Op, PluginLayout, PresentRequest},
    present::Presentation,
};

use crate::{channel::WasmChannel, module::PluginModule, transmit::WasmTx};

const FILTER_TAPS: usize = 65;

/// A plugin as the decoder registry sees it.
pub struct WasmDecoder {
    module: Arc<PluginModule>,
    descriptor: ChannelDescriptor,
}

impl WasmDecoder {
    pub fn new(module: Arc<PluginModule>) -> Self {
        let descriptor = describe(&module);
        Self { module, descriptor }
    }
}

fn describe(module: &PluginModule) -> ChannelDescriptor {
    let manifest = &module.manifest;
    let mut descriptor = manifest.descriptor.clone();
    descriptor.settings = manifest.form.clone();
    descriptor.identifiable = !descriptor.signatures.is_empty();
    if descriptor.emits.is_empty() {
        descriptor.emits = manifest
            .kinds
            .iter()
            .map(|kind| kind.kind.clone())
            .collect();
    }
    descriptor.defaults = module
        .call::<serde_json::Value>(Op::Normalize, &serde_json::json!({}))
        .ok()
        .map(|settings| {
            ChannelSettings::new(
                manifest.home_hz.unwrap_or(DEFAULT_FREQUENCY_HZ),
                ChannelParams {
                    type_id: descriptor.type_id.clone(),
                    settings,
                },
            )
        });
    descriptor.ports = sdrmm_wire::patch::channel_ports(&descriptor);
    descriptor
}

fn invalid(error: String) -> ChannelError {
    ChannelError::InvalidSettings(error)
}

impl WasmDecoder {
    fn check_type(&self, params: &ChannelParams) -> Result<(), ChannelError> {
        if params.type_id() == self.descriptor.type_id {
            Ok(())
        } else {
            Err(invalid(format!(
                "{} decoder got {} settings",
                self.descriptor.type_id,
                params.type_id()
            )))
        }
    }
}

impl Decoder for WasmDecoder {
    fn descriptor(&self) -> &ChannelDescriptor {
        &self.descriptor
    }

    fn create_rx(
        &self,
        ctx: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn ChannelRx>, ChannelError> {
        self.check_type(&settings.params)?;
        Ok(Box::new(WasmChannel::open(
            self.module.clone(),
            ctx.input_rate,
            &settings.params.settings,
        )?))
    }

    fn create_tx(
        &self,
        ctx: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn ChannelTx>, ChannelError> {
        self.check_type(&settings.params)?;
        if !self.descriptor.can_transmit {
            return Err(ChannelError::NoTransmitter(self.descriptor.type_id.clone()));
        }
        Ok(Box::new(WasmTx::open(
            self.module.clone(),
            ctx.input_rate,
            &settings.params.settings,
        )?))
    }

    /// A plugin that declares a message form gets the filled form as UTF-8 JSON in a frame.
    fn compose(&self, message: &serde_json::Value) -> Result<TxPayload, ChannelError> {
        if self.descriptor.message.is_empty() {
            return Err(ChannelError::InvalidPayload(format!(
                "{} sends no messages",
                self.descriptor.type_id
            )));
        }
        serde_json::to_vec(message)
            .map(TxPayload::Frame)
            .map_err(|error| ChannelError::InvalidPayload(error.to_string()))
    }

    fn layout(&self, params: &ChannelParams) -> Result<ChannelLayout, ChannelError> {
        self.check_type(params)?;
        let layout: PluginLayout = self
            .module
            .call(Op::Layout, &params.settings)
            .map_err(invalid)?;
        Ok(ChannelLayout {
            input_rate_hz: layout.input_rate_hz,
            band: layout.band,
            audio_channels: layout.audio_channels,
        })
    }

    /// A lowpass over the plugin's band; anything finer the plugin filters itself.
    fn filter(&self, params: &ChannelParams) -> Result<ChannelFilter, ChannelError> {
        let layout = self.layout(params)?;
        let half = layout.band.0.abs().max(layout.band.1.abs());
        let cutoff = half / layout.input_rate_hz;
        if !(cutoff.is_finite() && cutoff > 0.0 && cutoff < 0.5) {
            return Ok(ChannelFilter::Passthrough);
        }
        Ok(ChannelFilter::Symmetric(Decimator::new(
            &design_lowpass(FILTER_TAPS, cutoff),
            1,
        )))
    }

    fn normalize(&self, params: &ChannelParams) -> Result<ChannelParams, ChannelError> {
        self.check_type(params)?;
        let settings = self
            .module
            .call(Op::Normalize, &params.settings)
            .map_err(invalid)?;
        Ok(ChannelParams {
            type_id: params.type_id.clone(),
            settings,
        })
    }

    fn kinds(&self) -> Vec<KindSpec> {
        let codec: Arc<dyn KindCodec> = Arc::new(PluginCodec {
            module: self.module.clone(),
        });
        self.module
            .manifest
            .kinds
            .iter()
            .map(|entry| KindSpec::with_codec(entry.clone(), codec.clone()))
            .collect()
    }
}

/// Presents a plugin's events by asking the plugin.
struct PluginCodec {
    module: Arc<PluginModule>,
}

impl KindCodec for PluginCodec {
    fn present(&self, kind: &EventKind, data: &serde_json::Value) -> Result<Presentation, String> {
        self.module.call(
            Op::Present,
            &PresentRequest {
                kind: kind.clone(),
                data: data.clone(),
            },
        )
    }
}
