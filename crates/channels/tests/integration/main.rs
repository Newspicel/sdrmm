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
mod channel_perf;
mod common;
mod datv_perf;
mod dmr_ambe_gain;
mod dmr_baseline;
mod dmr_soft_gain;
mod dvbt2_payload;
mod ident_fixtures;
mod xng_fixtures;
mod xng_hfdl;
mod xng_vdl2;
