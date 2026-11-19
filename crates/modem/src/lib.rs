pub mod analog;
pub mod ble;
pub mod ccsds;
pub mod constellation;
pub mod cpm;
pub mod dvb;
pub mod ffsk;
pub mod framesync;
pub mod ita2;
pub mod jpeg;
pub mod k32;
pub mod keying;
pub mod linear;
pub mod morse;
pub mod mpegts;
pub mod multicarrier;
pub mod ofdm;
pub mod orthogonal;
pub mod packet;
pub mod ppm;
pub mod pulse;
pub mod quality;
pub mod soft;
pub mod split_phase;
pub mod spread;
pub mod ssdv;
pub mod symbolcode;
pub mod synth;
pub mod varicode;
pub mod wifi;
pub mod xrit;

#[cfg(test)]
mod testutil;

#[cfg(test)]
#[global_allocator]
static ALLOC: sdrmm_test_support::CountingAlloc = sdrmm_test_support::CountingAlloc::new();
