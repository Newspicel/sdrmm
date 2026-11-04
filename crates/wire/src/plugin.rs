//! The contract between the server and a WASM decoder plugin.
//!
//! A plugin is a core WebAssembly module. Control calls exchange JSON in the guest's memory;
//! the per-block call exchanges raw samples through buffers the guest owns, so the hot path
//! copies samples and allocates nothing.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    channel::{ChannelDescriptor, ChannelStatus, SettingField},
    decode::{DecoderEvent, EventKind, EventKindEntry},
    present::Presentation,
};

pub const ABI_VERSION: u32 = 1;
pub const IMPORT_MODULE: &str = "sdrmm";

/// Guest exports, by name.
pub mod export {
    pub const ABI: &str = "sdrmm_abi";
    pub const ALLOC: &str = "sdrmm_alloc";
    pub const MANIFEST: &str = "sdrmm_manifest";
    pub const CALL: &str = "sdrmm_call";
    pub const OPEN: &str = "sdrmm_open";
    pub const APPLY: &str = "sdrmm_apply";
    pub const INPUT: &str = "sdrmm_input";
    pub const PROCESS: &str = "sdrmm_process";
    pub const RETUNED: &str = "sdrmm_retuned";
    pub const CLOSE: &str = "sdrmm_close";
    pub const ERROR: &str = "sdrmm_error";
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

/// Control operations of `sdrmm_call(op, ptr, len)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    Layout = 1,
    Normalize = 2,
    Present = 3,
}

impl Op {
    #[must_use]
    pub fn from_code(code: i32) -> Option<Self> {
        [Self::Layout, Self::Normalize, Self::Present]
            .into_iter()
            .find(|op| *op as i32 == code)
    }
}

/// Everything the host needs to list and open a plugin's decoder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub abi: u32,
    pub descriptor: ChannelDescriptor,
    #[serde(default)]
    pub form: Vec<SettingField>,
    #[serde(default)]
    pub kinds: Vec<EventKindEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_hz: Option<f64>,
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

/// What one `sdrmm_process` call left in guest memory: little-endian `u32` words at the
/// pointer it returns, in this order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputHeader {
    pub audio_ptr: u32,
    /// Interleaved `f32` samples.
    pub audio_len: u32,
    pub audio_rate: u32,
    /// A JSON array of [`DecoderEvent`]s, empty when none.
    pub events_ptr: u32,
    pub events_len: u32,
    /// A JSON [`ChannelStatus`], empty when unchanged.
    pub status_ptr: u32,
    pub status_len: u32,
    pub rejected: u32,
    pub skipped: u32,
}

impl OutputHeader {
    pub const WORDS: usize = 9;
    pub const BYTES: usize = Self::WORDS * 4;

    #[must_use]
    pub fn to_words(self) -> [u32; Self::WORDS] {
        [
            self.audio_ptr,
            self.audio_len,
            self.audio_rate,
            self.events_ptr,
            self.events_len,
            self.status_ptr,
            self.status_len,
            self.rejected,
            self.skipped,
        ]
    }

    #[must_use]
    pub fn from_words(words: [u32; Self::WORDS]) -> Self {
        let [
            audio_ptr,
            audio_len,
            audio_rate,
            events_ptr,
            events_len,
            status_ptr,
            status_len,
            rejected,
            skipped,
        ] = words;
        Self {
            audio_ptr,
            audio_len,
            audio_rate,
            events_ptr,
            events_len,
            status_ptr,
            status_len,
            rejected,
            skipped,
        }
    }

    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::BYTES] {
        let mut bytes = [0u8; Self::BYTES];
        for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(self.to_words()) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    #[must_use]
    pub fn from_bytes(bytes: [u8; Self::BYTES]) -> Self {
        let mut words = [0u32; Self::WORDS];
        for (word, chunk) in words.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*chunk);
        }
        Self::from_words(words)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_survives_its_byte_form() {
        let header = OutputHeader {
            audio_ptr: 1,
            audio_len: 2,
            audio_rate: 48_000,
            events_ptr: 4,
            events_len: 5,
            status_ptr: 6,
            status_len: 7,
            rejected: 8,
            skipped: u32::MAX,
        };
        assert_eq!(OutputHeader::from_bytes(header.to_bytes()), header);
    }

    #[test]
    fn a_pointer_and_length_pack_into_one_word() {
        assert_eq!(unpack(pack(0xDEAD_BEEF, 17)), (0xDEAD_BEEF, 17));
        assert_eq!(unpack(pack(0, u32::MAX)), (0, u32::MAX));
    }

    #[test]
    fn a_reply_reads_as_a_result() {
        let json = serde_json::to_string(&Reply::<u8>::Err("bad".into())).expect("reply");
        let back: Reply<u8> = serde_json::from_str(&json).expect("reply parses");
        assert_eq!(back.into_result(), Err("bad".to_owned()));
        assert_eq!(Op::from_code(3), Some(Op::Present));
        assert_eq!(Op::from_code(9), None);
    }
}
