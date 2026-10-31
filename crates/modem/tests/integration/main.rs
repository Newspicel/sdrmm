use sdrmm_test_support::CountingAlloc;

/// Counts per thread, so the allocation tests stay exact beside the rest.
#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

mod analog;
mod analog_perf;
mod ft8_tones;
mod gmsk_msk_afsk_bundles;
mod linear_bundles;
mod mfsk_cpfsk;
mod mfsk_perf;
mod multicarrier;
mod multicarrier_perf;
mod ofdm;
mod ofdm_perf;
mod orthogonal_mfsk;
mod orthogonal_ppm_perf;
mod packet_phys;
mod ppm;
mod spread;
mod spread_perf;
