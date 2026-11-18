mod burst;
mod floor;
mod frames;
mod grid;

pub use self::{
    burst::{Burst, Bursts},
    floor::QuietFloor,
    frames::PowerFrames,
    grid::{
        BIN_HZ, EDGE_HZ, FLOOR_RISE_DB, bin_offset_hz, bins_between, fft_len, frame_s, frames_in,
        gated, occupied_band, threshold, usable_half_hz, warmup_frames,
    },
};

pub const HANN_BIN_SPREAD: f32 = 1.5;

#[must_use]
pub fn band_degrees(bins: usize) -> f32 {
    2.0 * (bins as f32 / HANN_BIN_SPREAD).max(1.0)
}

#[cfg(test)]
mod tests;
