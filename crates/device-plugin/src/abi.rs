use std::ffi::c_void;

use sdrmm_device::{DeviceError, GapScope, LaneEvent, LaneMark, Sample, Uncertainty};
use sdrmm_wire::Direction;

/// The version of [`PluginV1`] and of every wire type that crosses it as JSON. A host refuses
/// a plugin built for another.
pub const DEVICE_ABI_VERSION: u32 = 1;

/// The one symbol a device plugin exports, as `extern "C" fn(*const HostV1) -> *const PluginV1`.
pub const ENTRY_SYMBOL: &str = "sdrmm_device_plugin_v1";

pub type Entry = unsafe extern "C" fn(*const HostV1) -> *const PluginV1;
pub type Handle = *mut c_void;

/// Bytes the guest allocated. Only the guest's `free_bytes` may release them.
#[repr(C)]
pub struct Bytes {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl Bytes {
    pub const EMPTY: Self = Self {
        ptr: std::ptr::null_mut(),
        len: 0,
        cap: 0,
    };

    #[must_use]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        let mut bytes = std::mem::ManuallyDrop::new(bytes);
        Self {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        }
    }

    /// # Safety
    /// `self` came from [`Bytes::from_vec`] in this same binary and is released once.
    #[must_use]
    pub unsafe fn into_vec(self) -> Vec<u8> {
        if self.ptr.is_null() {
            return Vec::new();
        }
        unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) }
    }

    /// # Safety
    /// `ptr` and `len` describe live bytes.
    #[must_use]
    pub unsafe fn as_slice(&self) -> &[u8] {
        if self.ptr.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// Bytes borrowed for the length of one call.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}

impl Str {
    #[must_use]
    pub const fn of(text: &[u8]) -> Self {
        Self {
            ptr: text.as_ptr(),
            len: text.len(),
        }
    }

    /// # Safety
    /// `ptr` and `len` describe bytes alive for `'a`.
    #[must_use]
    pub unsafe fn bytes<'a>(self) -> &'a [u8] {
        if self.ptr.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// # Safety
    /// As [`Str::bytes`].
    #[must_use]
    pub unsafe fn text<'a>(self) -> std::borrow::Cow<'a, str> {
        String::from_utf8_lossy(unsafe { self.bytes() })
    }
}

/// A [`DeviceError`] as a code and its text. Code 0 is success.
#[repr(C)]
pub struct Status {
    pub code: u32,
    pub message: Bytes,
}

pub const OK: u32 = 0;
const NOT_FOUND: u32 = 1;
const UNSUPPORTED: u32 = 2;
const IO: u32 = 3;
const DISCONNECTED: u32 = 4;
const IN_USE: u32 = 5;
const PERMISSION_DENIED: u32 = 6;
const ALREADY_STREAMING: u32 = 7;
const DUPLEX_CONFLICT: u32 = 8;

impl Status {
    pub const OK: Self = Self {
        code: OK,
        message: Bytes::EMPTY,
    };

    #[must_use]
    pub fn from_error(error: &DeviceError) -> Self {
        let (code, text) = encode_error(error);
        Self {
            code,
            message: Bytes::from_vec(text.into_bytes()),
        }
    }
}

#[must_use]
pub fn encode_error(error: &DeviceError) -> (u32, String) {
    match error {
        DeviceError::NotFound(text) => (NOT_FOUND, text.clone()),
        DeviceError::Unsupported(text) => (UNSUPPORTED, text.clone()),
        DeviceError::Io(text) => (IO, text.clone()),
        DeviceError::Disconnected(text) => (DISCONNECTED, text.clone()),
        DeviceError::InUse(text) => (IN_USE, text.clone()),
        DeviceError::PermissionDenied(text) => (PERMISSION_DENIED, text.clone()),
        DeviceError::AlreadyStreaming => (ALREADY_STREAMING, String::new()),
        DeviceError::DuplexConflict { active, requested } => (
            DUPLEX_CONFLICT,
            serde_json::to_string(&[active, requested]).unwrap_or_default(),
        ),
    }
}

#[must_use]
pub fn decode_error(code: u32, text: String) -> DeviceError {
    match code {
        NOT_FOUND => DeviceError::NotFound(text),
        UNSUPPORTED => DeviceError::Unsupported(text),
        DISCONNECTED => DeviceError::Disconnected(text),
        IN_USE => DeviceError::InUse(text),
        PERMISSION_DENIED => DeviceError::PermissionDenied(text),
        ALREADY_STREAMING => DeviceError::AlreadyStreaming,
        DUPLEX_CONFLICT => match serde_json::from_str::<[Direction; 2]>(&text) {
            Ok([active, requested]) => DeviceError::DuplexConflict { active, requested },
            Err(_) => DeviceError::Io(format!("plugin sent a malformed duplex conflict: {text}")),
        },
        _ => DeviceError::Io(text),
    }
}

/// A [`LaneEvent`] without its position, which both sides already agree on.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventV1 {
    pub kind: u32,
    pub detail: u32,
    pub flag: u32,
    pub count: u64,
    pub ns: i64,
}

const MARK: u32 = 0;
const UNCERTAIN: u32 = 1;
const HARDWARE_TIME: u32 = 2;
const DEAF: u32 = 3;

impl EventV1 {
    #[must_use]
    pub fn encode(event: LaneEvent) -> Self {
        match event {
            LaneEvent::Mark { mark, .. } => encode_mark(mark),
            LaneEvent::Uncertain {
                error,
                scope,
                cause,
                ..
            } => Self {
                kind: UNCERTAIN,
                detail: cause_code(cause),
                flag: u32::from(scope == GapScope::Device),
                count: error,
                ns: 0,
            },
            LaneEvent::HardwareTime { ns, .. } => Self {
                kind: HARDWARE_TIME,
                ns,
                ..Self::default()
            },
            LaneEvent::Deaf { samples, .. } => Self {
                kind: DEAF,
                count: samples,
                ..Self::default()
            },
        }
    }

    #[must_use]
    pub fn decode(self, at: u64) -> Option<LaneEvent> {
        Some(match self.kind {
            MARK => LaneEvent::Mark {
                at,
                mark: decode_mark(self)?,
            },
            UNCERTAIN => LaneEvent::Uncertain {
                at,
                error: self.count,
                scope: if self.flag == 0 {
                    GapScope::Lane
                } else {
                    GapScope::Device
                },
                cause: cause_of(self.detail)?,
            },
            HARDWARE_TIME => LaneEvent::HardwareTime { at, ns: self.ns },
            DEAF => LaneEvent::Deaf {
                at,
                samples: self.count,
            },
            _ => return None,
        })
    }
}

fn encode_mark(mark: LaneMark) -> EventV1 {
    let (detail, flag, count) = match mark {
        LaneMark::NoiseSource { on, in_flight } => (0, u32::from(on), in_flight),
        LaneMark::Retuned { in_flight } => (1, 0, in_flight),
        LaneMark::GainChanged { in_flight } => (2, 0, in_flight),
        LaneMark::Ended => (3, 0, 0),
    };
    EventV1 {
        kind: MARK,
        detail,
        flag,
        count,
        ns: 0,
    }
}

fn decode_mark(event: EventV1) -> Option<LaneMark> {
    let in_flight = event.count;
    Some(match event.detail {
        0 => LaneMark::NoiseSource {
            on: event.flag != 0,
            in_flight,
        },
        1 => LaneMark::Retuned { in_flight },
        2 => LaneMark::GainChanged { in_flight },
        3 => LaneMark::Ended,
        _ => return None,
    })
}

const CAUSES: [Uncertainty; 6] = [
    Uncertainty::EstimatedGap,
    Uncertainty::Rearmed,
    Uncertainty::RateWrite,
    Uncertainty::Overflow,
    Uncertainty::Reset,
    Uncertainty::Unaligned,
];

fn cause_code(cause: Uncertainty) -> u32 {
    CAUSES
        .iter()
        .position(|known| *known == cause)
        .map_or(u32::MAX, |at| at as u32)
}

fn cause_of(code: u32) -> Option<Uncertainty> {
    CAUSES.get(code as usize).copied()
}

/// The most transmit channels one write carries, so neither side allocates for it.
pub const MAX_TX_CHANNELS: usize = 16;

/// One channel of samples, borrowed for one call.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SamplesV1 {
    pub ptr: *const Sample,
    pub len: usize,
}

