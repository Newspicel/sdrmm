//! The transmit half of a plugin: a modulator the server feeds payloads and pulls baseband from,
//! like a built-in one. Implement [`Transmitter`], call [`crate::export_transmit!`] once, and set
//! `can_transmit` on the manifest's descriptor.

use serde::de::DeserializeOwned;

use crate::Complex;

/// What the server hands a transmitter to send.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Payload<'a> {
    /// Mono audio at the transmitter's input rate.
    Audio(&'a [f32]),
    Frame(&'a [u8]),
}

/// One running modulator.
pub trait Transmitter: Sized {
    /// The same settings the decoder's form edits; fields the transmitter has no use for are
    /// ignored.
    type Settings: DeserializeOwned + Default;

    /// Opens a modulator producing baseband at `rate_hz`.
    fn open(rate_hz: f64, settings: Self::Settings) -> Result<Self, String>;

    fn apply(&mut self, settings: Self::Settings) -> Result<(), String>;

    /// Queues `payload`; refuse what the mode cannot carry or a backlog past its bound.
    fn submit(&mut self, payload: Payload<'_>) -> Result<(), String>;

    /// Fills `out` with what is queued and returns how many samples it wrote, fewer once the
    /// queue runs dry.
    fn generate(&mut self, out: &mut [Complex<f32>]) -> usize;
}

/// Exports the transmit ABI for the transmitter type `$tx`.
#[macro_export]
macro_rules! export_transmit {
    ($tx:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_tx_open(rate: f64, ptr: u32, len: u32) -> i32 {
                $crate::abi::transmit::open::<$tx>(rate, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_tx_apply(handle: i32, ptr: u32, len: u32) -> i32 {
                $crate::abi::transmit::apply::<$tx>(handle, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_tx_submit(handle: i32, kind: u32, ptr: u32, len: u32) -> i32 {
                $crate::abi::transmit::submit::<$tx>(handle, kind, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_tx_generate(handle: i32, max: u32) -> i64 {
                $crate::abi::transmit::generate::<$tx>(handle, max)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_tx_close(handle: i32) {
                $crate::abi::close(handle);
            }
        };
    };
}
