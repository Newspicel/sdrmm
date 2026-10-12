use sdrmm_wire::survey::{SURVEY_CELL_M, SurveyCell, SurveyGrid, SurveyStop, SurveyUpdate};

use crate::ui::map::{
    Geo,
    heat::Ramp,
    overlay::{Dot, EDGE, Heat, Overlay},
};

pub const SIGNAL_MIN_DBFS: f64 = -120.0;
pub const SIGNAL_MAX_DBFS: f64 = -20.0;
pub const RAMP_CSS: &str = "linear-gradient(to right, #231942, #5e2b83, #b33f62, #ef8354, #f6d365)";
const METRES_PER_DEG_LON: f64 = 111_320.0;
const METRES_PER_DEG_LAT: f64 = 110_540.0;
const HEAT_MAX_ZOOM: f64 = 17.0;
const POINT_MIN_ZOOM: f64 = 13.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyState {
    pub cells: Vec<SurveyCell>,
    pub recording: bool,
    pub level_dbfs: Option<f32>,
    pub target_hz: Option<f64>,
    pub dropped: u64,
    pub stopped: Option<SurveyStop>,
    pub behind: bool,
}

#[must_use]
pub fn cell_key(cell: &SurveyCell) -> (i64, i64, i64) {
    let x = cell.longitude * METRES_PER_DEG_LON * cell.latitude.to_radians().cos().max(0.01);
    let y = cell.latitude * METRES_PER_DEG_LAT;
    (
        cell.frequency_hz.round() as i64,
        (x / SURVEY_CELL_M).round() as i64,
        (y / SURVEY_CELL_M).round() as i64,
    )
}

pub fn with_cell(cells: &mut Vec<SurveyCell>, cell: SurveyCell, held: usize) {
    let key = cell_key(&cell);
    match cells.iter_mut().find(|existing| cell_key(existing) == key) {
        Some(existing) => *existing = cell,
        None => cells.push(cell),
    }
    let overflow = cells.len().saturating_sub(held);
    cells.drain(..overflow);
}

#[must_use]
pub fn followed(previous: &SurveyState, update: &SurveyUpdate) -> SurveyState {
    let held = update.cells as usize;
    let mut cells = previous.cells.clone();
    match &update.cell {
        Some(cell) => with_cell(&mut cells, cell.clone(), held),
        None => {
            let overflow = cells.len().saturating_sub(held);
            cells.drain(..overflow);
        }
    }
    let (level_dbfs, target_hz) = if update.stopped == Some(SurveyStop::RadioGone) {
        (None, None)
    } else if update.target_hz.is_none() {
        (previous.level_dbfs, previous.target_hz)
    } else {
        (update.level_dbfs, update.target_hz)
    };
    let stopped = update.stopped.or(if update.recording {
        None
    } else {
        previous.stopped
    });
    SurveyState {
        behind: held > cells.len(),
        cells,
        recording: update.recording,
        level_dbfs,
        target_hz,
        dropped: update.dropped,
        stopped,
    }
}

