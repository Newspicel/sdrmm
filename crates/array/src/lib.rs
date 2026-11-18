//! Array processors: the framework they run in and the built-in ones.

mod limits;
mod present;
mod processor;

pub mod band;
pub mod beamformer;
pub mod builtin;
pub mod correlator;
pub mod df;
pub mod hunt_sweep;
pub mod kernel;
pub mod passive_radar;
pub mod polarimeter;
pub mod pose_clock;
pub mod spatial_spectrum;
pub mod stitch;

pub use processor::*;

/// Counts per thread, so the allocation tests stay exact beside the rest.
#[cfg(test)]
#[global_allocator]
static ALLOC: sdrmm_test_support::CountingAlloc = sdrmm_test_support::CountingAlloc::new();

#[cfg(test)]
/// Every test in this binary runs against the built-in processors, installed once before main.
#[ctor::ctor(unsafe)]
fn install_for_tests() {
    crate::builtin::install();
}
