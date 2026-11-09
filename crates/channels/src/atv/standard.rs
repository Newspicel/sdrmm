use super::{AtvColor, AtvStandard};

pub(super) const PAL_SUBCARRIER_HZ: f64 = 4_433_618.75;
pub(super) const NTSC_SUBCARRIER_HZ: f64 = 315e6 / 88.0;
pub(super) const CHROMA_BANDWIDTH_HZ: f64 = 1_300_000.0;

/// Where a line's parts sit, in seconds from the sync leading edge, and how the levels relate.
#[derive(Clone, Copy, Debug)]
struct Seconds {
    sync: f64,
    burst: (f64, f64),
    active: (f64, f64),
    front_porch: f64,
    field_lines: u16,
    picture_delay: u16,
    white: f32,
    setup: f32,
    video_rate_hz: f64,
}

fn seconds(standard: AtvStandard) -> Seconds {
    match standard {
        AtvStandard::Ccir625 => Seconds {
            sync: 4.7e-6,
            burst: (5.6e-6, 7.85e-6),
            active: (10.5e-6, 62.5e-6),
            front_porch: 1.65e-6,
            field_lines: 288,
            picture_delay: 22,
            white: 7.0 / 3.0,
            setup: 0.0,
            video_rate_hz: 4.0 * PAL_SUBCARRIER_HZ,
        },
        AtvStandard::Eia525 => Seconds {
            sync: 4.7e-6,
            burst: (5.3e-6, 7.8e-6),
            active: (9.4e-6, 62.0e-6),
            front_porch: 1.5e-6,
            field_lines: 240,
            picture_delay: 18,
            white: 2.5,
            setup: 7.5 / 40.0,
            video_rate_hz: 4.0 * NTSC_SUBCARRIER_HZ,
        },
        AtvStandard::SystemA405 => Seconds {
            sync: 9.0e-6,
            burst: (9.9e-6, 12.15e-6),
            active: (14.8e-6, 97.0e-6),
            front_porch: 1.75e-6,
            field_lines: 188,
            picture_delay: 14,
            white: 7.0 / 3.0,
            setup: 0.0,
            video_rate_hz: 1000.0 * AtvStandard::SystemA405.line_rate_hz(),
        },
    }
}

pub(super) fn subcarrier_hz(color: AtvColor) -> Option<f64> {
    match color {
        AtvColor::Monochrome => None,
        AtvColor::Pal => Some(PAL_SUBCARRIER_HZ),
        AtvColor::Ntsc => Some(NTSC_SUBCARRIER_HZ),
    }
}

/// The rate lines are cut at: four times the subcarrier for colour, so chroma demodulates
/// with signs alone, and the standard's own rate for monochrome, never above the input.
pub(super) fn video_rate_hz(standard: AtvStandard, color: AtvColor, input_rate: f64) -> f64 {
    subcarrier_hz(color).map_or_else(
        || seconds(standard).video_rate_hz.min(input_rate),
        |carrier| 4.0 * carrier,
    )
}

/// One standard's line geometry in video-rate samples.
#[derive(Clone, Copy, Debug)]
pub(super) struct Geometry {
    pub rate: f64,
    pub line: f64,
    pub sync: f64,
    pub burst: (f64, f64),
    pub active: (f64, f64),
    pub front_porch: f64,
    pub width: u16,
    pub field_lines: u16,
    pub picture_delay: u16,
    /// White over the blanking-to-tip step, so levels follow the sync whatever the deviation.
    pub white: f32,
    pub setup: f32,
}

impl Geometry {
    pub fn new(standard: AtvStandard, rate: f64) -> Self {
        let s = seconds(standard);
        let at = |t: f64| t * rate;
        Self {
            rate,
            line: rate / standard.line_rate_hz(),
            sync: at(s.sync),
            burst: (at(s.burst.0), at(s.burst.1)),
            active: (at(s.active.0), at(s.active.1)),
            front_porch: at(s.front_porch),
            width: at(s.active.1 - s.active.0).round() as u16,
            field_lines: s.field_lines,
            picture_delay: s.picture_delay,
            white: s.white,
            setup: s.setup,
        }
    }

    pub fn us(&self, micros: f64) -> f64 {
        micros * 1e-6 * self.rate
    }
}
