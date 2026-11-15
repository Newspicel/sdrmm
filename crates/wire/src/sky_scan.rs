use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    DEFAULT_SIGNAL_MAP_BANDWIDTH_HZ, MAX_SIGNAL_MAP_BANDWIDTH_HZ, MAX_SIGNAL_MAP_OFFSET_HZ,
    channel::{NumberSpec, SettingField},
    present::{Field, Unit, fields, number, text_of},
    satellite::SkyPoint,
};

pub const MAX_SKY_CELLS: usize = 4_096;
pub const MIN_SKY_STEP_DEG: f64 = 1.0;
pub const MAX_SKY_STEP_DEG: f64 = 90.0;
pub const MIN_SKY_DWELL_MS: u32 = 100;
pub const MAX_SKY_DWELL_MS: u32 = 60_000;
pub const MAX_SKY_ELEVATION_DEG: f64 = 90.0;
const TURN: f64 = 360.0;
const KHZ: f64 = 1_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct SkyScanParams {
    pub offset_hz: i64,
    pub bandwidth_hz: u64,
    /// Compass bearing the sweep starts at.
    pub azimuth_from_deg: f64,
    /// How far clockwise it sweeps; 360 goes all the way round.
    pub azimuth_span_deg: f64,
    pub elevation_from_deg: f64,
    pub elevation_to_deg: f64,
    pub step_deg: f64,
    pub dwell_ms: u32,
}

impl Default for SkyScanParams {
    fn default() -> Self {
        Self {
            offset_hz: 0,
            bandwidth_hz: DEFAULT_SIGNAL_MAP_BANDWIDTH_HZ,
            azimuth_from_deg: 0.0,
            azimuth_span_deg: TURN,
            elevation_from_deg: 0.0,
            elevation_to_deg: 0.0,
            step_deg: 10.0,
            dwell_ms: 1_000,
        }
    }
}

