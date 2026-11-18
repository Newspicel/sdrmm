//! What the guest exports do, apart from moving bytes through memory: they hand each call to the
//! plugin's decoder and encoder, the same traits a built-in implements. Kept off the ABI so it
//! runs, and is tested, natively too.

use sdrmm_decoder::{
    Channel, ChannelCtx, Decoder,
    encoder::{Encoder, EncoderParams, Modulator},
};
use sdrmm_wire::{
    ChannelParams, ChannelSettings, FeedEntry, FeedSpec, KindSpec,
    plugin::{
        ABI_VERSION, DecoderManifest, FeedRequest, Op, PluginFeed, PluginLayout, PluginManifest,
        PresentRequest, Reply, TuneReply, TuneRequest,
    },
    present::Presentation,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::Plugin;

/// Everything one plugin brings, built once when the guest first needs it.
pub struct Parts {
    decoder: Option<Box<dyn Decoder>>,
    encoder: Option<Box<dyn Encoder>>,
    processor: Option<sdrmm_wire::ProcessorType>,
    kinds: Vec<KindSpec>,
    feeds: Vec<FeedSpec>,
}

impl Parts {
    #[must_use]
    pub fn of<P: Plugin>() -> Self {
        let decoder = P::decoder();
        let mut kinds = decoder.as_ref().map_or_default(|decoder| decoder.kinds());
        kinds.extend(P::kinds());
        let feeds = decoder.as_ref().map_or_default(|decoder| decoder.feeds());
        Self {
            decoder,
            encoder: P::encoder(),
            processor: P::processor(),
            kinds,
            feeds,
        }
    }

    /// The array processor the plugin declares, if it declares one.
    #[must_use]
    pub const fn processor(&self) -> Option<&sdrmm_wire::ProcessorType> {
        self.processor.as_ref()
    }

    #[must_use]
    pub fn manifest(&self) -> PluginManifest {
        PluginManifest {
            abi: ABI_VERSION,
            kinds: self.kinds.iter().map(|spec| spec.entry.clone()).collect(),
            decoder: self.decoder.as_ref().map(|decoder| DecoderManifest {
                descriptor: decoder.descriptor().clone(),
                feeds: self.feeds.iter().map(plugin_feed).collect(),
            }),
            encoder: self
                .encoder
                .as_ref()
                .map(|encoder| encoder.descriptor().clone()),
            processor: self.processor.clone(),
        }
    }

    /// How an event of one of the plugin's kinds reads, a [`PresentRequest`] in.
    pub fn present(&self, request: &[u8]) -> Result<Presentation, String> {
        let request: PresentRequest = parse(request)?;
        let spec = self
            .kinds
            .iter()
            .find(|spec| spec.entry.kind == request.kind)
            .ok_or_else(|| format!("no presenter for {}", request.kind))?;
        spec.codec.present(&request.kind, &request.data)
    }

    /// A control call: `op` and its JSON in, a JSON [`Reply`] out.
    #[must_use]
    pub fn call(&self, op: i32, input: &[u8]) -> Vec<u8> {
        let Some(op) = Op::from_code(op) else {
            return json(&Reply::<()>::Err(format!("unknown op {op}")));
        };
        match op {
            Op::Layout => reply(self.layout(input)),
            Op::Normalize => reply(self.normalize(input)),
            Op::Filter => reply(self.filter(input)),
            Op::Tune => reply(self.tune(input)),
            Op::Feed => reply(self.feed(input)),
            Op::Width => reply(self.width(input)),
            Op::Compose => reply(self.compose(input)),
        }
    }

    /// A channel of the decoder fed at `rate`, a JSON [`ChannelSettings`] in.
    pub fn open(&self, rate: f64, settings: &[u8]) -> Result<Box<dyn Channel>, String> {
        let settings: ChannelSettings = parse(settings)?;
        self.decoder()?
            .open(ChannelCtx { input_rate: rate }, &settings)
            .map_err(|error| error.to_string())
    }

    /// A modulator of the encoder, its settings JSON in.
    pub fn modulate(&self, settings: &[u8]) -> Result<Box<dyn Modulator>, String> {
        let encoder = self.encoder()?;
        encoder
            .open(&encoder_params(encoder, settings)?)
            .map_err(|error| error.to_string())
    }

    fn decoder(&self) -> Result<&dyn Decoder, String> {
        self.decoder
            .as_deref()
            .ok_or_else(|| "the plugin brings no decoder".to_owned())
    }

    fn encoder(&self) -> Result<&dyn Encoder, String> {
        self.encoder
            .as_deref()
            .ok_or_else(|| "the plugin brings no encoder".to_owned())
    }

    fn params(&self, settings: Value) -> Result<ChannelParams, String> {
        Ok(ChannelParams {
            type_id: self.decoder()?.descriptor().type_id.clone(),
            settings,
        })
    }

    fn layout(&self, input: &[u8]) -> Result<PluginLayout, String> {
        let layout = self
            .decoder()?
            .layout(&self.params(parse(input)?)?)
            .map_err(|error| error.to_string())?;
        Ok(PluginLayout {
            input_rate_hz: layout.input_rate_hz,
            band: layout.band,
            audio_channels: layout.audio_channels,
        })
    }

    fn normalize(&self, input: &[u8]) -> Result<Value, String> {
        let params = self
            .decoder()?
            .normalize(&self.params(parse(input)?)?)
            .map_err(|error| error.to_string())?;
        Ok(params.settings)
    }

    fn filter(&self, input: &[u8]) -> Result<sdrmm_wire::FilterSpec, String> {
        self.decoder()?
            .filter(&self.params(parse(input)?)?)
            .map_err(|error| error.to_string())
    }

    fn tune(&self, input: &[u8]) -> Result<TuneReply, String> {
        let request: TuneRequest = parse(input)?;
        let tuned = self
            .decoder()?
            .tune(&self.params(request.settings)?, &request.signal)
            .map_err(|error| error.to_string())?;
        Ok(TuneReply {
            settings: tuned.params.settings,
            offset_hz: tuned.offset_hz,
        })
    }

    fn feed(&self, input: &[u8]) -> Result<Vec<u8>, String> {
        let request: FeedRequest = parse(input)?;
        let feed = self
            .feeds
            .iter()
            .find(|feed| feed.format == request.format)
            .ok_or_else(|| format!("no feed {}", request.format))?;
        feed.encoder.encode(&request.event)
    }

    fn width(&self, input: &[u8]) -> Result<f64, String> {
        let encoder = self.encoder()?;
        encoder
            .width_hz(&encoder_params(encoder, input)?)
            .map_err(|error| error.to_string())
    }

    fn compose(&self, input: &[u8]) -> Result<sdrmm_decoder::encoder::TxPayload, String> {
        self.encoder()?
            .compose(&parse(input)?)
            .map_err(|error| error.to_string())
    }
}

fn plugin_feed(feed: &FeedSpec) -> PluginFeed {
    PluginFeed {
        entry: FeedEntry {
            format: feed.format.clone(),
            name: feed.name.clone(),
            default_address: feed.default_address.clone(),
            kinds: feed.kinds.clone(),
        },
        heartbeat: feed.heartbeat.clone(),
    }
}

fn encoder_params(encoder: &dyn Encoder, settings: &[u8]) -> Result<EncoderParams, String> {
    Ok(EncoderParams {
        type_id: encoder.descriptor().type_id.clone(),
        settings: parse(settings)?,
    })
}

/// JSON from the host, an empty buffer reading as `null`.
pub(crate) fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    let bytes = if bytes.is_empty() { b"null" } else { bytes };
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}

pub(crate) fn json(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|error| {
        serde_json::to_vec(&Reply::<()>::Err(error.to_string())).unwrap_or_default()
    })
}

fn reply<T: Serialize>(result: Result<T, String>) -> Vec<u8> {
    json(&Reply::from(result))
}
