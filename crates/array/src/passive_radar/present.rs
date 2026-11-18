use sdrmm_wire::channel::{ChannelHealth, ChannelStatus};
use sdrmm_wire::present::{Unit, count, fields, number, percent, text_of};
use sdrmm_wire::processor::view::{
    Cell, Flag, Headline, Mark, MarkShape, ProcessorView, SurfaceView, TableRow, TableView,
};

use crate::Report;
use crate::present::db;
use sdrmm_wire::{RadarAoa, RadarProblem, RadarTrack, RadarUpdate, ReferenceMode, TrackState};

const TRACK_ROWS: usize = 6;

const TRACK_HEADS: [&str; 6] = ["Track", "km", "m/s", "Bearing", "dB", "Truth"];

fn reference_text(update: &RadarUpdate) -> String {
    let reference = update.health.reference;
    match (reference.mode, reference.locked) {
        (ReferenceMode::Raw, _) => "Raw".to_owned(),
        (_, false) => "Lost".to_owned(),
        (ReferenceMode::Cma, true) => format!("CMA {:.0} dB", reference.quality_db),
        (ReferenceMode::DabRemod, true) => format!("DAB {:.0} dB", reference.quality_db),
    }
}

fn mean(values: &[f32]) -> Option<f32> {
    (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
}

const fn chip_only(problem: &RadarProblem) -> Option<&'static str> {
    match problem {
        RadarProblem::TableOutOfRange => Some("Bearings use the ideal geometry"),
        _ => None,
    }
}

fn bearing_cell(aoa: Option<&RadarAoa>) -> Cell {
    match aoa {
        None => Cell::of("-"),
        Some(RadarAoa {
            bearing_deg: Some(deg),
            ..
        }) => Cell::of(format!("{deg:.0}°")),
        Some(aoa) => Cell {
            text: format!("{:.0}°", aoa.azimuth_deg),
            faint: true,
            title: Some("Array frame".to_owned()),
        },
    }
}

fn truth_text(track: &RadarTrack) -> String {
    track.truth.as_ref().map_or_else(
        || "-".to_owned(),
        |truth| truth.callsign.clone().unwrap_or_else(|| truth.id.clone()),
    )
}

fn track_row(track: &RadarTrack) -> TableRow {
    let coasting = track.state == TrackState::Coasting;
    TableRow {
        key: track.id,
        cells: vec![
            Cell::of(format!("T{}", track.id)),
            Cell::of(format!("{:.1}", track.range_km)),
            Cell::of(format!("{:.0}", track.range_rate_mps)),
            bearing_cell(track.aoa.as_ref()),
            Cell::of(format!("{:.0}", track.snr_db)),
            Cell::of(truth_text(track)),
        ],
        title: Some(
            if coasting {
                "Coasting"
            } else {
                "Show on the plot"
            }
            .to_owned(),
        ),
        dim: coasting,
        pick: true,
    }
}

fn track_mark(track: &RadarTrack) -> Mark {
    Mark {
        x: f64::from(track.range_km),
        y: f64::from(track.doppler_hz),
        shape: MarkShape::Square,
        key: Some(track.id),
        label: Some(format!("T{}", track.id)),
        trail: track
            .trail
            .iter()
            .map(|point| [f64::from(point.range_km), f64::from(point.doppler_hz)])
            .collect(),
        faint: track.state == TrackState::Coasting,
        ..Mark::default()
    }
}

fn marks(update: &RadarUpdate) -> Vec<Mark> {
    let detections = update.detections.iter().map(|detection| Mark {
        x: f64::from(detection.range_km),
        y: f64::from(detection.doppler_hz),
        ..Mark::default()
    });
    let truth = update
        .truth
        .iter()
        .filter(|truth| truth.in_view)
        .map(|truth| Mark {
            x: f64::from(truth.range_km),
            y: f64::from(truth.doppler_hz),
            shape: MarkShape::Triangle,
            title: Some(truth.callsign.clone().unwrap_or_else(|| truth.id.clone())),
            ..Mark::default()
        });
    detections
        .chain(update.tracks.iter().map(track_mark))
        .chain(truth)
        .collect()
}

fn headline(update: &RadarUpdate) -> Option<Headline> {
    if let Some(problem) = update
        .problems
        .iter()
        .find(|held| chip_only(held).is_none())
    {
        return Some(Headline {
            text: problem.label().to_owned(),
            warn: true,
        });
    }
    (update.axes.carrier_hz > 0.0).then(|| Headline {
        text: format!("{:.2} MHz", update.axes.carrier_hz / 1e6),
        warn: false,
    })
}

fn tracks_by_strength(update: &RadarUpdate) -> Vec<&RadarTrack> {
    let mut tracks: Vec<&RadarTrack> = update.tracks.iter().collect();
    tracks.sort_by(|a, b| b.snr_db.total_cmp(&a.snr_db).then(a.id.cmp(&b.id)));
    tracks.truncate(TRACK_ROWS);
    tracks
}

impl Report for RadarUpdate {
    fn status(&self) -> ChannelStatus {
        let health = &self.health;
        let targets = self
            .tracks
            .iter()
            .filter(|track| track.state == TrackState::Confirmed)
            .count();
        let lost = health.dropped_cpis + health.dropped_reports + health.lagged_updates;
        let readouts = fields(&[
            ("targets", "Targets", count(Some(targets as f64))),
            ("clutter", "Clutter", db(mean(&health.suppression_db), 0)),
            ("load", "Load", percent(health.load)),
            ("reference", "Ref", text_of(reference_text(self))),
            (
                "cpi",
                "CPI",
                number(Some(self.axes.cpi_ms / 1_000.0), Unit::Seconds, 2),
            ),
            (
                "drops",
                "Drops",
                count(Some(lost as f64).filter(|lost| *lost > 0.0)),
            ),
        ]);
        ChannelStatus {
            health: ChannelHealth::default().lock(health.reference.locked),
            readouts,
            choices: Vec::new(),
        }
    }

    fn view(&self) -> ProcessorView {
        ProcessorView {
            headline: headline(self),
            flags: self
                .problems
                .iter()
                .filter_map(|problem| {
                    chip_only(problem).map(|title| Flag::warn(problem.label(), title))
                })
                .collect(),
            surface: Some(SurfaceView {
                marks: marks(self),
                ..SurfaceView::default()
            }),
            table: Some(TableView {
                heads: TRACK_HEADS.map(str::to_owned).to_vec(),
                rows: tracks_by_strength(self)
                    .into_iter()
                    .map(track_row)
                    .collect(),
            }),
            ..ProcessorView::default()
        }
    }
}
