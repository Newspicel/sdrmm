mod band;
mod delay;
mod design;

pub use self::{
    band::{Band, BandFilter},
    delay::Delay,
    design::{design_from_response, design_hilbert, design_vestigial},
};

pub const MAX_TAPS: usize = 1_023;

/// The tap count a band filter builds with: odd, within 3..=MAX_TAPS.
fn odd_taps(taps: usize) -> usize {
    taps.clamp(MIN_TAPS, MAX_TAPS) | 1
}

const MIN_TAPS: usize = 3;

#[cfg(test)]
mod tests;