impl SkyScanParams {
    #[must_use]
    pub fn problem(&self) -> Option<&'static str> {
        let checks = [
            (
                self.offset_hz.unsigned_abs() <= MAX_SIGNAL_MAP_OFFSET_HZ.unsigned_abs(),
                "Offset out of range",
            ),
            (
                (1..=MAX_SIGNAL_MAP_BANDWIDTH_HZ).contains(&self.bandwidth_hz),
                "Width out of range",
            ),
            (
                (0.0..TURN).contains(&self.azimuth_from_deg),
                "Az from out of range",
            ),
            (
                (MIN_SKY_STEP_DEG..=TURN).contains(&self.azimuth_span_deg),
                "Az span out of range",
            ),
            (
                (0.0..=MAX_SKY_ELEVATION_DEG).contains(&self.elevation_from_deg),
                "El from out of range",
            ),
            (
                (self.elevation_from_deg..=MAX_SKY_ELEVATION_DEG).contains(&self.elevation_to_deg),
                "El to out of range",
            ),
            (
                (MIN_SKY_STEP_DEG..=MAX_SKY_STEP_DEG).contains(&self.step_deg),
                "Step out of range",
            ),
            (
                (MIN_SKY_DWELL_MS..=MAX_SKY_DWELL_MS).contains(&self.dwell_ms),
                "Dwell out of range",
            ),
        ];
        checks
            .iter()
            .find(|(ok, _)| !ok)
            .map(|(_, text)| *text)
            .or_else(|| (self.count() > MAX_SKY_CELLS).then_some("Too many points, widen Step"))
    }

    #[must_use]
    pub fn valid(&self) -> bool {
        self.problem().is_none()
    }

    #[must_use]
    pub fn columns(&self) -> usize {
        let steps = (self.azimuth_span_deg / self.step_deg).floor() as usize;
        if self.azimuth_span_deg >= TURN {
            steps.max(1)
        } else {
            steps + 1
        }
    }

    #[must_use]
    pub fn rows(&self) -> usize {
        ((self.elevation_to_deg - self.elevation_from_deg) / self.step_deg).floor() as usize + 1
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.columns().saturating_mul(self.rows())
    }

    /// Every point to visit, row by row from the lowest, each row swept back the way the last
    /// came so the rotator never swings round between rows.
    #[must_use]
    pub fn points(&self) -> Vec<SkyPoint> {
        if !self.valid() {
            return Vec::new();
        }
        let azimuths = self.columns();
        (0..self.rows())
            .flat_map(|row| {
                let elevation_deg = self.elevation_from_deg + row as f64 * self.step_deg;
                let order: Box<dyn Iterator<Item = usize>> = if row % 2 == 0 {
                    Box::new(0..azimuths)
                } else {
                    Box::new((0..azimuths).rev())
                };
                order.map(move |column| SkyPoint {
                    azimuth_deg: (self.azimuth_from_deg + column as f64 * self.step_deg)
                        .rem_euclid(TURN),
                    elevation_deg,
                })
            })
            .collect()
    }

    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let degrees = |min: f64, max: f64| NumberSpec::new(min, max, 1.0).unit("°");
        let max_offset = MAX_SIGNAL_MAP_OFFSET_HZ as f64;
        vec![
            SettingField::new(
                "offset_hz",
                "Offset",
                NumberSpec::new(-max_offset, max_offset, 100.0).scaled(KHZ, "kHz"),
            )
            .title("From the lane centre")
            .quiet(0),
            SettingField::new(
                "bandwidth_hz",
                "Width",
                NumberSpec::new(1.0, MAX_SIGNAL_MAP_BANDWIDTH_HZ as f64, 100.0).scaled(KHZ, "kHz"),
            )
            .title("Band it measures"),
            SettingField::new("azimuth_from_deg", "Az from", degrees(0.0, TURN - 1.0))
                .title("Compass bearing the sweep starts at"),
            SettingField::new(
                "azimuth_span_deg",
                "Az span",
                degrees(MIN_SKY_STEP_DEG, TURN),
            )
            .title("How far clockwise it sweeps"),
            SettingField::new(
                "elevation_from_deg",
                "El from",
                degrees(0.0, MAX_SKY_ELEVATION_DEG),
            ),
            SettingField::new(
                "elevation_to_deg",
                "El to",
                degrees(0.0, MAX_SKY_ELEVATION_DEG),
            )
            .title("Same as El from for one row along the horizon"),
            SettingField::new(
                "step_deg",
                "Step",
                degrees(MIN_SKY_STEP_DEG, MAX_SKY_STEP_DEG),
            )
            .title("Spacing between points"),
            SettingField::new(
                "dwell_ms",
                "Dwell",
                NumberSpec::new(
                    f64::from(MIN_SKY_DWELL_MS),
                    f64::from(MAX_SKY_DWELL_MS),
                    100.0,
                )
                .unit("ms"),
            )
            .title("How long it listens at each point"),
        ]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SkyScanNode {
    #[serde(default)]
    pub settings: SkyScanParams,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SkyCell {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub level_dbfs: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SkyScanStatus {
    pub node: String,
    pub running: bool,
    /// Bearings in each row of the grid.
    pub columns: u32,
    /// Elevations the grid climbs through.
    pub rows: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<SkyCell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readouts: Vec<Field>,
}

impl SkyScanStatus {
    pub fn shape(&mut self, settings: &SkyScanParams) {
        self.columns = u32::try_from(settings.columns()).unwrap_or(u32::MAX);
        self.rows = u32::try_from(settings.rows()).unwrap_or(u32::MAX);
    }

    #[must_use]
    pub fn peak(&self) -> Option<SkyCell> {
        self.cells
            .iter()
            .copied()
            .max_by(|a, b| a.level_dbfs.total_cmp(&b.level_dbfs))
    }

    #[must_use]
    pub fn with_readouts(mut self) -> Self {
        let peak = self.peak();
        let total = self.columns.saturating_mul(self.rows);
        let progress = format!("{}/{total}", self.cells.len());
        self.readouts = fields(&[
            ("progress", "Points", text_of(progress)),
            (
                "peak_azimuth",
                "Peak az",
                number(peak.map(|cell| cell.azimuth_deg), Unit::Degrees, 0),
            ),
            (
                "peak_elevation",
                "Peak el",
                number(peak.map(|cell| cell.elevation_deg), Unit::Degrees, 0),
            ),
            (
                "peak_level",
                "Peak",
                number(peak.map(|cell| cell.level_dbfs), Unit::Dbfs, 1),
            ),
        ]);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn azimuths(points: &[SkyPoint]) -> Vec<f64> {
        points.iter().map(|point| point.azimuth_deg).collect()
    }

    #[test]
    fn a_full_turn_visits_each_bearing_once() {
        let params = SkyScanParams {
            step_deg: 90.0,
            ..SkyScanParams::default()
        };
        assert_eq!(azimuths(&params.points()), [0.0, 90.0, 180.0, 270.0]);
    }

    #[test]
    fn a_partial_sweep_ends_on_its_last_bearing_across_north() {
        let params = SkyScanParams {
            azimuth_from_deg: 300.0,
            azimuth_span_deg: 120.0,
            step_deg: 60.0,
            ..SkyScanParams::default()
        };
        assert_eq!(azimuths(&params.points()), [300.0, 0.0, 60.0]);
    }

    #[test]
    fn rows_sweep_back_and_forth_from_the_lowest() {
        let params = SkyScanParams {
            azimuth_span_deg: 20.0,
            elevation_to_deg: 10.0,
            step_deg: 10.0,
            ..SkyScanParams::default()
        };
        let points = params.points();
        assert_eq!(azimuths(&points), [0.0, 10.0, 20.0, 20.0, 10.0, 0.0]);
        let elevations: Vec<f64> = points.iter().map(|point| point.elevation_deg).collect();
        assert_eq!(elevations, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    }

    #[test]
    fn nonsense_settings_are_refused() {
        let base = SkyScanParams::default();
        assert!(base.valid());
        for bad in [
            SkyScanParams {
                step_deg: 0.0,
                ..base
            },
            SkyScanParams {
                azimuth_from_deg: 360.0,
                ..base
            },
            SkyScanParams {
                elevation_from_deg: 30.0,
                elevation_to_deg: 10.0,
                ..base
            },
            SkyScanParams {
                dwell_ms: 0,
                ..base
            },
            SkyScanParams {
                bandwidth_hz: 0,
                ..base
            },
            SkyScanParams {
                step_deg: f64::NAN,
                ..base
            },
            SkyScanParams {
                step_deg: 1.0,
                elevation_to_deg: 90.0,
                ..base
            },
        ] {
            assert!(!bad.valid(), "{bad:?}");
            assert!(bad.points().is_empty());
        }
    }

    #[test]
    fn the_peak_is_the_loudest_cell() {
        let cell = |azimuth_deg, level_dbfs| SkyCell {
            azimuth_deg,
            elevation_deg: 0.0,
            level_dbfs,
        };
        let status = SkyScanStatus {
            columns: 3,
            rows: 1,
            cells: vec![cell(0.0, -80.0), cell(90.0, -40.0), cell(180.0, -70.0)],
            ..SkyScanStatus::default()
        }
        .with_readouts();
        assert_eq!(status.peak().map(|peak| peak.azimuth_deg), Some(90.0));
        assert_eq!(status.readouts.len(), 4);
    }
}
