//! The hybrid FM waveform's geometry at the rate this receiver runs.

/// The NRSC-5 subcarrier spacing, 1 488 375 / 4096 Hz.
pub const SPACING_HZ: f64 = 1_488_375.0 / 4096.0;
/// The transform covers ±279 kHz: the primary sidebands end at ±198 kHz, inside the
/// converter's flat band at this rate, and it is the narrowest rate whose cyclic prefix is
/// still a whole number of samples.
pub const FFT: usize = 1536;
pub const INPUT_RATE_HZ: f64 = SPACING_HZ * FFT as f64;
pub const CP: usize = FFT * 7 / 128;
pub const SYMBOL: usize = FFT + CP;
/// OFDM symbols per L1 block.
pub const BLOCK: usize = 32;
/// L1 blocks per P1 frame.
pub const BLOCKS: usize = 16;
pub const CENTRE: usize = FFT / 2;
/// The outermost reference subcarriers of the lower and upper primary sidebands.
pub const LOWER_EDGE: usize = CENTRE - 546;
pub const UPPER_EDGE: usize = CENTRE + 546;
/// Subcarriers per partition: one reference, then data.
pub const PARTITION: usize = 19;
pub const DATA_CARRIERS: usize = PARTITION - 1;
/// Partitions in each primary main sideband.
pub const MAIN_PARTITIONS: usize = 10;
/// Partitions in each sideband with all four extended partitions on.
pub const MAX_PARTITIONS: usize = 14;
/// Soft bits the primary main sidebands carry per block.
pub const MAIN_BITS: usize = 2 * 2 * MAIN_PARTITIONS * DATA_CARRIERS * BLOCK;
/// Data bits of one P1 frame and one PIDS frame.
pub const P1_BITS: usize = 146_176;
pub const PIDS_BITS: usize = 80;
