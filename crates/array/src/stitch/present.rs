use sdrmm_wire::channel::ChannelStatus;
use sdrmm_wire::present::{Unit, count, fields, number};
use sdrmm_wire::processor::view::{Cell, ProcessorView, TableRow, TableView};

use super::{StitchLane, StitchReading};
use crate::Report;
use crate::present::{drops, flags, signed};

const LANE_HEADS: [&str; 5] = ["Lane", "MHz", "Eq", "Phase", "Coh"];

fn lane_row(lane: &StitchLane) -> TableRow {
    let name = format!("L{}", lane.lane + 1);
    let eq = format!("{} dB", signed(lane.noise_eq_db, 1));
    let phase = format!("{:.0}°", lane.phase_deg);
    let coherence = lane
        .coherence
        .map_or_else(|| "-".to_owned(), |value| format!("{value:.2}"));
    let mut title = format!("{name} {eq} {phase} {coherence}");
    if lane.spur_bins > 0 {
        title.push_str(&format!(" {} spur bins", lane.spur_bins));
    }
    TableRow {
        key: lane.lane,
        cells: vec![
            Cell::of(name),
            Cell::of(format!("{:.3}", lane.center_hz / 1e6)),
            Cell::of(eq),
            Cell::of(phase),
            Cell::of(coherence),
        ],
        title: Some(title),
        ..TableRow::default()
    }
}

impl Report for StitchReading {
    fn status(&self) -> ChannelStatus {
        ChannelStatus::readouts(fields(&[
            (
                "center",
                "Center",
                number(Some(self.center_hz), Unit::Hz, 0),
            ),
            ("span", "Span", number(Some(self.span_hz), Unit::Hz, 0)),
            ("lanes", "Lanes", count(Some(self.lanes.len() as f64))),
        ]))
    }

    fn view(&self) -> ProcessorView {
        let mut found = flags(&[(
            self.no_overlap,
            "No overlap",
            "Lanes leave gaps between them",
        )]);
        found.extend(drops(self.dropped_blocks, "Blocks lost"));
        ProcessorView {
            flags: found,
            table: Some(TableView {
                heads: LANE_HEADS.map(str::to_owned).to_vec(),
                rows: self.lanes.iter().map(lane_row).collect(),
            }),
            ..ProcessorView::default()
        }
    }
}
