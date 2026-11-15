mod kernel;
mod shape;
mod taper;

pub use kernel::{BatchKernel, WeightsAt};
pub use shape::BatchShape;
pub use taper::DopplerTaper;

pub const MIN_FFT: usize = 16;
pub const MAX_FFT: usize = 1 << 20;
pub const MAX_SURVEILLANCE: usize = 15;

#[cfg(test)]
mod tests;