/// A firmware sweep plan: `bands` holds start and stop pairs in Hz.
#[repr(C)]
pub struct SweepPlanV1 {
    pub bands: *const [f64; 2],
    pub band_count: usize,
    pub sample_rate_hz: f64,
}

/// What the host lends every plugin: where logs and samples go. The first four fields keep their
/// place in every version, so a plugin on another ABI can still say why it refuses. Every callback is safe to call
/// from any thread; one sink is fed from one thread at a time.
#[repr(C)]
pub struct HostV1 {
    pub abi: u32,
    pub size: u32,
    /// The most verbose log level the host keeps: 0 off, 1 error up to 5 trace.
    pub max_level: u32,
    pub log: extern "C" fn(level: u32, target: Str, message: Str),
    pub sink_samples: extern "C" fn(ctx: Handle, samples: *const Sample, len: usize),
    pub sink_event: extern "C" fn(ctx: Handle, event: EventV1),
    pub sink_dropped: extern "C" fn(ctx: Handle, samples: u64),
    pub sink_fail: extern "C" fn(ctx: Handle, code: u32, message: Str),
    pub sink_release: extern "C" fn(ctx: Handle),
    pub sweep_push: extern "C" fn(ctx: Handle, center_hz: f64, samples: *const Sample, len: usize),
    pub sweep_fail: extern "C" fn(ctx: Handle, code: u32, message: Str),
    pub sweep_release: extern "C" fn(ctx: Handle),
}

