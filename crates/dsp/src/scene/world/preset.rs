use super::layout::{circle, line};
use super::{Clutter, DONGLE_SPACING_M, Echo, Emitter, KRAKEN_RADIUS_M, Scene, Waveform};

const DEFAULT_SEED: u64 = 0x5EED_BE7C_0000_0137;
const KRAKEN_ELEMENTS: usize = 5;
const SQUARE_ELEMENTS: usize = 4;
const PAIR_ELEMENTS: usize = 2;

/// A `KrakenSDR` circle, a four-element square on the same radius and a two-dongle line,
/// watching one FM emitter and its echo.
#[must_use]
pub fn default_scene() -> Scene {
    let mut positions = circle(KRAKEN_RADIUS_M, KRAKEN_ELEMENTS);
    positions.extend(circle(KRAKEN_RADIUS_M, SQUARE_ELEMENTS));
    positions.extend(line(DONGLE_SPACING_M, PAIR_ELEMENTS));
    Scene {
        positions,
        emitters: vec![Emitter {
            azimuth_deg: 137.0,
            elevation_deg: 0.0,
            offset_hz: 0.0,
            power_dbfs: -30.0,
            waveform: Waveform::Fm {
                deviation_hz: 75_000.0,
                rate_hz: 1_000.0,
            },
            paths: Vec::new(),
        }],
        echoes: vec![Echo {
            emitter: 0,
            azimuth_deg: 250.0,
            delay_s: 40e-6,
            doppler_hz: 60.0,
            gain_db: -25.0,
        }],
        clutter: Clutter::default(),
        thermal_dbfs: -60.0,
        noise_source_dbfs: -20.0,
        noise_bandwidth: 2_000_000.0,
        seed: DEFAULT_SEED,
    }
}
