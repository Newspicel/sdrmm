//! ACARS blocks and the application messages inside them.

pub mod adsc;
pub mod airline5z;
mod app;
pub mod arinc622;
pub mod block;
pub mod cfb;
mod codec;
pub mod fpn;
pub mod media_adv;
mod message;
pub mod met;
pub mod miam;
pub mod min;
pub mod ohma;
pub mod oooi;
pub mod position;
pub mod qseries;
#[cfg(test)]
mod real_messages;
pub mod reasm;
pub mod sublabel;
#[cfg(test)]
mod xng_equivalence;

#[cfg(test)]
pub use app::summary;
pub use app::{AcarsApp, AppDecode, decode};
pub use message::AcarsCore;
