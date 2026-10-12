use sdrmm_wire::{
    channel::ChannelDescriptor,
    device::{Capabilities, DeviceSettings, StreamSettings, Tuning},
    patch::PatchGraph,
    state::DeviceSet,
};

use crate::{
    shell::library_target::{IqLane, LaneKind, iq_lanes, lane_of},
    ui::faces::channel::{
        lane_center_hz, lane_rate_hz,
        settings::{radio_window_hz, reaches_hz},
    },
};

#[derive(Clone, Debug, PartialEq)]
pub enum Reach {
    Stay,
    Radio(u32, Box<DeviceSettings>),
    Array(String),
}

#[must_use]
pub fn reachable_hz(capabilities: &Capabilities, hz: f64) -> f64 {
    let ranges = &capabilities.freq_ranges;
    if ranges.is_empty() {
        return hz.max(0.0);
    }
    ranges
        .iter()
        .map(|range| hz.clamp(range.min, range.max.max(range.min)))
        .min_by(|a, b| (a - hz).abs().total_cmp(&(b - hz).abs()))
        .unwrap_or(hz)
}

#[must_use]
pub fn stream_settings(set: &DeviceSet, stream: u32) -> DeviceSettings {
    set.settings
        .for_stream(stream, &set.capabilities.per_stream)
}

#[must_use]
pub fn auto_tuning(set: &DeviceSet, stream: u32) -> bool {
    stream_settings(set, stream).tuning.unwrap_or_default() == Tuning::Auto
}

#[must_use]
pub fn center_hz(set: &DeviceSet, stream: u32) -> Option<f64> {
    stream_settings(set, stream).center_hz
}

#[must_use]
pub fn tune_delta(capabilities: &Capabilities, stream: u32, hz: f64) -> DeviceSettings {
    let center_hz = Some(reachable_hz(capabilities, hz));
    if capabilities.per_stream.tuning {
        DeviceSettings {
            streams: vec![StreamSettings {
                stream,
                center_hz,
                tuning: Some(Tuning::Manual),
                ..StreamSettings::default()
            }],
            ..DeviceSettings::default()
        }
    } else {
        DeviceSettings {
            center_hz,
            tuning: Some(Tuning::Manual),
            ..DeviceSettings::default()
        }
    }
}

fn hears(set: &DeviceSet, stream: u32, descriptor: Option<&ChannelDescriptor>, hz: f64) -> bool {
    let window = radio_window_hz(
        lane_center_hz(set, stream),
        lane_rate_hz(set, stream),
        descriptor,
    );
    reaches_hz(hz, window)
}

#[must_use]
pub fn radio_pull_for(
    set: &DeviceSet,
    stream: u32,
    descriptor: Option<&ChannelDescriptor>,
    hz: f64,
    lane_count: usize,
) -> Option<DeviceSettings> {
    if lane_count > 1 || auto_tuning(set, stream) || hears(set, stream, descriptor, hz) {
        return None;
    }
    Some(tune_delta(&set.capabilities, stream, hz))
}

#[must_use]
pub fn array_pull_for(
    set: &DeviceSet,
    lane: &IqLane,
    descriptor: Option<&ChannelDescriptor>,
    hz: f64,
) -> Option<String> {
    let LaneKind::Virtual { array: Some(array) } = &lane.kind else {
        return None;
    };
    (!hears(set, lane.stream, descriptor, hz)).then(|| array.clone())
}

#[must_use]
pub fn reach_for(
    graph: &PatchGraph,
    node: &str,
    set: &DeviceSet,
    devices: &[(String, DeviceSet)],
    descriptor: Option<&ChannelDescriptor>,
    hz: f64,
) -> Reach {
    let lane = lane_of(graph, node, devices);
    if let Some(lane) = lane.as_ref().filter(|lane| lane.kind != LaneKind::Radio) {
        return array_pull_for(set, lane, descriptor, hz).map_or(Reach::Stay, Reach::Array);
    }
    let stream = lane.map_or(0, |lane| lane.stream);
    let lanes = iq_lanes(graph, node, devices).len();
    radio_pull_for(set, stream, descriptor, hz, lanes)
        .map_or(Reach::Stay, |delta| Reach::Radio(set.id, Box::new(delta)))
}

