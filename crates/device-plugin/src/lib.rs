//! Write an sdr-- radio driver as a native plugin, and load one.
//!
//! A plugin's driver is written exactly like a built-in one, against [`sdrmm_device`]: a
//! [`DeviceDriver`] that opens [`SdrDevice`]s. Declare its backend with
//! [`DeviceDriver::spec`], build the crate as a `cdylib` and call [`export_driver!`] once. The
//! server loads the library from its plugins folder and the radio shows up beside the built-in
//! ones.
//!
//! Control calls cross the boundary as the same wire-type JSON the REST API speaks; samples cross
//! as a pointer and a length, so a plugin streams as fast as a built-in.

pub mod abi;
pub mod guest;
pub mod host;

pub use sdrmm_device::{self as device, DeviceDriver, DeviceError, RxSink, SdrDevice, TxStream};
pub use sdrmm_wire as wire;

#[cfg(test)]
mod tests;
