//! The array processor half of the plugin ABI. A plugin whose manifest declares a `processor`
//! exports these and runs on every lane of an array at once, like a built-in processor.
//!
//! Each block, `sdrmm_array_input` hands out a guest buffer for `lanes × samples` interleaved
//! little-endian `f32` I/Q, one lane after the other, and `sdrmm_array_context` one of
//! [`BlockContext::BYTES`] for what the block carries beside its samples. `sdrmm_array_process`
//! returns where its [`ArrayHeader`] sits. The header points at one little-endian `f64` per
//! declared readout (NaN for none), a JSON array of events, an encoded surface frame and the
//! lanes it wrote. Buffers stay the guest's until its next call.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::array::{ArrayGeometry, ArrayTuningMode, MAX_ARRAY_LANES};
use crate::device::Coherence;

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
    /// `(handle: i32) -> ptr: u32`, room for one [`super::BlockContext`].
    pub const CONTEXT: &str = "sdrmm_array_context";
    /// `(handle: i32, lanes: u32, samples: u32) -> header_ptr: u32`
    pub const PROCESS: &str = "sdrmm_array_process";
    /// `(handle: i32)`
    pub const CLOSE: &str = "sdrmm_array_close";
    /// `(handle: i32, action: u32) -> i32`, one of [`super::action`]; a type that declares
    /// actions exports it.
    pub const ACTION: &str = "sdrmm_array_action";

    pub const ALL: [&str; 8] = [OPEN, APPLY, RETUNE, RESET, INPUT, CONTEXT, PROCESS, CLOSE];
}

/// Codes of the actions an operator sends a processor.
pub mod action {
    pub const CLEAR_TRACKS: u32 = 0;
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
    #[serde(default)]
    pub geometry: ArrayGeometry,
    #[serde(default)]
    pub tier: Coherence,
    #[serde(default)]
    pub tuning: ArrayTuningMode,
    /// The most samples per lane one block carries.
    #[serde(default)]
    pub max_block: u32,
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

/// The most lanes a block context describes.
pub const CONTEXT_LANES: usize = MAX_ARRAY_LANES as usize;
/// The most other peaks a steer carries.
pub const STEER_OTHERS: usize = 3;
/// The most lanes a processor writes.
pub const LANE_OUTPUTS: usize = 2;

/// The calibration a block was taken under.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockCal {
    /// The calibration phase, its index in `CalPhase` order.
    pub phase: u32,
    /// The cal source, its index in `CalSourceKind` order; `u32::MAX` for none.
    pub source: u32,
    pub phase_ready: bool,
    pub gain_ready: bool,
    pub generation: u32,
    pub phase_sigma_deg: f32,
    pub gain_sigma_db: f32,
    /// The band the calibration holds across, NaN when it holds everywhere.
    pub valid_low_hz: f64,
    pub valid_high_hz: f64,
}

/// Where the array stood and pointed at a block; NaN where it is not known.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockPose {
    pub heading_deg: f64,
    pub heading_sigma_deg: f32,
    pub yaw_rate_dps: f32,
    pub lat: f64,
    pub lon: f64,
    pub altitude_m: f64,
    pub accuracy_m: f32,
    pub speed_mps: f32,
    pub moving: bool,
    pub follows: bool,
}

/// A bearing to steer at, handed in or out; NaN where a value is not known.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockSteer {
    pub present: bool,
    pub same_array: bool,
    pub relative_deg: f64,
    pub true_deg: f64,
    pub elevation_deg: f64,
    pub sigma_deg: f32,
    pub others: u32,
    pub others_relative_deg: [f64; STEER_OTHERS],
    pub others_true_deg: [f64; STEER_OTHERS],
    pub wall_ms: u64,
}

/// One lane of a block: where it is tuned and the correction it needs at the processor's band.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockLane {
    pub center_hz: f64,
    pub correction: [f32; 2],
}

/// What a block carries beside its samples, written to the buffer `sdrmm_array_context` hands
/// out, little-endian in field order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockContext {
    pub first_index: u64,
    pub unix_ns: u64,
    pub generation: u32,
    pub gap_before: bool,
    /// The host has applied the array's correction to the samples.
    pub corrected: bool,
    pub correction_generation: u32,
    pub cal: BlockCal,
    pub pose: BlockPose,
    /// The newest bearing wired to the processor's steer input.
    pub steer: BlockSteer,
    pub lanes: u32,
    pub lane: [BlockLane; CONTEXT_LANES],
}

/// Writes little-endian scalars in order.
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    fn f64(&mut self, value: f64) {
        self.u64(value.to_bits());
    }

    fn flag(&mut self, value: bool) {
        self.u32(u32::from(value));
    }
}

/// Reads little-endian scalars in order; past the end it reads zeros.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let mut word = [0u8; N];
        if let Some(slice) = self.bytes.get(self.at..self.at + N) {
            word.copy_from_slice(slice);
        }
        self.at += N;
        word
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }

    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take())
    }

    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }

    fn f64(&mut self) -> f64 {
        f64::from_bits(self.u64())
    }

    fn flag(&mut self) -> bool {
        self.u32() != 0
    }
}

