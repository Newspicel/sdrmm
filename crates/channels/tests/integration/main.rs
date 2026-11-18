use sdrmm_test_support::CountingAlloc;

/// Counts per thread, so the allocation tests stay exact beside the rest.
#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

mod channel_perf;
mod common;
mod datv_perf;
mod dmr_ambe_gain;
mod dmr_baseline;
mod dmr_soft_gain;
mod dvbt2_payload;
mod ident;
mod ident_fixtures;
mod signal_id;
mod xng_fixtures;
mod xng_hfdl;
mod xng_vdl2;

/// Every test in this binary runs against the built-in decoders, installed once before main.
#[ctor::ctor(unsafe)]
fn install_for_tests() {
    sdrmm_channels::builtin::install();
}
