//! The one decoder API: what a decoder declares, how a channel runs, and the registries that
//! open both. Built-in decoders and plugins implement the same traits.

use std::{borrow::Cow, sync::Arc};

use num_complex::Complex;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, ChannelStatus, DecoderEvent, PositionFix};

pub mod aircraft;
pub mod audio_chain;
pub mod broadcast;
pub mod catalog;
pub mod encoder;
mod filter;
mod front;
pub mod if_stage;
pub mod link;
pub mod pacer;
pub mod registry;
pub mod signal_id;
pub mod symbols;
pub mod voice;

pub use audio_chain::{AudioChain, ClickProfile, DenoiseNets, click_profile};
pub use encoder::Encoder;
pub use filter::ChannelFilter;
pub use front::ChannelFront;
pub use registry::{
    ChannelLayout, Decoder, Follow, Native, Registry, TrunkGrant, TrunkPlacement, Trunking, Tuned,
    announced_placements, audio_channels, channel_filter, check, create, default_settings,
    descriptor, descriptors, input_rate, install_plugin_kinds, install_plugins, layout, normalize,
    occupied_band, plugin_refusal, recipe, recipes, registry, signal_catalog, trunking, tune,
};
pub use symbols::SymbolTap;

pub const AUDIO_RATE: u32 = sdrmm_wire::audio::AUDIO_RATE_HZ;

/// Makes `decoders` and `encoders` what this build compiles in. Startup calls it before anything
/// reads either registry; the first call wins, so tests may call it as often as they like.
pub fn install(decoders: fn() -> Vec<Arc<dyn Decoder>>, encoders: fn() -> Vec<Arc<dyn Encoder>>) {
    registry::install_built_ins(decoders);
    encoder::install_built_ins(encoders);
}

/// A channel's settings as its decoder's struct, refusing another decoder's.
pub fn settings_of<P: sdrmm_wire::DecoderSettings>(
    settings: &ChannelSettings,
) -> Result<P, ChannelError> {
    settings
        .params
        .decode()
        .map_err(ChannelError::InvalidSettings)
}

/// A serde default for a setting that starts on.
#[must_use]
pub const fn default_true() -> bool {
    true
}

/// The gain control that brings decoded voice to a steady listening level.
#[must_use]
pub fn voice_leveller() -> sdrmm_dsp::Agc {
    sdrmm_dsp::Agc::new(f64::from(AUDIO_RATE), 0.25, 0.005, 0.5, 100.0)
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("unknown channel type: {0}")]
    UnknownType(String),
    #[error("invalid settings: {0}")]
    InvalidSettings(String),
    #[error("invalid payload: {0}")]
    InvalidPayload(String),
    #[error("{0}")]
    Refused(&'static str),
    #[error("{0}")]
    Unsupported(String),
    #[error("type {0} is already built in")]
    BuiltIn(String),
    #[error("type {0} is loaded twice")]
    LoadedTwice(String),
}

#[derive(Clone, Copy, Debug)]
pub struct ChannelCtx {
    pub input_rate: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoPicture {
    pub width: u16,
    pub height: u16,
    pub luma: Vec<u8>,
    pub rgb: Vec<u8>,
}

impl VideoPicture {
    /// A grey picture: each colour channel carries `luma`.
    #[must_use]
    pub fn grey(width: u16, height: u16, luma: &[u8]) -> Self {
        Self {
            width,
            height,
            luma: luma.to_vec(),
            rgb: luma.iter().flat_map(|&value| [value; 3]).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    /// The decoder that drew it.
    pub source: Cow<'static, str>,
    pub mode: String,
    pub complete: bool,
    pub lines: u16,
    pub picture: VideoPicture,
}

#[derive(Default)]
pub struct ChannelOutputs {
    pub audio_pcm: Vec<f32>,
    pub audio_rate: u32,
    pub events: Vec<DecoderEvent>,
    pub video: Vec<VideoPicture>,
    pub images: Vec<DecodedImage>,
    pub symbols: SymbolTap,
    pub status: Option<ChannelStatus>,
}

impl ChannelOutputs {
    pub fn reset(&mut self) {
        self.audio_pcm.clear();
        self.audio_rate = 0;
        self.events.clear();
        self.video.clear();
        self.images.clear();
        self.symbols.clear();
        self.status = None;
    }
}

/// A channel whose decoder is built with [`Native`]: its static descriptor and how to open it.
pub trait NativeChannel: Channel + Sized {
    fn descriptor() -> &'static ChannelDescriptor;

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError>;
}

/// A running decoder channel, built in or loaded as a plugin.
pub trait Channel: Send {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError>;

    fn retuned(&mut self) {}

    fn position_changed(&mut self, _fix: Option<&PositionFix>) {}

    /// When the first sample of the next `process` call was captured, by the wall clock. Called
    /// only while the source's clock is known.
    fn captured_at(&mut self, _time: std::time::SystemTime) {}

    /// Where the receiver's own LO artifact falls in this channel's baseband, if it falls in it.
    ///
    /// A zero-IF front end's DC term is a carrier like any other and cannot be told apart from a
    /// real one by its shape, so anything measuring the passband has to be told to disregard it.
    fn lo_artifact_at(&mut self, _offset_hz: Option<f64>) {}

    fn needs_gated_input(&self) -> bool {
        true
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs);

    fn rejected(&self) -> u32 {
        0
    }

    fn skipped(&self) -> u32 {
        0
    }
}

/// Refuses a `what` setting outside `range`, in `unit`.
pub fn check_range<T: PartialOrd + std::fmt::Display + Copy>(
    what: &str,
    unit: &str,
    value: T,
    range: std::ops::RangeInclusive<T>,
) -> Result<(), ChannelError> {
    if range.contains(&value) {
        return Ok(());
    }
    Err(ChannelError::InvalidSettings(format!(
        "{what} must be in [{}, {}] {unit}, got {value}",
        range.start(),
        range.end()
    )))
}

/// Refuses an input rate other than the one `descriptor` declares.
pub fn check_input_rate(
    ctx: ChannelCtx,
    descriptor: &ChannelDescriptor,
) -> Result<(), ChannelError> {
    check_rate(ctx, descriptor, descriptor.input_rate_hz)
}

/// Refuses an input rate other than `expected`.
pub fn check_rate(
    ctx: ChannelCtx,
    descriptor: &ChannelDescriptor,
    expected: f64,
) -> Result<(), ChannelError> {
    if ctx.input_rate == expected {
        Ok(())
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "{} expects {expected} Hz input, engine supplied {} Hz",
            descriptor.type_id, ctx.input_rate
        )))
    }
}

#[cfg(test)]
mod tests;
