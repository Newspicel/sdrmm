use sdrmm_test_support::CountingAlloc;

/// Counts per thread, so the allocation tests stay exact beside the rest.
#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

mod alloc_adaptive;
mod alloc_assign;
mod alloc_beamform;
mod alloc_correlator;
mod alloc_covariance;
mod alloc_doa;
mod alloc_linalg;
mod alloc_manifold;
mod alloc_radar;
mod alloc_stitch;
mod alloc_subspace;
mod alloc_sweep;
mod array_sync_alloc;
mod performance;
