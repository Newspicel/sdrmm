//! Write an sdr-- decoder, and with it a transmitter or an array processor, as a WASM plugin.
//!
//! Implement [`Plugin`] for a type, call [`export!`] once, and build the crate as a `cdylib` for
//! `wasm32-unknown-unknown`. The server loads the `.wasm` from its plugins directory and the
//! decoder shows up beside the built-in ones. `dsp` and `modem` are the same building blocks the
//! built-in decoders use; [`kernels`] runs the heavy ones natively in the host.

pub mod kernels;
mod outputs;
pub mod processor;
pub mod transmit;

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub mod abi;

pub use num_complex::Complex;
pub use outputs::Outputs;
pub use processor::{ArrayOutputs, Lanes, Processor};
pub use sdrmm_dsp as dsp;
pub use sdrmm_modem as modem;
pub use sdrmm_wire as wire;
pub use serde_json;
pub use transmit::{Payload, Transmitter};

use sdrmm_wire::{
    EventKind,
    plugin::{PluginLayout, PluginManifest},
    present::Presentation,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

/// One decoder: what it is, how it is laid out, and a running channel of it.
pub trait Plugin: Sized {
    type Settings: Serialize + DeserializeOwned + Default;

    fn manifest() -> PluginManifest;

    fn layout(settings: &Self::Settings) -> PluginLayout;

    /// Opens a channel fed at `input_rate_hz`.
    fn open(input_rate_hz: f64, settings: Self::Settings) -> Result<Self, String>;

    fn apply(&mut self, settings: Self::Settings) -> Result<(), String>;

    fn process(&mut self, iq: &[Complex<f32>], out: &mut Outputs);

    fn retuned(&mut self) {}

    /// How an event of one of this plugin's kinds reads in a log.
    fn present(kind: &EventKind, data: &Value) -> Result<Presentation, String>;
}

/// Parses settings JSON the way the host will, absent fields taking their defaults.
pub fn settings<S: DeserializeOwned>(json: &[u8]) -> Result<S, String> {
    if json.is_empty() {
        return from_value(Value::Null);
    }
    from_value(serde_json::from_slice(json).map_err(|error| error.to_string())?)
}

/// Settings from parsed JSON, `null` taking every default.
pub fn from_value<S: DeserializeOwned>(value: Value) -> Result<S, String> {
    let value = if value.is_null() {
        Value::Object(serde_json::Map::new())
    } else {
        value
    };
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// Exports the ABI the host calls for the plugin type `$plugin`.
#[macro_export]
macro_rules! export {
    ($plugin:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_abi() -> u32 {
                $crate::wire::plugin::ABI_VERSION
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_alloc(len: u32) -> u32 {
                $crate::abi::alloc(len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_manifest() -> i64 {
                $crate::abi::manifest::<$plugin>()
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_call(op: i32, ptr: u32, len: u32) -> i64 {
                $crate::abi::call::<$plugin>(op, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_open(rate: f64, ptr: u32, len: u32) -> i32 {
                $crate::abi::open::<$plugin>(rate, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_apply(handle: i32, ptr: u32, len: u32) -> i32 {
                $crate::abi::apply::<$plugin>(handle, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_input(handle: i32, samples: u32) -> u32 {
                $crate::abi::input::<$plugin>(handle, samples)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_process(handle: i32, samples: u32) -> u32 {
                $crate::abi::process::<$plugin>(handle, samples)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_retuned(handle: i32) {
                $crate::abi::retuned::<$plugin>(handle);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_close(handle: i32) {
                $crate::abi::close(handle);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_error() -> i64 {
                $crate::abi::error()
            }
        };
    };
}
