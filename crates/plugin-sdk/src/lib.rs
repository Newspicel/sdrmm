//! Write an `SDRmm` decoder, encoder or array processor, or any mix of them, as a WASM plugin.
//!
//! A plugin's decoder and encoder are written exactly like built-in ones, against
//! [`sdrmm_decoder`]: a [`Channel`] opened through [`NativeChannel`] and declared with the
//! [`Native`] builder, a [`Modulator`] declared with [`NativeEncoder`]. Implement [`Plugin`] to
//! hand them over and call [`export!`] once; an array processor implements
//! [`array::ArrayProcessor`], exactly like a built-in one, and adds [`export_processor!`]. Build the crate as a `cdylib` for `wasm32-unknown-unknown`. The
//! server loads the `.wasm` from its plugins directory and each part shows up beside the
//! built-in ones. `dsp`, `modem`, `codec`, `crypto`, `aviation` and `array` are the same
//! building blocks the built-ins use; [`kernels`] runs the heavy ones natively in the host.

mod guest;
pub mod kernels;
mod output;
pub mod processor;

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub mod abi;

pub use guest::Parts;
pub use num_complex::Complex;
pub use output::OutputWire;
pub use processor::{Hosted, Staged, defaults};
pub use sdrmm_array as array;
pub use sdrmm_aviation as aviation;
pub use sdrmm_codec as codec;
pub use sdrmm_crypto as crypto;
pub use sdrmm_decoder::{
    self as decoder, Channel, ChannelCtx, ChannelError, ChannelOutputs, DecodedImage,
    NativeChannel, SymbolTap, VideoPicture,
    encoder::{
        Encoder, EncoderBase, EncoderError, Modulator, NativeEncoder, NativeModulator, TxPayload,
    },
    registry::{Decoder, Native},
};
pub use sdrmm_dsp as dsp;
pub use sdrmm_modem as modem;
pub use sdrmm_wire as wire;
pub use serde_json;

use sdrmm_wire::{KindSpec, ProcessorType};
use serde::de::DeserializeOwned;
use serde_json::Value;

/// What a plugin brings. Every part is optional; the manifest lists those it returns.
pub trait Plugin {
    /// The decoder, built like a built-in one, usually with [`Native`].
    fn decoder() -> Option<Box<dyn Decoder>> {
        None
    }

    /// The encoder, built like a built-in one, usually with [`NativeEncoder`].
    fn encoder() -> Option<Box<dyn Encoder>> {
        None
    }

    /// The array processor [`export_processor!`] runs.
    fn processor() -> Option<ProcessorType> {
        None
    }

    /// Event kinds the decoder does not declare itself, such as the processor's.
    fn kinds() -> Vec<KindSpec> {
        Vec::new()
    }
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

/// Exports the plugin type `$plugin`: what every plugin has, and the decoder and encoder ABI
/// its parts run through.
#[macro_export]
macro_rules! export {
    ($plugin:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            fn parts() -> &'static $crate::Parts {
                $crate::abi::parts::<$plugin>()
            }

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
                $crate::abi::manifest(parts())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_present(ptr: u32, len: u32) -> i64 {
                $crate::abi::present(parts(), ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_error() -> i64 {
                $crate::abi::error()
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_call(op: i32, ptr: u32, len: u32) -> i64 {
                $crate::abi::call(parts(), op, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_open(rate: f64, ptr: u32, len: u32) -> i32 {
                $crate::abi::open(parts(), rate, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_apply(handle: i32, ptr: u32, len: u32) -> i32 {
                $crate::abi::apply(handle, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_input(handle: i32, samples: u32) -> u32 {
                $crate::abi::input(handle, samples)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_process(handle: i32, samples: u32, flags: u32) -> u32 {
                $crate::abi::process(handle, samples, flags)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_retuned(handle: i32) {
                $crate::abi::retuned(handle);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_position(handle: i32, ptr: u32, len: u32) {
                $crate::abi::position(handle, ptr, len);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_clock(handle: i32, unix_ns: i64) {
                $crate::abi::clock(handle, unix_ns);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_lo_artifact(handle: i32, offset_hz: f64) {
                $crate::abi::lo_artifact(handle, offset_hz);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_gated(handle: i32) -> i32 {
                $crate::abi::gated(handle)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_close(handle: i32) {
                $crate::abi::close(handle);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_encoder_open(ptr: u32, len: u32) -> i32 {
                $crate::abi::encoder::open(parts(), ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_encoder_submit(
                handle: i32,
                kind: u32,
                ptr: u32,
                len: u32,
            ) -> i32 {
                $crate::abi::encoder::submit(handle, kind, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_encoder_generate(handle: i32, max: u32) -> i64 {
                $crate::abi::encoder::generate(handle, max)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_encoder_close(handle: i32) {
                $crate::abi::close(handle);
            }
        };
    };
}
