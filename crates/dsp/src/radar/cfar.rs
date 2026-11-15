mod alpha_table;
mod coherence;
mod correlation;
mod detector;
mod plane;
mod range;
mod spec;

pub use alpha_table::AlphaTable;
pub use coherence::LaneCoherence;
pub use correlation::range_correlation;
pub use detector::{Cfar, Hit};
pub use spec::CfarSpec;

pub const MAX_GUARD: usize = 16;
pub const MAX_TRAIN_RANGE: usize = 64;
pub const MAX_TRAIN_DOPPLER: usize = 16;
pub const MAX_LANES: usize = 15;
pub const RHO_TABLE: [f64; 14] = [
    1.0, 1.33, 1.6, 2.0, 2.5, 3.2, 4.0, 5.0, 6.3, 7.0, 8.0, 10.0, 12.6, 14.0,
];
pub const CORRELATION_LAGS: usize = 3;

const MAX_PAD: usize = MAX_GUARD + MAX_TRAIN_DOPPLER;
const CLUTTER_FILL: f32 = 1.0;
const LOOKS_TOLERANCE: f64 = 0.1;

#[cfg(test)]
mod tests;
