//! The array processor half of the plugin ABI. A plugin whose manifest declares a `processor`
//! exports these and runs on every lane of an array at once, like a built-in processor.
//!
//! Each block, `sdrmm_array_input` hands out a guest buffer for `lanes × samples` interleaved
//! little-endian `f32` I/Q, one lane after the other, and `sdrmm_array_process` returns where its
//! [`ArrayHeader`] sits. The header points at one little-endian `f64` per declared readout (NaN
//! for none) and at a JSON array of events. Buffers stay the guest's until its next call.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Guest exports, by name.
pub mod export {
    /// `(ptr: u32, len: u32) -> handle: i32`, the JSON of an [`super::ArrayOpen`].
    pub const OPEN: &str = "sdrmm_array_open";
    /// `(handle: i32, settings_ptr: u32, settings_len: u32) -> i32`
    pub const APPLY: &str = "sdrmm_array_apply";
    /// `(handle: i32, facts_ptr: u32, facts_len: u32) -> i32`, the array's new
    /// [`super::ArrayFacts`].
    pub const RETUNE: &str = "sdrmm_array_retune";
    /// `(handle: i32, cause: u32)`, one of [`super::Reset`].
    pub const RESET: &str = "sdrmm_array_reset";
    /// `(handle: i32, lanes: u32, samples: u32) -> ptr: u32`
    pub const INPUT: &str = "sdrmm_array_input";
    /// `(handle: i32, lanes: u32, samples: u32) -> header_ptr: u32`
    pub const PROCESS: &str = "sdrmm_array_process";
    /// `(handle: i32)`
    pub const CLOSE: &str = "sdrmm_array_close";

    pub const ALL: [&str; 7] = [OPEN, APPLY, RETUNE, RESET, INPUT, PROCESS, CLOSE];
}

/// What a processor knows of the array it runs on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ArrayFacts {
    pub lanes: u32,
    pub sample_rate: f64,
    pub center_hz: f64,
    #[serde(default)]
    pub lane_centers_hz: Vec<f64>,
    /// Element positions in metres, in lane order.
    #[serde(default)]
    pub positions_m: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArrayOpen {
    pub facts: ArrayFacts,
    #[serde(default)]
    pub settings: Value,
}

/// Why the host asks a processor to forget what it has accumulated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Reset {
    Realigned = 0,
    Retuned = 1,
    Calibrated = 2,
    Gap = 3,
    Resumed = 4,
}

impl Reset {
    pub const ALL: [Self; 5] = [
        Self::Realigned,
        Self::Retuned,
        Self::Calibrated,
        Self::Gap,
        Self::Resumed,
    ];

    #[must_use]
    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|cause| *cause as u32 == code)
    }
}

/// `lock` in an [`ArrayHeader`].
pub mod lock {
    pub const UNKNOWN: u32 = 0;
    pub const LOST: u32 = 1;
    pub const HELD: u32 = 2;
}

/// What one `sdrmm_array_process` call left: little-endian `u32` words in this order. Health
/// numbers are `f32` bits, NaN while unknown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ArrayHeader {
    /// Non-zero when this block carries a new report.
    pub report: u32,
    pub values_ptr: u32,
    pub values_len: u32,
    pub lock: u32,
    pub snr_db: f32,
    pub freq_error_hz: f32,
    pub errors_per_s: f32,
    /// A JSON array of `DecoderEvent`s, empty when none.
    pub events_ptr: u32,
    pub events_len: u32,
    /// Results the plugin cut, counted up over its life.
    pub truncated: u32,
}

impl ArrayHeader {
    pub const WORDS: usize = 10;
    pub const BYTES: usize = Self::WORDS * 4;

    #[must_use]
    pub fn to_words(self) -> [u32; Self::WORDS] {
        [
            self.report,
            self.values_ptr,
            self.values_len,
            self.lock,
            self.snr_db.to_bits(),
            self.freq_error_hz.to_bits(),
            self.errors_per_s.to_bits(),
            self.events_ptr,
            self.events_len,
            self.truncated,
        ]
    }

    #[must_use]
    pub fn from_words(words: [u32; Self::WORDS]) -> Self {
        let [
            report,
            values_ptr,
            values_len,
            lock,
            snr_db,
            freq_error_hz,
            errors_per_s,
            events_ptr,
            events_len,
            truncated,
        ] = words;
        Self {
            report,
            values_ptr,
            values_len,
            lock,
            snr_db: f32::from_bits(snr_db),
            freq_error_hz: f32::from_bits(freq_error_hz),
            errors_per_s: f32::from_bits(errors_per_s),
            events_ptr,
            events_len,
            truncated,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_survives_its_byte_form() {
        let header = ArrayHeader {
            report: 1,
            values_ptr: 64,
            values_len: 3,
            lock: lock::HELD,
            snr_db: 12.5,
            freq_error_hz: f32::NAN,
            errors_per_s: 0.0,
            events_ptr: 128,
            events_len: 40,
            truncated: 2,
        };
        let back = ArrayHeader::from_bytes(header.to_bytes());
        assert!(back.freq_error_hz.is_nan());
        assert_eq!(
            ArrayHeader {
                freq_error_hz: 0.0,
                ..back
            },
            ArrayHeader {
                freq_error_hz: 0.0,
                ..header
            }
        );
    }

    #[test]
    fn a_reset_reads_back_from_its_code() {
        for cause in Reset::ALL {
            assert_eq!(Reset::from_code(cause as u32), Some(cause));
        }
        assert_eq!(Reset::from_code(9), None);
    }
}