#[must_use]
pub fn seeded(previous: &SurveyState, grid: SurveyGrid) -> SurveyState {
    SurveyState {
        cells: grid.cells,
        recording: grid.recording,
        dropped: grid.dropped,
        behind: false,
        ..previous.clone()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurveyView {
    pub radio_wired: bool,
    pub position_wired: bool,
    pub positioned: bool,
    pub target_hz: Option<f64>,
    pub level_dbfs: Option<f32>,
    pub recording: bool,
    pub retuned: bool,
}

#[must_use]
pub fn retuned_since(cells: &[SurveyCell], target_hz: Option<f64>) -> bool {
    cells
        .first()
        .zip(target_hz)
        .is_some_and(|(first, target)| first.frequency_hz.round() != target.round())
}

#[must_use]
pub fn can_start(view: &SurveyView) -> bool {
    view.radio_wired
        && view.position_wired
        && view.positioned
        && view.level_dbfs.is_some()
        && !view.retuned
}

#[must_use]
pub fn status(view: &SurveyView) -> &'static str {
    if !view.radio_wired {
        "Wire a radio"
    } else if !view.position_wired {
        "Wire a position"
    } else if view.recording {
        "Recording each new GPS fix"
    } else if view.target_hz.is_none() {
        "Waiting for the radio"
    } else if view.retuned {
        "Retuned: clear to start again"
    } else if view.level_dbfs.is_none() {
        "Offset is outside the IQ span"
    } else if view.positioned {
        "Ready"
    } else {
        "Waiting for GPS"
    }
}

#[must_use]
pub fn offset_limit_hz(span_hz: f64, bandwidth_hz: f64) -> f64 {
    if !span_hz.is_finite() || !bandwidth_hz.is_finite() {
        return 0.0;
    }
    ((span_hz - bandwidth_hz) / 2.0).floor().max(0.0)
}

fn fixed(value: f64, digits: usize) -> String {
    let scale = 10f64.powi(digits as i32);
    format!("{:.digits$}", (value * scale).round() / scale)
}

#[must_use]
pub fn csv(cells: &[SurveyCell], offset_hz: i64, bandwidth_hz: u64) -> String {
    let mut out = String::from(
        "time,frequency_hz,offset_hz,bandwidth_hz,latitude,longitude,accuracy_m,level_dbfs,observations",
    );
    for cell in cells {
        let accuracy = cell
            .accuracy_m
            .map(|value| fixed(value, 1))
            .unwrap_or_default();
        out.push('\n');
        out.push_str(&format!(
            "{},{},{offset_hz},{bandwidth_hz},{},{},{accuracy},{},{}",
            cell.measured_at,
            cell.frequency_hz,
            fixed(cell.latitude, 7),
            fixed(cell.longitude, 7),
            fixed(f64::from(cell.level_dbfs), 2),
            cell.observations,
        ));
    }
    out
}

fn level_colour(level: f64) -> u32 {
    let ramp = Ramp {
        stops: vec![
            (SIGNAL_MIN_DBFS, 0x23_19_42, 1.0),
            (-95.0, 0x5e_2b_83, 1.0),
            (-70.0, 0xb3_3f_62, 1.0),
            (-45.0, 0xef_83_54, 1.0),
            (SIGNAL_MAX_DBFS, 0xf6_d3_65, 1.0),
        ],
    };
    ramp.at(level).0
}

#[must_use]
pub fn overlay(cells: &[SurveyCell]) -> Overlay {
    let span = SIGNAL_MAX_DBFS - SIGNAL_MIN_DBFS;
    let weight = |level: f64| 0.05 + 0.95 * ((level - SIGNAL_MIN_DBFS) / span).clamp(0.0, 1.0);
    let mut out = Overlay::default();
    if cells.is_empty() {
        return out;
    }
    let at = |cell: &SurveyCell| Geo::new(cell.latitude, cell.longitude);
    out.heat.push(Heat {
        points: cells
            .iter()
            .map(|cell| (at(cell), weight(f64::from(cell.level_dbfs))))
            .collect(),
        radius: vec![(0.0, 3.0), (15.0, 20.0)],
        intensity: vec![(0.0, 0.35), (15.0, 1.25)],
        opacity: vec![(13.0, 0.8), (17.0, 0.2)],
        ramp: Ramp {
            stops: vec![
                (0.0, 0x23_19_42, 0.0),
                (0.15, 0x23_19_42, 1.0),
                (0.35, 0x5e_2b_83, 1.0),
                (0.55, 0xb3_3f_62, 1.0),
                (0.75, 0xef_83_54, 1.0),
                (1.0, 0xf6_d3_65, 1.0),
            ],
        },
        max_zoom: HEAT_MAX_ZOOM,
    });
    for cell in cells {
        out.dots.push(Dot {
            radius: vec![(13.0, 2.0), (17.0, 6.0)],
            stroke: EDGE,
            min_zoom: POINT_MIN_ZOOM,
            fade: vec![(13.0, 0.0), (15.0, 0.9)],
            ..Dot::plain(at(cell), 2.0, level_colour(f64::from(cell.level_dbfs)))
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(lat: f64, lon: f64, frequency_hz: f64, level_dbfs: f32) -> SurveyCell {
        SurveyCell {
            latitude: lat,
            longitude: lon,
            frequency_hz,
            level_dbfs,
            measured_at: "2026-08-15T10:00:00Z".to_owned(),
            observations: 1,
            accuracy_m: None,
        }
    }

    fn update(cells: u32, cell: Option<SurveyCell>) -> SurveyUpdate {
        SurveyUpdate {
            level_dbfs: Some(-50.0),
            target_hz: Some(145_500_000.0),
            recording: true,
            cells,
            dropped: 0,
            cell,
            stopped: None,
        }
    }

    #[test]
    fn a_cell_in_the_same_ten_metres_replaces_the_old_one() {
        let mut cells = vec![cell(52.52, 13.405, 145_500_000.0, -40.0)];
        with_cell(
            &mut cells,
            cell(52.520_001, 13.405_001, 145_500_000.0, -60.0),
            10,
        );
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].level_dbfs, -60.0);
        with_cell(&mut cells, cell(52.52, 13.405, 145_525_000.0, -41.0), 10);
        assert_eq!(cells.len(), 2);
        with_cell(&mut cells, cell(52.6, 13.405, 145_525_000.0, -41.0), 2);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].frequency_hz, 145_525_000.0);
    }

    #[test]
    fn an_update_follows_the_server_and_notices_it_fell_behind() {
        let start = SurveyState::default();
        let next = followed(
            &start,
            &update(1, Some(cell(52.52, 13.405, 145_500_000.0, -40.0))),
        );
        assert_eq!(next.cells.len(), 1);
        assert!(next.recording && !next.behind);
        assert_eq!(next.target_hz, Some(145_500_000.0));
        let missed = followed(&next, &update(3, None));
        assert!(missed.behind);
        let quiet = followed(
            &next,
            &SurveyUpdate {
                target_hz: None,
                level_dbfs: None,
                ..update(1, None)
            },
        );
        assert_eq!(quiet.level_dbfs, Some(-50.0));
    }

    #[test]
    fn a_lost_radio_stops_and_forgets_the_level() {
        let held = followed(&SurveyState::default(), &update(0, None));
        let gone = followed(
            &held,
            &SurveyUpdate {
                recording: false,
                stopped: Some(SurveyStop::RadioGone),
                ..update(0, None)
            },
        );
        assert_eq!((gone.level_dbfs, gone.target_hz), (None, None));
        assert_eq!(gone.stopped, Some(SurveyStop::RadioGone));
        let idle = followed(
            &gone,
            &SurveyUpdate {
                recording: false,
                ..update(0, None)
            },
        );
        assert_eq!(idle.stopped, Some(SurveyStop::RadioGone));
        assert_eq!(followed(&gone, &update(0, None)).stopped, None);
    }

    #[test]
    fn a_seed_replaces_the_cells_and_keeps_the_live_level() {
        let live = followed(&SurveyState::default(), &update(5, None));
        let grid = SurveyGrid {
            node: "survey".to_owned(),
            frequency_hz: None,
            offset_hz: 0,
            bandwidth_hz: 12_500,
            recording: false,
            cells: vec![cell(52.52, 13.405, 145_500_000.0, -40.0)],
            dropped: 4,
        };
        let next = seeded(&live, grid);
        assert_eq!(next.cells.len(), 1);
        assert_eq!(next.dropped, 4);
        assert!(!next.behind && !next.recording);
        assert_eq!(next.level_dbfs, Some(-50.0));
    }

    fn view() -> SurveyView {
        SurveyView {
            radio_wired: true,
            position_wired: true,
            positioned: true,
            target_hz: Some(145_500_000.0),
            level_dbfs: Some(-50.0),
            recording: false,
            retuned: false,
        }
    }

    #[test]
    fn the_status_names_what_is_missing_first() {
        assert_eq!(status(&SurveyView::default()), "Wire a radio");
        let unplaced = SurveyView {
            position_wired: false,
            ..view()
        };
        assert_eq!(status(&unplaced), "Wire a position");
        let silent = SurveyView {
            target_hz: None,
            level_dbfs: None,
            ..view()
        };
        assert_eq!(status(&silent), "Waiting for the radio");
        let retuned = SurveyView {
            retuned: true,
            ..view()
        };
        assert_eq!(status(&retuned), "Retuned: clear to start again");
        assert!(!can_start(&retuned));
        let outside = SurveyView {
            level_dbfs: None,
            ..view()
        };
        assert_eq!(status(&outside), "Offset is outside the IQ span");
        let lost = SurveyView {
            positioned: false,
            ..view()
        };
        assert_eq!(status(&lost), "Waiting for GPS");
        assert!(!can_start(&lost));
        assert_eq!(status(&view()), "Ready");
        assert!(can_start(&view()));
    }

    #[test]
    fn a_retune_is_a_different_whole_hertz() {
        let cells = [cell(52.52, 13.405, 145_500_000.2, -40.0)];
        assert!(!retuned_since(&cells, Some(145_500_000.0)));
        assert!(retuned_since(&cells, Some(145_525_000.0)));
        assert!(!retuned_since(&[], Some(1.0)));
        assert!(!retuned_since(&cells, None));
    }

    #[test]
    fn the_offset_keeps_the_whole_width_inside_the_span() {
        assert_eq!(offset_limit_hz(1_000_000.0, 12_500.0), 493_750.0);
        assert_eq!(offset_limit_hz(10_000.0, 12_500.0), 0.0);
    }

    #[test]
    fn the_export_carries_units_and_observation_counts() {
        let exported = csv(
            &[SurveyCell {
                observations: 3,
                accuracy_m: Some(4.25),
                ..cell(52.52, 13.405, 145_500_000.0, -67.125)
            }],
            -25_000,
            12_500,
        );
        assert!(exported.contains("frequency_hz,offset_hz,bandwidth_hz"));
        assert!(
            exported.contains(
                "2026-08-15T10:00:00Z,145500000,-25000,12500,52.5200000,13.4050000,4.3,-67.13,3"
            ),
            "{exported}"
        );
    }

    #[test]
    fn a_survey_draws_a_heatmap_that_stops_and_points_that_fade_in() {
        let drawn = overlay(&[cell(52.52, 13.405, 145_500_000.0, -50.0)]);
        assert_eq!(drawn.heat.len(), 1);
        assert_eq!(drawn.heat[0].max_zoom, HEAT_MAX_ZOOM);
        assert_eq!(drawn.dots.len(), 1);
        assert_eq!(drawn.dots[0].opacity(13.0), 0.0);
        assert!((drawn.dots[0].opacity(15.0) - 0.9).abs() < 1e-6);
        assert!(overlay(&[]).heat.is_empty());
    }
}
