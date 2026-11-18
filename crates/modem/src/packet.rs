//! Packet radio receive: an FM channel keyed with 1200 baud AFSK or 9600 baud G3RUH, sliced to
//! HDLC frames. AX.25 and the Benshi BSS chat protocol both ride on it.

mod afsk;
pub mod ax25;
mod g3ruh;
mod keyer;
mod lane;
mod receiver;
mod recent;
mod synth;
#[cfg(test)]
mod tests;

pub use keyer::{Keyer, Line};
pub use receiver::{PacketMode, PacketReceiver};
pub use recent::RecentFrames;
pub use synth::afsk_burst;

pub const AFSK_MARK_HZ: f64 = 1_200.0;
pub const AFSK_SPACE_HZ: f64 = 2_200.0;
pub const AFSK_BAUD: f64 = 1_200.0;
pub const G3RUH_BAUD: f64 = 9_600.0;
pub const DEVIATION_HZ: f64 = 3_000.0;
