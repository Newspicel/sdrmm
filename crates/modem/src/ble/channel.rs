use super::{CHANNEL_SPACING_HZ, RF_CHANNELS};

const BAND_START_HZ: f64 = 2_402_000_000.0;
const ON_GRID_HZ: f64 = 500_000.0;
/// Channel indices of the three advertising channels, at RF channels 0, 12 and 39.
const ADVERTISING: [u8; 3] = [37, 38, 39];

#[must_use]
pub fn rf_channel_hz(rf: u8) -> f64 {
    BAND_START_HZ + f64::from(rf) * CHANNEL_SPACING_HZ
}

#[must_use]
pub fn rf_channel_at(frequency_hz: f64) -> Option<u8> {
    let steps = ((frequency_hz - BAND_START_HZ) / CHANNEL_SPACING_HZ).round();
    let on_grid = (frequency_hz - BAND_START_HZ - steps * CHANNEL_SPACING_HZ).abs() <= ON_GRID_HZ;
    (on_grid && (0.0..f64::from(RF_CHANNELS)).contains(&steps)).then_some(steps as u8)
}

#[must_use]
pub fn channel_index(rf: u8) -> u8 {
    match rf {
        0 => ADVERTISING[0],
        12 => ADVERTISING[1],
        39 => ADVERTISING[2],
        1..=11 => rf - 1,
        _ => rf - 2,
    }
}
