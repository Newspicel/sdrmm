//! The encoder of a plugin: the server feeds it payloads and pulls baseband from it, like a
//! built-in one. Implement [`Encoder`], call [`crate::export_encoder!`] once, and declare it as
//! the manifest's `encoder`.

use serde::de::DeserializeOwned;

use crate::Complex;

/// What the server hands an encoder to send.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Payload<'a> {
    /// Mono audio at the encoder's rate.
    Audio(&'a [f32]),
    Frame(&'a [u8]),
}

/// One running encoder.
pub trait Encoder: Sized {
    /// The settings the encoder's form edits.
    type Settings: DeserializeOwned + Default;

    /// Opens an encoder producing baseband at `rate_hz`, its descriptor's rate.
    fn open(rate_hz: f64, settings: Self::Settings) -> Result<Self, String>;

    /// Queues `payload`; refuse what the mode cannot carry or a backlog past its bound.
    fn submit(&mut self, payload: Payload<'_>) -> Result<(), String>;

    /// Fills `out` with what is queued and returns how many samples it wrote, fewer once the
    /// queue runs dry.
    fn generate(&mut self, out: &mut [Complex<f32>]) -> usize;
}

/// Exports the encoder ABI for the encoder type `$tx`.
#[macro_export]
macro_rules! export_encoder {
    ($tx:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_encoder_open(rate: f64, ptr: u32, len: u32) -> i32 {
                $crate::abi::encoder::open::<$tx>(rate, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_encoder_submit(
                handle: i32,
                kind: u32,
                ptr: u32,
                len: u32,
            ) -> i32 {
                $crate::abi::encoder::submit::<$tx>(handle, kind, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_encoder_generate(handle: i32, max: u32) -> i64 {
                $crate::abi::encoder::generate::<$tx>(handle, max)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_encoder_close(handle: i32) {
                $crate::abi::close(handle);
            }
        };
    };
}
