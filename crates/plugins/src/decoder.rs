use std::sync::Arc;

use sdrmm_channels::{ChannelCtx, ChannelError, ChannelFilter, ChannelLayout, ChannelRx, Decoder};
use sdrmm_dsp::{Decimator, design_lowpass};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DEFAULT_FREQUENCY_HZ,
    plugin::{DecoderManifest, Op, PluginLayout, PluginManifest},
};

use crate::{channel::WasmChannel, module::PluginModule};

const FILTER_TAPS: usize = 65;

/// A plugin's decoder as the decoder registry sees it.
pub struct WasmDecoder {
    module: Arc<PluginModule>,
    descriptor: ChannelDescriptor,
}

impl WasmDecoder {
    /// The decoder `module` declares, if it declares one.
    #[must_use]
    pub fn of(module: &Arc<PluginModule>) -> Option<Self> {
        let decoder = module.manifest.decoder.as_ref()?;
        Some(Self {
            descriptor: describe(module, decoder),
            module: module.clone(),
        })
    }
}

fn describe(module: &PluginModule, decoder: &DecoderManifest) -> ChannelDescriptor {
    let mut descriptor = decoder.descriptor.clone();
    descriptor.settings = decoder.form.clone();
    descriptor.identifiable = !descriptor.signatures.is_empty();
    if descriptor.emits.is_empty() {
        descriptor.emits = decoded_kinds(&module.manifest);
    }
    descriptor.defaults = module
        .call::<serde_json::Value>(Op::Normalize, &serde_json::json!({}))
        .ok()
        .map(|settings| {
            ChannelSettings::new(
                decoder.home_hz.unwrap_or(DEFAULT_FREQUENCY_HZ),
                ChannelParams {
                    type_id: descriptor.type_id.clone(),
                    settings,
                },
            )
        });
    descriptor.ports = sdrmm_wire::patch::channel_ports(&descriptor);
    descriptor
}

/// The plugin's kinds its processor does not emit, which are the decoder's.
fn decoded_kinds(manifest: &PluginManifest) -> Vec<sdrmm_wire::EventKind> {
    let processed = manifest
        .processor
        .as_ref()
        .map(|processor| processor.emits.as_slice())
        .unwrap_or_default();
    manifest
        .kinds
        .iter()
        .map(|entry| entry.kind.clone())
        .filter(|kind| !processed.contains(kind))
        .collect()
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
}