/// The driver behind a table is a `DeviceDriver`, which is `Send` and `Sync`, and the rest of
/// the table is static, so the host may call it from any thread.
unsafe impl Send for PluginV1 {}
/// As for `Send`.
unsafe impl Sync for PluginV1 {}

/// What a plugin hands the host: its driver and every call the host may make on it. JSON
/// arguments and results are the wire types the matching trait method takes or returns.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PluginV1 {
    pub abi: u32,
    pub size: u32,
    /// The plugin's crate name and version.
    pub plugin: Str,
    /// The sdr-- revision the plugin was built against, or empty.
    pub sdk_build: Str,
    /// Whether the plugin unwinds panics. One built with `panic = "abort"` would take the host
    /// down with it, so the host refuses it.
    pub panic_unwind: bool,
    pub driver: Handle,
    pub driver_drop: extern "C" fn(driver: Handle),
    pub free_bytes: extern "C" fn(Bytes),
    pub spec: extern "C" fn(driver: Handle, out: *mut Bytes) -> Status,
    pub probe: extern "C" fn(driver: Handle, deep: bool, out: *mut Bytes) -> Status,
    pub open: extern "C" fn(driver: Handle, info: Str, device: *mut Handle) -> Status,
    pub resolve: extern "C" fn(driver: Handle, key: Str, out: *mut Bytes) -> Status,
    pub health: extern "C" fn(driver: Handle, out: *mut Bytes) -> Status,
    pub write_serial:
        extern "C" fn(driver: Handle, key: Str, serial: Str, out: *mut Bytes) -> Status,
    pub capabilities: extern "C" fn(device: Handle, out: *mut Bytes) -> Status,
    pub settings: extern "C" fn(device: Handle, out: *mut Bytes) -> Status,
    pub apply: extern "C" fn(device: Handle, settings: Str) -> Status,
    pub rx_start: extern "C" fn(device: Handle, sinks: *const Handle, count: usize) -> Status,
    pub rx_stop: extern "C" fn(device: Handle) -> Status,
    pub duplex: extern "C" fn(device: Handle, out: *mut Bytes) -> Status,
    pub in_flight_samples: extern "C" fn(device: Handle, out: *mut u64) -> Status,
    pub tx_start: extern "C" fn(
        device: Handle,
        channels: *const u32,
        count: usize,
        tx: *mut Handle,
    ) -> Status,
    pub agc_gains: extern "C" fn(device: Handle, out: *mut Bytes) -> Status,
    pub readouts: extern "C" fn(device: Handle, out: *mut Bytes) -> Status,
    pub set_noise_source: extern "C" fn(device: Handle, on: bool) -> Status,
    pub sweep_start:
        extern "C" fn(device: Handle, plan: *const SweepPlanV1, sink: Handle) -> Status,
    pub sweep_stop: extern "C" fn(device: Handle) -> Status,
    pub device_drop: extern "C" fn(device: Handle),
    pub tx_write: extern "C" fn(
        tx: Handle,
        channels: *const SamplesV1,
        count: usize,
        timeout_ns: u64,
        end_burst: bool,
        written: *mut usize,
    ) -> Status,
    pub tx_stop: extern "C" fn(tx: Handle) -> Status,
    pub tx_drop: extern "C" fn(tx: Handle),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_survives_the_trip() {
        let events = [
            LaneEvent::Mark {
                at: 9,
                mark: LaneMark::NoiseSource {
                    on: true,
                    in_flight: 7,
                },
            },
            LaneEvent::Mark {
                at: 9,
                mark: LaneMark::Retuned { in_flight: 3 },
            },
            LaneEvent::Mark {
                at: 9,
                mark: LaneMark::GainChanged { in_flight: 4 },
            },
            LaneEvent::Mark {
                at: 9,
                mark: LaneMark::Ended,
            },
            LaneEvent::Uncertain {
                at: 9,
                error: 12,
                scope: GapScope::Device,
                cause: Uncertainty::Unaligned,
            },
            LaneEvent::HardwareTime { at: 9, ns: -5 },
            LaneEvent::Deaf {
                at: 9,
                samples: 100,
            },
        ];
        for event in events {
            assert_eq!(EventV1::encode(event).decode(9), Some(event));
        }
    }

    #[test]
    fn an_event_from_a_newer_plugin_is_not_guessed_at() {
        let unknown = EventV1 {
            kind: 99,
            ..EventV1::default()
        };
        assert_eq!(unknown.decode(0), None);
    }

    #[test]
    fn every_error_survives_the_trip() {
        let errors = [
            DeviceError::NotFound("a".into()),
            DeviceError::Unsupported("b".into()),
            DeviceError::Io("c".into()),
            DeviceError::Disconnected("d".into()),
            DeviceError::InUse("e".into()),
            DeviceError::PermissionDenied("f".into()),
            DeviceError::AlreadyStreaming,
            DeviceError::DuplexConflict {
                active: Direction::Rx,
                requested: Direction::Tx,
            },
        ];
        for error in errors {
            let (code, text) = encode_error(&error);
            assert_eq!(decode_error(code, text).to_string(), error.to_string());
        }
    }
}
