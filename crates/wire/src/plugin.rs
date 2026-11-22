//! The contract between the server and a WASM plugin.
//!
//! A plugin is a core WebAssembly module that brings any of a decoder, an encoder and an array
//! processor, each through exports of its own. Control calls exchange JSON in the guest's memory;
//! the per-block call exchanges raw samples through buffers the guest owns, so the hot path
//! copies samples and allocates nothing.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    channel::{ChannelDescriptor, ChannelStatus},
    decode::{DecoderEvent, EventKind, EventKindEntry, FeedEntry, IdentSignal},
    encoder::EncoderDescriptor,
    present::Presentation,
    processor::ProcessorType,
};

pub mod encoder;
mod output;
pub mod processor;

pub use output::{OutputHeader, Picture, PictureImage, plane_code, plane_of, read_pictures};

/// The ABI the SDK builds plugins for. A host speaking an older one refuses them, so a plugin
/// never sends that host a symbol plane it cannot read.
pub const ABI_VERSION: u32 = 5;
/// The oldest ABI this server still loads. 4 lacks only the carriers symbol plane.
pub const OLDEST_ABI_VERSION: u32 = 4;
pub const IMPORT_MODULE: &str = "sdrmm";

/// Guest exports every plugin has, by name.
pub mod export {
    pub const ABI: &str = "sdrmm_abi";
    pub const ALLOC: &str = "sdrmm_alloc";
    pub const MANIFEST: &str = "sdrmm_manifest";
    pub const ERROR: &str = "sdrmm_error";
    /// `(op: i32, ptr: u32, len: u32) -> packed reply: i64`, one of [`super::Op`] with its JSON
    /// in and a JSON [`super::Reply`] out. A plugin with a decoder or an encoder needs it.
    pub const CALL: &str = "sdrmm_call";
    /// `(ptr: u32, len: u32) -> packed reply: i64`, a [`super::PresentRequest`] in, a
    /// [`super::PresentReply`] out. Only a plugin that declares event kinds needs it.
    pub const PRESENT: &str = "sdrmm_present";
}

/// Guest exports of a plugin's decoder, by name. Settings travel as a JSON
/// [`crate::ChannelSettings`].
pub mod decoder {
    /// `(rate: f64, ptr: u32, len: u32) -> handle: i32`
    pub const OPEN: &str = "sdrmm_open";
    /// `(handle: i32, ptr: u32, len: u32) -> i32`
    pub const APPLY: &str = "sdrmm_apply";
    /// `(handle: i32, samples: u32) -> ptr: u32`, where the next block's I/Q goes.
    pub const INPUT: &str = "sdrmm_input";
    /// `(handle: i32, samples: u32, flags: u32) -> header_ptr: u32`, flags from
    /// [`super::process`].
    pub const PROCESS: &str = "sdrmm_process";
    /// `(handle: i32)`
    pub const CLOSE: &str = "sdrmm_close";
    /// Optional. `(handle: i32)`, after the channel moved.
    pub const RETUNED: &str = "sdrmm_retuned";
    /// Optional. `(handle: i32, ptr: u32, len: u32)`, the receiver's position as a JSON
    /// `Option<PositionFix>`.
    pub const POSITION: &str = "sdrmm_position";
    /// Optional. `(handle: i32, unix_ns: i64)`, when the next block's first sample was captured.
    /// Called only while the source's clock is known.
    pub const CLOCK: &str = "sdrmm_clock";
    /// Optional. `(handle: i32, offset_hz: f64)`, where the receiver's LO artifact sits in the
    /// channel's baseband, NaN once it sits outside.
    pub const LO_ARTIFACT: &str = "sdrmm_lo_artifact";
    /// Optional. `(handle: i32) -> i32`, zero when the channel wants nothing while squelched.
    pub const GATED: &str = "sdrmm_gated";

    pub const REQUIRED: [&str; 5] = [OPEN, APPLY, INPUT, PROCESS, CLOSE];
}

/// Bits of the decoder's `sdrmm_process` flags.
pub mod process {
    /// Someone watches the channel's symbols: fill the symbol fields of the header.
    pub const WANT_SYMBOLS: u32 = 1;
}

/// Host imports in [`IMPORT_MODULE`], by name: native `dsp` kernels working on guest memory.
pub mod import {
    pub const LOG: &str = "log";
    pub const FIR_NEW: &str = "fir_new";
    pub const FIR_PROCESS: &str = "fir_process";
    pub const FFT_NEW: &str = "fft_new";
    pub const FFT_RUN: &str = "fft_run";
    pub const RESAMPLER_NEW: &str = "resampler_new";
    pub const RESAMPLER_PROCESS: &str = "resampler_process";
    pub const KERNEL_DROP: &str = "kernel_drop";
}

/// Control operations of `sdrmm_call(op, ptr, len)`, each with the JSON it takes and returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// Decoder settings in, a [`PluginLayout`] out.
    Layout = 1,
    /// Decoder settings in, the settings with every default filled in out.
    Normalize = 2,
    /// Decoder settings in, a [`crate::FilterSpec`] out.
    Filter = 3,
    /// A [`TuneRequest`] in, a [`TuneReply`] out.
    Tune = 4,
    /// A [`FeedRequest`] in, the frame's bytes out.
    Feed = 5,
    /// Encoder settings in, the width in Hz they keep the signal inside out.
    Width = 6,
    /// A filled message form in, the payload to submit out.
    Compose = 7,
}

