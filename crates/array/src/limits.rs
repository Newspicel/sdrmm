//! Bands every processor that listens to part of the array's span shares.

use sdrmm_wire::limits::{BandLimits, Bounds};
use sdrmm_wire::processor::MIN_BAND_HZ;

use crate::df::{MAX_DF_BANDWIDTH_HZ, MAX_DF_OFFSET_HZ, MIN_DF_BANDWIDTH_HZ};

const OFFSET_HZ: Bounds<f64> = Bounds::new(-MAX_DF_OFFSET_HZ, MAX_DF_OFFSET_HZ);

pub(crate) const NARROW_BAND: BandLimits = BandLimits {
    offset_hz: OFFSET_HZ,
    bandwidth_hz: Bounds::new(MIN_DF_BANDWIDTH_HZ, MAX_DF_BANDWIDTH_HZ),
};

pub(crate) const WIDE_BAND: BandLimits = BandLimits {
    offset_hz: OFFSET_HZ,
    bandwidth_hz: Bounds::new(MIN_BAND_HZ, MAX_DF_BANDWIDTH_HZ),
};

pub(crate) fn power_of_two_in(value: u32, bounds: Bounds<u32>) -> bool {
    value.is_power_of_two() && bounds.contains(value)
}

pub(crate) fn band_holds(band: BandLimits, bandwidth_hz: Option<f64>) -> bool {
    bandwidth_hz.is_none_or(|hz| band.bandwidth_hz.contains(hz))
}

#[cfg(test)]
mod tests;
