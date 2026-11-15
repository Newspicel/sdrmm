mod burst;
mod floor;
mod frames;

pub use self::{
    burst::{Burst, Bursts},
    floor::QuietFloor,
    frames::PowerFrames,
};

pub const HANN_BIN_SPREAD: f32 = 1.5;

#[must_use]
pub fn band_degrees(bins: usize) -> f32 {
    2.0 * (bins as f32 / HANN_BIN_SPREAD).max(1.0)
}

#[cfg(test)]
mod tests;
