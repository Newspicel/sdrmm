//! The array processor half of a plugin: it runs on every lane of an array at once, like a
//! built-in direction finder. Implement [`Processor`], call [`crate::export_processor!`] once,
//! and declare it as the manifest's `processor`.

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    Complex,
    wire::{ChannelHealth, DecoderEvent, EventContract, EventKind, plugin::processor::ArrayFacts},
};

pub use crate::wire::plugin::processor::Reset;

/// One block of every lane, each `len()` samples long.
#[derive(Clone, Copy, Debug)]
pub struct Lanes<'a> {
    samples: &'a [Complex<f32>],
    count: usize,
    len: usize,
}

impl<'a> Lanes<'a> {
    /// `samples` holds `count` lanes one after the other.
    #[must_use]
    pub fn new(samples: &'a [Complex<f32>], count: usize) -> Self {
        let len = samples.len().checked_div(count).unwrap_or(0);
        Self {
            samples,
            count,
            len,
        }
    }

    #[must_use]
    pub const fn count(&self) -> usize {
        self.count
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Lane `index`, empty past the last.
    #[must_use]
    pub fn lane(&self, index: usize) -> &'a [Complex<f32>] {
        self.samples
            .get(index * self.len..(index + 1) * self.len)
            .unwrap_or(&[])
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a [Complex<f32>]> + '_ {
        (0..self.count).map(|index| self.lane(index))
    }
}

/// What one block produced: readout values in the order the processor declares them, the
/// health footer and events.
#[derive(Debug, Default)]
pub struct ArrayOutputs {
    pub(crate) values: Vec<f64>,
    pub(crate) report: bool,
    pub health: ChannelHealth,
    pub events: Vec<DecoderEvent>,
    /// Results cut at a limit, counted up over the processor's life.
    pub truncated: u32,
}

impl ArrayOutputs {
    /// Sets readout `index` and sends a report with this block.
    pub fn set(&mut self, index: usize, value: f64) {
        if self.values.len() <= index {
            self.values.resize(index + 1, f64::NAN);
        }
        self.values[index] = value;
        self.report = true;
    }

    /// Clears readout `index` until it is set again.
    pub fn unset(&mut self, index: usize) {
        self.set(index, f64::NAN);
    }

    #[must_use]
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    #[must_use]
    pub const fn reports(&self) -> bool {
        self.report
    }

    pub fn emit<T: EventContract>(&mut self, payload: &T) {
        self.events.push(DecoderEvent::of(payload));
    }

    pub fn emit_as(&mut self, kind: EventKind, payload: &impl Serialize) {
        self.events.push(DecoderEvent::tagged(kind, payload));
    }

    /// Ready for the next block; readout values stay until set again.
    pub fn clear(&mut self) {
        self.report = false;
        self.events.clear();
    }
}

/// One running array processor.
pub trait Processor: Sized {
    type Settings: DeserializeOwned + Default;

    fn open(facts: &ArrayFacts, settings: Self::Settings) -> Result<Self, String>;

    fn apply(&mut self, settings: Self::Settings) -> Result<(), String>;

    /// The array changed rate, tuning or shape. Refuse what this processor cannot follow and the
    /// server opens it again.
    fn retune(&mut self, facts: &ArrayFacts) -> Result<(), String>;

    /// Forget what was accumulated; the samples before and after do not join up.
    fn reset(&mut self, _cause: Reset) {}

    fn process(&mut self, lanes: Lanes<'_>, out: &mut ArrayOutputs);
}

/// The JSON a fresh node of a processor with `S` settings starts on.
#[must_use]
pub fn defaults<S: Serialize + Default>() -> Value {
    serde_json::to_value(S::default()).unwrap_or(Value::Null)
}

/// Exports the array processor ABI for the processor type `$processor`.
#[macro_export]
macro_rules! export_processor {
    ($processor:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_open(ptr: u32, len: u32) -> i32 {
                $crate::abi::processor::open::<$processor>(ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_apply(handle: i32, ptr: u32, len: u32) -> i32 {
                $crate::abi::processor::apply::<$processor>(handle, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_retune(handle: i32, ptr: u32, len: u32) -> i32 {
                $crate::abi::processor::retune::<$processor>(handle, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_reset(handle: i32, cause: u32) {
                $crate::abi::processor::reset::<$processor>(handle, cause);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_input(handle: i32, lanes: u32, samples: u32) -> u32 {
                $crate::abi::processor::input::<$processor>(handle, lanes, samples)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_process(handle: i32, lanes: u32, samples: u32) -> u32 {
                $crate::abi::processor::process::<$processor>(handle, lanes, samples)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_close(handle: i32) {
                $crate::abi::close(handle);
            }
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_split_one_buffer_in_order() {
        let samples: Vec<Complex<f32>> = (0..6).map(|n| Complex::new(n as f32, 0.0)).collect();
        let lanes = Lanes::new(&samples, 2);
        assert_eq!((lanes.count(), lanes.len()), (2, 3));
        assert_eq!(lanes.lane(1)[0].re, 3.0);
        assert!(lanes.lane(2).is_empty());
    }

    #[test]
    fn a_set_value_reports_and_others_stay_unknown() {
        let mut out = ArrayOutputs::default();
        out.set(2, 1.5);
        assert!(out.reports());
        assert!(out.values()[0].is_nan());
        out.clear();
        assert!(!out.reports());
        assert_eq!(out.values()[2], 1.5);
    }
}
