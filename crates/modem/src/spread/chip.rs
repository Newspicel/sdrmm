mod read;
mod shaper;
mod timing;

pub use self::{
    read::{integrate, integrate_chips, sample_at},
    shaper::{ChipShaper, find_burst},
    timing::ChipTiming,
};

pub const TIMING_BW: f64 = 0.01;