impl Op {
    const ALL: [Self; 7] = [
        Self::Layout,
        Self::Normalize,
        Self::Filter,
        Self::Tune,
        Self::Feed,
        Self::Width,
        Self::Compose,
    ];

    #[must_use]
    pub fn from_code(code: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|op| *op as i32 == code)
    }
}

/// What a plugin brings: any of a decoder, an encoder and an array processor, and the event
/// kinds they emit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub abi: u32,
    /// Event kinds the decoder and the processor emit, presented through [`export::PRESENT`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<EventKindEntry>,
    /// Run through the [`decoder`] exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder: Option<DecoderManifest>,
    /// Run through the [`encoder::export`] exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder: Option<EncoderDescriptor>,
    /// Run through the [`processor::export`] exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processor: Option<ProcessorType>,
}

impl PluginManifest {
    /// Whether the plugin brings anything to load.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.decoder.is_none() && self.encoder.is_none() && self.processor.is_none()
    }
}

/// Everything the host needs to list and open a plugin's decoder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecoderManifest {
    /// The completed descriptor, its settings form and defaults included.
    pub descriptor: ChannelDescriptor,
    /// Feed formats the decoder offers, each frame made by [`Op::Feed`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feeds: Vec<PluginFeed>,
}

/// A feed format a plugin decoder offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginFeed {
    #[serde(flatten)]
    pub entry: FeedEntry,
    /// Sent to idle clients so they know the feed is alive; empty sends nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heartbeat: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginLayout {
    pub input_rate_hz: f64,
    pub band: (f64, f64),
    #[serde(default = "mono")]
    pub audio_channels: u8,
}

fn mono() -> u8 {
    1
}

/// Settings to shape to a signal Signal ID found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TuneRequest {
    pub settings: Value,
    pub signal: IdentSignal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TuneReply {
    pub settings: Value,
    /// How far the channel tunes off the signal centre.
    pub offset_hz: f64,
}

/// One event to frame in one of the decoder's feed formats.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeedRequest {
    pub format: String,
    pub event: DecoderEvent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresentRequest {
    pub kind: EventKind,
    pub data: Value,
}

/// A control call's answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply<T> {
    Ok(T),
    Err(String),
}

impl<T> Reply<T> {
    pub fn into_result(self) -> Result<T, String> {
        match self {
            Self::Ok(value) => Ok(value),
            Self::Err(error) => Err(error),
        }
    }
}

impl<T> From<Result<T, String>> for Reply<T> {
    fn from(result: Result<T, String>) -> Self {
        match result {
            Ok(value) => Self::Ok(value),
            Err(error) => Self::Err(error),
        }
    }
}

pub type PresentReply = Reply<Presentation>;

/// Packs a guest pointer and length into the `i64` a guest export returns.
#[must_use]
pub fn pack(ptr: u32, len: u32) -> i64 {
    i64::from(ptr) << 32 | i64::from(len)
}

#[must_use]
pub fn unpack(packed: i64) -> (u32, u32) {
    let bits = packed as u64;
    ((bits >> 32) as u32, (bits & 0xFFFF_FFFF) as u32)
}

/// What the events and status buffers decode to, for both sides to agree on.
pub type EventBatch = Vec<DecoderEvent>;
pub type StatusUpdate = ChannelStatus;

/// The events a guest left in a buffer.
pub fn read_events(bytes: &[u8]) -> Result<EventBatch, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("events: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pointer_and_length_pack_into_one_word() {
        assert_eq!(unpack(pack(0xDEAD_BEEF, 17)), (0xDEAD_BEEF, 17));
        assert_eq!(unpack(pack(0, u32::MAX)), (0, u32::MAX));
    }

    #[test]
    fn a_manifest_brings_any_mix_of_parts() {
        let encoder_only: PluginManifest = serde_json::from_value(serde_json::json!({
            "abi": ABI_VERSION,
            "encoder": {"type_id": "beacon", "name": "Beacon", "rate_hz": 48_000.0, "bandwidth_hz": 2_400.0},
        }))
        .expect("an encoder alone parses");
        assert!(encoder_only.decoder.is_none() && encoder_only.kinds.is_empty());
        assert!(!encoder_only.is_empty());
        let empty: PluginManifest =
            serde_json::from_value(serde_json::json!({"abi": ABI_VERSION})).expect("parses");
        assert!(empty.is_empty());
    }

    #[test]
    fn a_reply_reads_as_a_result() {
        let json = serde_json::to_string(&Reply::<u8>::Err("bad".into())).expect("reply");
        let back: Reply<u8> = serde_json::from_str(&json).expect("reply parses");
        assert_eq!(back.into_result(), Err("bad".to_owned()));
        assert_eq!(Op::from_code(2), Some(Op::Normalize));
        assert_eq!(Op::from_code(7), Some(Op::Compose));
        assert_eq!(Op::from_code(8), None);
    }

    #[test]
    fn a_decoder_manifest_carries_its_feeds() {
        let manifest: DecoderManifest = serde_json::from_value(serde_json::json!({
            "descriptor": {"type_id": "probe", "name": "Probe", "bandwidth_hz": 1e3, "input_rate_hz": 48e3},
            "feeds": [{"format": "probe-text", "name": "Probe text", "default_address": "0.0.0.0:3000",
                "kinds": ["probe"], "heartbeat": [10]}],
        }))
        .expect("parses");
        let feed = &manifest.feeds[0];
        assert_eq!(feed.entry.format, "probe-text");
        assert_eq!(feed.heartbeat, b"\n");
    }
}