impl BlockContext {
    pub const BYTES: usize = 8
        + 8
        + 4 * 4
        + (4 * 7 + 8 * 2)
        + (8 + 4 + 4 + 8 * 3 + 4 + 4 + 4 * 2)
        + (4 * 2 + 8 * 3 + 4 + 4 + 8 * 2 * STEER_OTHERS + 8)
        + 4
        + CONTEXT_LANES * (8 + 4 * 2);

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Writer {
            bytes: Vec::with_capacity(Self::BYTES),
        };
        out.u64(self.first_index);
        out.u64(self.unix_ns);
        out.u32(self.generation);
        out.flag(self.gap_before);
        out.flag(self.corrected);
        out.u32(self.correction_generation);
        self.write_cal(&mut out);
        self.write_pose(&mut out);
        self.write_steer(&mut out);
        out.u32(self.lanes);
        for lane in &self.lane {
            out.f64(lane.center_hz);
            out.f32(lane.correction[0]);
            out.f32(lane.correction[1]);
        }
        out.bytes
    }

    fn write_cal(&self, out: &mut Writer) {
        let cal = &self.cal;
        out.u32(cal.phase);
        out.u32(cal.source);
        out.flag(cal.phase_ready);
        out.flag(cal.gain_ready);
        out.u32(cal.generation);
        out.f32(cal.phase_sigma_deg);
        out.f32(cal.gain_sigma_db);
        out.f64(cal.valid_low_hz);
        out.f64(cal.valid_high_hz);
    }

    fn write_pose(&self, out: &mut Writer) {
        let pose = &self.pose;
        out.f64(pose.heading_deg);
        out.f32(pose.heading_sigma_deg);
        out.f32(pose.yaw_rate_dps);
        out.f64(pose.lat);
        out.f64(pose.lon);
        out.f64(pose.altitude_m);
        out.f32(pose.accuracy_m);
        out.f32(pose.speed_mps);
        out.flag(pose.moving);
        out.flag(pose.follows);
    }

    fn write_steer(&self, out: &mut Writer) {
        let steer = &self.steer;
        out.flag(steer.present);
        out.flag(steer.same_array);
        out.f64(steer.relative_deg);
        out.f64(steer.true_deg);
        out.f64(steer.elevation_deg);
        out.f32(steer.sigma_deg);
        out.u32(steer.others);
        for deg in steer.others_relative_deg {
            out.f64(deg);
        }
        for deg in steer.others_true_deg {
            out.f64(deg);
        }
        out.u64(steer.wall_ms);
    }

    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut read = Reader { bytes, at: 0 };
        let mut context = Self {
            first_index: read.u64(),
            unix_ns: read.u64(),
            generation: read.u32(),
            gap_before: read.flag(),
            corrected: read.flag(),
            correction_generation: read.u32(),
            ..Self::default()
        };
        context.cal = BlockCal {
            phase: read.u32(),
            source: read.u32(),
            phase_ready: read.flag(),
            gain_ready: read.flag(),
            generation: read.u32(),
            phase_sigma_deg: read.f32(),
            gain_sigma_db: read.f32(),
            valid_low_hz: read.f64(),
            valid_high_hz: read.f64(),
        };
        context.pose = BlockPose {
            heading_deg: read.f64(),
            heading_sigma_deg: read.f32(),
            yaw_rate_dps: read.f32(),
            lat: read.f64(),
            lon: read.f64(),
            altitude_m: read.f64(),
            accuracy_m: read.f32(),
            speed_mps: read.f32(),
            moving: read.flag(),
            follows: read.flag(),
        };
        context.steer = read_steer(&mut read);
        context.lanes = read.u32();
        for lane in &mut context.lane {
            lane.center_hz = read.f64();
            lane.correction = [read.f32(), read.f32()];
        }
        context
    }
}

fn read_steer(read: &mut Reader<'_>) -> BlockSteer {
    let mut steer = BlockSteer {
        present: read.flag(),
        same_array: read.flag(),
        relative_deg: read.f64(),
        true_deg: read.f64(),
        elevation_deg: read.f64(),
        sigma_deg: read.f32(),
        others: read.u32(),
        ..BlockSteer::default()
    };
    for deg in &mut steer.others_relative_deg {
        *deg = read.f64();
    }
    for deg in &mut steer.others_true_deg {
        *deg = read.f64();
    }
    steer.wall_ms = read.u64();
    steer
}

/// What one `sdrmm_array_process` call left: little-endian `u32` words in this order. Health
/// and steer numbers are `f32` bits, NaN while unknown.
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
    /// Non-zero when this block steers what is wired to the bearing output.
    pub steer: u32,
    pub steer_same_array: u32,
    pub steer_relative_deg: f32,
    pub steer_true_deg: f32,
    pub steer_elevation_deg: f32,
    pub steer_sigma_deg: f32,
    pub steer_others: u32,
    pub steer_others_relative_deg: [f32; STEER_OTHERS],
    pub steer_others_true_deg: [f32; STEER_OTHERS],
    /// One encoded surface frame, as a stream sends it; empty when none.
    pub surface_ptr: u32,
    pub surface_len: u32,
    /// Interleaved `f32` I/Q each lane output wrote this block, in port order.
    pub lane_ptr: [u32; LANE_OUTPUTS],
    pub lane_samples: [u32; LANE_OUTPUTS],
}

