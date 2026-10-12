use std::collections::HashMap;

use sdrmm_wire::{
    array::{ArrayLaneStatus, ArrayStatus},
    patch::{NodeBody, PatchGraph},
    state::DeviceSet,
};

use super::labels::delay_label;
use crate::ui::faces::{
    channel::lane_center_hz,
    device::lanes::{ref_label, rx_stream_count, stream_label},
};

pub const RADIO: &str = "Radio";

pub type Sets = HashMap<String, DeviceSet>;

#[derive(Clone, Debug, PartialEq)]
pub struct LaneRow {
    pub lane: u32,
    pub source_label: String,
    pub status: Option<ArrayLaneStatus>,
}

fn source_name(graph: &PatchGraph, sets: &Sets, node: &str) -> String {
    if let Some(set) = sets.get(node) {
        return set.device.label.clone();
    }
    let found = graph.node(node);
    let reference = found.and_then(|found| match &found.body {
        NodeBody::Device(device) => device.device.as_ref().map(ref_label),
        _ => None,
    });
    reference
        .or_else(|| found.and_then(|found| found.label.clone()))
        .unwrap_or_else(|| RADIO.to_owned())
}

fn wired(graph: &PatchGraph, array: &str) -> Vec<(u32, String, u32)> {
    graph
        .array_lanes(array)
        .into_iter()
        .enumerate()
        .filter_map(|(lane, wire)| {
            let (node, stream) = wire?;
            Some((u32::try_from(lane).ok()?, node.to_owned(), stream))
        })
        .collect()
}

#[must_use]
pub fn lane_rows(
    graph: &PatchGraph,
    sets: &Sets,
    array: &str,
    status: Option<&ArrayStatus>,
) -> Vec<LaneRow> {
    wired(graph, array)
        .into_iter()
        .map(|(lane, node, stream)| {
            let streams = sets
                .get(&node)
                .map_or(1, |set| rx_stream_count(&set.capabilities));
            let name = source_name(graph, sets, &node);
            LaneRow {
                lane: lane + 1,
                source_label: format!("{name} {}", stream_label("iq", stream, streams)),
                status: status
                    .and_then(|status| status.lanes.iter().find(|found| found.lane == lane))
                    .cloned(),
            }
        })
        .collect()
}

#[must_use]
pub fn member_devices(graph: &PatchGraph, array: &str) -> Vec<String> {
    let mut members: Vec<String> = Vec::new();
    for (_, node, _) in wired(graph, array) {
        if !members.contains(&node) {
            members.push(node);
        }
    }
    members
}

#[must_use]
pub fn lane_title(row: &LaneRow) -> String {
    let Some(lane) = &row.status else {
        return format!("Lane {} · {}", row.lane, row.source_label);
    };
    [
        format!("Lane {}", row.lane),
        row.source_label.clone(),
        format!("phase {:.2}°", lane.phase_deg),
        format!("gain {:.2} dB", lane.gain_db),
        format!("delay {} smp", delay_label(lane.delay_samples)),
        format!("gaps {}", lane.gaps),
    ]
    .join(" · ")
}

#[must_use]
pub fn span_hz(
    graph: &PatchGraph,
    sets: &Sets,
    array: &str,
    status: Option<&ArrayStatus>,
) -> Option<f64> {
    let status = status?;
    let centres: Vec<f64> = wired(graph, array)
        .into_iter()
        .filter_map(|(_, node, stream)| lane_center_hz(sets.get(&node)?, stream))
        .collect();
    let low = centres.iter().copied().reduce(f64::min)?;
    let high = centres.iter().copied().reduce(f64::max)?;
    Some(high - low + status.sample_rate)
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::device::{Coherence, DeviceSettings, StreamSettings};

    use super::*;
    use crate::ui::faces::array::fixtures::{device, lane, placed, radio};

    fn patch(edges: Vec<sdrmm_wire::patch::PatchEdge>) -> PatchGraph {
        PatchGraph {
            nodes: vec![
                placed("kraken", device("virtual", "kraken5"), None),
                placed("rtl", device("rtlsdr", "7"), None),
                placed(
                    "north",
                    NodeBody::default_for("array").expect("array"),
                    Some("North"),
                ),
            ],
            edges,
        }
    }

    fn kraken() -> DeviceSet {
        radio(5, Coherence::TimeSync)
    }

    #[test]
    fn lanes_are_numbered_by_port_not_by_list_position() {
        let graph = patch(vec![
            lane("kraken", 0, "north", 0),
            lane("kraken", 2, "north", 2),
        ]);
        let rows = lane_rows(&graph, &Sets::new(), "north", None);
        let lanes: Vec<u32> = rows.iter().map(|row| row.lane).collect();
        assert_eq!(lanes, vec![1, 3]);
    }

    #[test]
    fn each_lane_is_named_by_radio_and_stream() {
        let graph = patch(vec![
            lane("kraken", 0, "north", 0),
            lane("kraken", 2, "north", 2),
            lane("rtl", 0, "north", 3),
        ]);
        let status = ArrayStatus {
            lanes: [0, 2, 3]
                .map(|lane| ArrayLaneStatus {
                    lane,
                    phase_deg: if lane == 2 { 12.34 } else { 0.0 },
                    ..ArrayLaneStatus::default()
                })
                .to_vec(),
            ..ArrayStatus::default()
        };
        let sets = Sets::from([("kraken".to_owned(), kraken())]);
        let rows = lane_rows(&graph, &sets, "north", Some(&status));
        let labels: Vec<&str> = rows.iter().map(|row| row.source_label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["KrakenSDR iq1", "KrakenSDR iq3", "rtlsdr · 7 iq"]
        );
        assert_eq!(
            lane_title(&rows[1]),
            "Lane 3 · KrakenSDR iq3 · phase 12.34° · gain 0.00 dB · delay 0.00 smp · gaps 0"
        );
        let unmeasured = &lane_rows(&graph, &Sets::new(), "north", None)[0];
        assert_eq!(unmeasured.status, None);
        assert_eq!(lane_title(unmeasured), "Lane 1 · virtual · kraken5 iq");
        assert_eq!(member_devices(&graph, "north"), vec!["kraken", "rtl"]);
    }

    #[test]
    fn the_span_runs_from_the_lowest_to_the_highest_lane_plus_one_rate() {
        let mut spread = radio(3, Coherence::TimeSync);
        spread.settings = DeviceSettings {
            center_hz: Some(100e6),
            streams: [(1, 102e6), (2, 104e6)]
                .map(|(stream, hz)| StreamSettings {
                    stream,
                    center_hz: Some(hz),
                    ..StreamSettings::default()
                })
                .to_vec(),
            ..DeviceSettings::default()
        };
        let graph = patch(vec![
            lane("kraken", 0, "north", 0),
            lane("kraken", 1, "north", 1),
            lane("kraken", 2, "north", 2),
        ]);
        let status = ArrayStatus {
            sample_rate: 2.4e6,
            ..ArrayStatus::default()
        };
        let sets = Sets::from([("kraken".to_owned(), spread)]);
        assert_eq!(span_hz(&graph, &sets, "north", Some(&status)), Some(6.4e6));
        assert_eq!(span_hz(&graph, &Sets::new(), "north", Some(&status)), None);
        assert_eq!(span_hz(&graph, &sets, "north", None), None);
    }
}