#[must_use]
pub fn same_mode(mode: Option<&str>, channel_type: Option<&str>) -> bool {
    match mode.filter(|mode| !mode.is_empty()) {
        None => true,
        Some(mode) => channel_type.is_some_and(|kind| kind.eq_ignore_ascii_case(mode)),
    }
}

#[must_use]
pub fn mode_hint(
    mode: Option<&str>,
    channel_type: Option<&str>,
    what: &str,
    ask_without_channel: bool,
) -> Option<String> {
    if same_mode(mode, channel_type) {
        return None;
    }
    let mode = mode?.to_uppercase();
    match channel_type {
        Some(kind) => Some(format!(
            "{mode} is the mode for this {what}, not {}",
            kind.to_uppercase()
        )),
        None => ask_without_channel
            .then(|| format!("{mode} is the mode for this {what}: set it on a channel")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(settings: serde_json::Value, ranges: serde_json::Value) -> DeviceSet {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "device": { "driver": "rtlsdr", "key": "0", "label": "RTL-SDR" },
            "capabilities": {
                "freq_ranges": ranges, "sample_rates": [], "gains": [],
                "antennas": [], "bandwidths": [], "duplex": "rx_only"
            },
            "settings": settings,
            "status": "running",
            "channels": [],
        }))
        .expect("a device set")
    }

    #[test]
    fn a_tune_holds_the_radio_to_its_nearest_range_and_goes_manual() {
        let radio = set(
            serde_json::json!({}),
            serde_json::json!([{ "min": 24e6, "max": 1766e6 }]),
        );
        let delta = tune_delta(&radio.capabilities, 0, 10e6);
        assert_eq!(delta.center_hz, Some(24e6));
        assert_eq!(delta.tuning, Some(Tuning::Manual));
        assert!(auto_tuning(&radio, 0));
    }

    fn manual(extra: serde_json::Value) -> DeviceSet {
        let mut settings = serde_json::json!({
            "center_hz": 145e6, "sample_rate": 2e6, "tuning": "manual"
        });
        if let (Some(into), Some(from)) = (settings.as_object_mut(), extra.as_object()) {
            into.extend(from.clone());
        }
        set(settings, serde_json::json!([]))
    }

    fn nfm() -> ChannelDescriptor {
        ChannelDescriptor {
            type_id: "nfm".to_owned(),
            bandwidth_hz: 12_500.0,
            ..ChannelDescriptor::default()
        }
    }

    #[test]
    fn leaves_several_wired_radios_to_server_placement() {
        assert!(
            radio_pull_for(&manual(serde_json::json!({})), 0, Some(&nfm()), 433.5e6, 2).is_none()
        );
    }

    #[test]
    fn leaves_the_radio_where_it_is_when_it_already_hears_the_frequency() {
        assert!(
            radio_pull_for(&manual(serde_json::json!({})), 0, Some(&nfm()), 145.5e6, 1).is_none()
        );
    }

    #[test]
    fn pulls_the_radio_over_a_frequency_outside_its_window() {
        let delta = radio_pull_for(&manual(serde_json::json!({})), 0, Some(&nfm()), 433.5e6, 1);
        assert_eq!(
            delta.as_ref().and_then(|delta| delta.center_hz),
            Some(433.5e6)
        );
        assert_eq!(delta.and_then(|delta| delta.tuning), Some(Tuning::Manual));
    }

    #[test]
    fn leaves_a_radio_alone_that_has_no_window_yet() {
        let unclocked = set(
            serde_json::json!({ "center_hz": 145e6, "tuning": "manual" }),
            serde_json::json!([]),
        );
        assert!(radio_pull_for(&unclocked, 0, Some(&nfm()), 433.5e6, 1).is_none());
    }

    #[test]
    fn leaves_an_auto_radio_to_the_engine() {
        let auto = manual(serde_json::json!({ "tuning": "auto" }));
        assert!(radio_pull_for(&auto, 0, Some(&nfm()), 433.5e6, 1).is_none());
    }

    #[test]
    fn retunes_the_array_behind_a_beam_that_cannot_hear_the_frequency() {
        let mut beamed = manual(serde_json::json!({}));
        beamed.virtual_lanes = vec![sdrmm_wire::array::VirtualLane {
            stream: 5,
            node: "beam".to_owned(),
            port: "beam".to_owned(),
            center_hz: 100e6,
            sample_rate: 2e6,
        }];
        let lane = |array: Option<&str>| IqLane {
            source: "dev".to_owned(),
            stream: 5,
            kind: LaneKind::Virtual {
                array: array.map(str::to_owned),
            },
        };
        assert_eq!(
            array_pull_for(&beamed, &lane(Some("array")), Some(&nfm()), 433e6).as_deref(),
            Some("array")
        );
        assert!(array_pull_for(&beamed, &lane(Some("array")), Some(&nfm()), 100.2e6).is_none());
        assert!(array_pull_for(&beamed, &lane(None), Some(&nfm()), 433e6).is_none());
        let radio = IqLane {
            kind: LaneKind::Radio,
            ..lane(Some("array"))
        };
        assert!(array_pull_for(&beamed, &radio, Some(&nfm()), 433e6).is_none());
    }

    #[test]
    fn a_bookmark_without_a_mode_suits_any_channel() {
        assert!(same_mode(None, Some("nfm")));
        assert!(same_mode(Some(""), None));
        assert!(same_mode(Some("NFM"), Some("nfm")));
        assert!(!same_mode(Some("am"), Some("nfm")));
        assert!(!same_mode(Some("am"), None));
    }

    #[test]
    fn names_the_mode_a_channel_should_run() {
        assert_eq!(
            mode_hint(Some("am"), Some("nfm"), "bookmark", false).as_deref(),
            Some("AM is the mode for this bookmark, not NFM")
        );
        assert_eq!(mode_hint(Some("am"), None, "bookmark", false), None);
        assert_eq!(
            mode_hint(Some("am"), None, "band", true).as_deref(),
            Some("AM is the mode for this band: set it on a channel")
        );
        assert_eq!(mode_hint(Some("nfm"), Some("NFM"), "band", true), None);
        assert_eq!(mode_hint(None, Some("nfm"), "band", true), None);
    }

    fn wired(sources: &[&str]) -> PatchGraph {
        use sdrmm_wire::patch::{
            ChannelNode, DeviceNode, NodeBody, PatchEdge, PatchNode, PortRef, Position,
        };
        let node = |id: &str, body: NodeBody| PatchNode {
            id: id.to_owned(),
            body,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        };
        let mut nodes: Vec<PatchNode> = sources
            .iter()
            .map(|id| node(id, NodeBody::Device(DeviceNode::default())))
            .collect();
        nodes.push(node(
            "ch",
            NodeBody::Channel(ChannelNode {
                channel_type: "nfm".to_owned(),
                tuning_locked: false,
            }),
        ));
        let edges = sources
            .iter()
            .map(|id| PatchEdge {
                from: PortRef {
                    node: (*id).to_owned(),
                    port: "iq".to_owned(),
                },
                to: PortRef {
                    node: "ch".to_owned(),
                    port: "iq".to_owned(),
                },
            })
            .collect();
        PatchGraph { nodes, edges }
    }

    #[test]
    fn a_channel_on_one_radio_pulls_it_and_on_several_leaves_them() {
        let radio = manual(serde_json::json!({}));
        let devices = vec![("a".to_owned(), radio.clone())];
        let reach = reach_for(
            &wired(&["a"]),
            "ch",
            &radio,
            &devices,
            Some(&nfm()),
            433.5e6,
        );
        assert!(matches!(reach, Reach::Radio(1, ref delta) if delta.center_hz == Some(433.5e6)));
        assert_eq!(
            reach_for(
                &wired(&["a"]),
                "ch",
                &radio,
                &devices,
                Some(&nfm()),
                145.1e6
            ),
            Reach::Stay
        );
        let both = vec![
            ("a".to_owned(), radio.clone()),
            ("b".to_owned(), radio.clone()),
        ];
        assert_eq!(
            reach_for(
                &wired(&["a", "b"]),
                "ch",
                &radio,
                &both,
                Some(&nfm()),
                433.5e6
            ),
            Reach::Stay
        );
    }
}