impl ArrayHeader {
    pub const WORDS: usize = 10 + 7 + 2 * STEER_OTHERS + 2 + 2 * LANE_OUTPUTS;
    pub const BYTES: usize = Self::WORDS * 4;

    #[must_use]
    pub fn to_words(self) -> [u32; Self::WORDS] {
        let mut words = [0u32; Self::WORDS];
        let mut at = 0;
        let mut put = |word: u32| {
            words[at] = word;
            at += 1;
        };
        for word in [
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
            self.steer,
            self.steer_same_array,
            self.steer_relative_deg.to_bits(),
            self.steer_true_deg.to_bits(),
            self.steer_elevation_deg.to_bits(),
            self.steer_sigma_deg.to_bits(),
            self.steer_others,
        ] {
            put(word);
        }
        for deg in self.steer_others_relative_deg {
            put(deg.to_bits());
        }
        for deg in self.steer_others_true_deg {
            put(deg.to_bits());
        }
        put(self.surface_ptr);
        put(self.surface_len);
        for word in self.lane_ptr.into_iter().chain(self.lane_samples) {
            put(word);
        }
        words
    }

    #[must_use]
    pub fn from_words(words: [u32; Self::WORDS]) -> Self {
        let mut next = words.into_iter();
        let mut word = || next.next().unwrap_or_default();
        let mut header = Self {
            report: word(),
            values_ptr: word(),
            values_len: word(),
            lock: word(),
            snr_db: f32::from_bits(word()),
            freq_error_hz: f32::from_bits(word()),
            errors_per_s: f32::from_bits(word()),
            events_ptr: word(),
            events_len: word(),
            truncated: word(),
            steer: word(),
            steer_same_array: word(),
            steer_relative_deg: f32::from_bits(word()),
            steer_true_deg: f32::from_bits(word()),
            steer_elevation_deg: f32::from_bits(word()),
            steer_sigma_deg: f32::from_bits(word()),
            steer_others: word(),
            ..Self::default()
        };
        for deg in &mut header.steer_others_relative_deg {
            *deg = f32::from_bits(word());
        }
        for deg in &mut header.steer_others_true_deg {
            *deg = f32::from_bits(word());
        }
        header.surface_ptr = word();
        header.surface_len = word();
        for ptr in &mut header.lane_ptr {
            *ptr = word();
        }
        for samples in &mut header.lane_samples {
            *samples = word();
        }
        header
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

    fn same(a: f32, b: f32) -> bool {
        a == b || (a.is_nan() && b.is_nan())
    }

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
            steer: 1,
            steer_relative_deg: 42.0,
            steer_true_deg: f32::NAN,
            steer_others: 1,
            steer_others_relative_deg: [10.0, 0.0, 0.0],
            surface_ptr: 256,
            surface_len: 99,
            lane_ptr: [512, 0],
            lane_samples: [16, 0],
            ..ArrayHeader::default()
        };
        let back = ArrayHeader::from_bytes(header.to_bytes());
        assert!(same(back.freq_error_hz, header.freq_error_hz));
        assert!(same(back.steer_true_deg, header.steer_true_deg));
        let clean = |header: ArrayHeader| ArrayHeader {
            freq_error_hz: 0.0,
            steer_true_deg: 0.0,
            ..header
        };
        assert_eq!(clean(back), clean(header));
    }

    #[test]
    fn a_context_survives_its_byte_form_at_its_size() {
        let mut context = BlockContext {
            first_index: 7,
            unix_ns: 1_790_000_000_000_000_000,
            generation: 3,
            gap_before: true,
            corrected: true,
            correction_generation: 2,
            cal: BlockCal {
                phase: 3,
                source: 1,
                phase_ready: true,
                phase_sigma_deg: 1.5,
                valid_low_hz: 1e6,
                valid_high_hz: 2e6,
                ..BlockCal::default()
            },
            pose: BlockPose {
                heading_deg: 90.0,
                lat: 52.0,
                lon: 13.0,
                moving: true,
                ..BlockPose::default()
            },
            steer: BlockSteer {
                present: true,
                relative_deg: 30.0,
                others: 2,
                others_relative_deg: [1.0, 2.0, 0.0],
                wall_ms: 99,
                ..BlockSteer::default()
            },
            lanes: 2,
            ..BlockContext::default()
        };
        context.lane[1] = BlockLane {
            center_hz: 100e6,
            correction: [0.5, -0.5],
        };
        let bytes = context.to_bytes();
        assert_eq!(bytes.len(), BlockContext::BYTES);
        assert_eq!(BlockContext::from_bytes(&bytes), context);
    }

    #[test]
    fn a_reset_reads_back_from_its_code() {
        for cause in Reset::ALL {
            assert_eq!(Reset::from_code(cause as u32), Some(cause));
        }
        assert_eq!(Reset::from_code(9), None);
    }
}
