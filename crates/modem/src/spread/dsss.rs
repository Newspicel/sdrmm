mod demod;
mod modulator;
mod params;
#[cfg(test)]
mod tests;

pub use self::{
    demod::{Acquisition, DsssDemod},
    modulator::DsssMod,
    params::DsssParams,
};

pub const TRACK_BW: f64 = 0.02;

pub const MAX_CHIPS: usize = 4_096;
