use sdrmm_test_support::CountingAlloc;

/// Counts per thread, so the allocation tests stay exact beside the rest.
#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

mod alloc_band;
mod alloc_beamformer;
mod alloc_correlator;
mod alloc_df;
mod alloc_passive_radar;
mod alloc_polarimeter;
mod alloc_spatial_spectrum;
mod alloc_stitch;

/// Every test in this binary runs against the built-in processors, installed once before main.
#[ctor::ctor(unsafe)]
fn install_for_tests() {
    sdrmm_array::builtin::install();
}
