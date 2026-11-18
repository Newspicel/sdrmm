use std::sync::Arc;

use sdrmm_decoder::{Channel, ChannelCtx, ChannelError, ChannelLayout, Decoder, Tuned};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DEFAULT_FREQUENCY_HZ, DecoderEvent,
    FeedEncoder, FeedSpec, FilterSpec, IdentSignal,
    plugin::{
        DecoderManifest, FeedRequest, Op, PluginFeed, PluginLayout, PluginManifest, TuneReply,
        TuneRequest,
    },
};

use crate::{channel::WasmChannel, module::PluginModule};

/// A plugin's decoder as the decoder registry sees it.
pub struct WasmDecoder {
    module: Arc<PluginModule>,
    descriptor: ChannelDescriptor,
    feeds: Vec<FeedSpec>,
}

impl WasmDecoder {
    /// The decoder `module` declares, if it declares one.
    #[must_use]
    pub fn of(module: &Arc<PluginModule>) -> Option<Self> {
        let decoder = module.manifest.decoder.as_ref()?;
        Some(Self {
            descriptor: describe(module, decoder),
            feeds: decoder
                .feeds
                .iter()
                .map(|feed| feed_spec(module, feed))
                .collect(),
            module: module.clone(),
        })
    }
}

fn describe(module: &PluginModule, decoder: &DecoderManifest) -> ChannelDescriptor {
    let mut descriptor = decoder.descriptor.clone();
    descriptor.identifiable = !descriptor.signatures.is_empty();
    if descriptor.emits.is_empty() {
        descriptor.emits = decoded_kinds(&module.manifest);
    }
    if descriptor.defaults.is_none() {
        descriptor.defaults = module
            .call::<serde_json::Value>(Op::Normalize, &serde_json::json!({}))
            .ok()
            .map(|settings| {
                ChannelSettings::new(
                    DEFAULT_FREQUENCY_HZ,
                    ChannelParams {
                        type_id: descriptor.type_id.clone(),
                        settings,
                    },
                )
            });
    }
    descriptor.ports = sdrmm_wire::patch::channel_ports(&descriptor);
    descriptor
}

/// The plugin's kinds its processor does not emit, which are the decoder's.
fn decoded_kinds(manifest: &PluginManifest) -> Vec<sdrmm_wire::EventKind> {
    let processed = manifest
        .processor
        .as_ref()
        .map_or_default(|processor| processor.emits.as_slice());
    manifest
        .kinds
        .iter()
        .map(|entry| entry.kind.clone())
        .filter(|kind| !processed.contains(kind))
        .collect()
}

fn feed_spec(module: &Arc<PluginModule>, feed: &PluginFeed) -> FeedSpec {
    FeedSpec {
        format: feed.entry.format.clone(),
        name: feed.entry.name.clone(),
        default_address: feed.entry.default_address.clone(),
        kinds: feed.entry.kinds.clone(),
        heartbeat: feed.heartbeat.clone(),
        encoder: Arc::new(PluginFeedEncoder {
            module: module.clone(),
            format: feed.entry.format.clone(),
        }),
    }
}

/// Frames events in one of a plugin's feed formats by asking the plugin.
struct PluginFeedEncoder {
    module: Arc<PluginModule>,
    format: String,
}

impl FeedEncoder for PluginFeedEncoder {
    fn encode(&self, event: &DecoderEvent) -> Result<Vec<u8>, String> {
        let request = FeedRequest {
            format: self.format.clone(),
            event: event.clone(),
        };
        self.module.call(Op::Feed, &request)
    }
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

    fn ask<T: serde::de::DeserializeOwned>(
        &self,
        op: Op,
        params: &ChannelParams,
    ) -> Result<T, ChannelError> {
        self.check_type(params)?;
        self.module.call(op, &params.settings).map_err(invalid)
    }
}

impl Decoder for WasmDecoder {
    fn descriptor(&self) -> &ChannelDescriptor {
        &self.descriptor
    }

    fn open(
        &self,
        ctx: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn Channel>, ChannelError> {
        self.check_type(&settings.params)?;
        Ok(Box::new(WasmChannel::open(
            self.module.clone(),
            ctx.input_rate,
            settings,
        )?))
    }

    fn layout(&self, params: &ChannelParams) -> Result<ChannelLayout, ChannelError> {
        let layout: PluginLayout = self.ask(Op::Layout, params)?;
        Ok(ChannelLayout {
            input_rate_hz: layout.input_rate_hz,
            band: layout.band,
            audio_channels: layout.audio_channels,
        })
    }

    fn filter(&self, params: &ChannelParams) -> Result<FilterSpec, ChannelError> {
        self.ask(Op::Filter, params)
    }

    fn normalize(&self, params: &ChannelParams) -> Result<ChannelParams, ChannelError> {
        Ok(ChannelParams {
            type_id: params.type_id.clone(),
            settings: self.ask(Op::Normalize, params)?,
        })
    }

    fn feeds(&self) -> Vec<FeedSpec> {
        self.feeds.clone()
    }

    fn tune(&self, params: &ChannelParams, signal: &IdentSignal) -> Result<Tuned, ChannelError> {
        self.check_type(params)?;
        let request = TuneRequest {
            settings: params.settings.clone(),
            signal: signal.clone(),
        };
        let reply: TuneReply = self.module.call(Op::Tune, &request).map_err(invalid)?;
        Ok(Tuned {
            params: ChannelParams {
                type_id: params.type_id.clone(),
                settings: reply.settings,
            },
            offset_hz: reply.offset_hz,
        })
    }
}
