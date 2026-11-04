use std::sync::LazyLock;

use sdrmm_wire::{
    AUDIO_PORT, ChannelNode, ChannelSettings, DeviceNode, EVENTS_PORT, IQ_PORT, NodeBody,
    PatchEdge, PatchGraph, PatchNode, PortRef, Position, TemplateInfo, VIDEO_PORT,
};
use serde::Deserialize;

/// The templates, as data: each names decoders by type id, and one whose decoders are not all
/// registered is left out.
const TEMPLATES: &str = include_str!("../data/templates.json");

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Sink {
    Readout,
    Speaker,
    Video,
    Map,
    Log,
}

impl Sink {
    const fn node(self) -> &'static str {
        match self {
            Self::Readout => "readout",
            Self::Speaker => "speaker",
            Self::Video => "video",
            Self::Map => "map",
            Self::Log => "log",
        }
    }

    const fn body(self) -> NodeBody {
        match self {
            Self::Readout => NodeBody::Readout,
            Self::Speaker => NodeBody::Speaker,
            Self::Video => NodeBody::Video,
            Self::Map => NodeBody::Map,
            Self::Log => NodeBody::DecoderLog(sdrmm_wire::DecoderLogNode {
                group: None,
                limit: None,
            }),
        }
    }

    const fn port(self) -> &'static str {
        match self {
            Self::Speaker => AUDIO_PORT,
            Self::Video => VIDEO_PORT,
            Self::Readout | Self::Map | Self::Log => EVENTS_PORT,
        }
    }
}

const ORDER: &[Sink] = &[
    Sink::Readout,
    Sink::Speaker,
    Sink::Video,
    Sink::Map,
    Sink::Log,
];

#[derive(Deserialize)]
struct Channel {
    #[serde(flatten)]
    settings: ChannelSettings,
    sinks: Vec<Sink>,
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    name: String,
    description: String,
    explainer: String,
    center_hz: f64,
    sample_rate: f64,
    channels: Vec<Channel>,
}

const COLUMN: f32 = 520.0;
const ROW: f32 = 240.0;
const SINK_ROW: f32 = 420.0;

fn patch(channels: &[Channel]) -> PatchGraph {
    let node = |id: &str, body: NodeBody, x: f32, y: f32| PatchNode {
        id: id.to_string(),
        body,
        position: Position { x, y },
        size: None,
        label: None,
    };
    let wire = |from: (&str, &str), to: (&str, &str)| PatchEdge {
        from: PortRef {
            node: from.0.to_string(),
            port: from.1.to_string(),
        },
        to: PortRef {
            node: to.0.to_string(),
            port: to.1.to_string(),
        },
    };

    let mut nodes = vec![
        node("dev", NodeBody::Device(DeviceNode::default()), 0.0, 0.0),
        node("scope", NodeBody::Scope, COLUMN, -40.0),
    ];
    let mut edges = vec![wire(("dev", IQ_PORT), ("scope", IQ_PORT))];

    for (index, channel) in channels.iter().enumerate() {
        let id = format!("ch{index}");
        #[expect(
            clippy::cast_precision_loss,
            reason = "a template has a handful of channels; the row index is exact in f32"
        )]
        let y = 260.0 + index as f32 * ROW;
        nodes.push(node(
            &id,
            NodeBody::Channel(ChannelNode {
                channel_type: channel.settings.params.type_id().to_owned(),
                tuning_locked: false,
            }),
            COLUMN,
            y,
        ));
        edges.push(wire(("dev", IQ_PORT), (&id, IQ_PORT)));
        for sink in &channel.sinks {
            edges.push(wire((&id, sink.port()), (sink.node(), sink.port())));
        }
    }

    let used = ORDER
        .iter()
        .filter(|sink| channels.iter().any(|c| c.sinks.contains(sink)));
    for (index, sink) in used.enumerate() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a template draws at most five sinks; the row index is exact in f32"
        )]
        let y = -40.0 + index as f32 * SINK_ROW;
        nodes.push(node(sink.node(), sink.body(), COLUMN * 2.0, y));
    }
    PatchGraph { nodes, edges }
}

fn entries() -> Vec<Entry> {
    serde_json::from_str(TEMPLATES).unwrap_or_else(|error| {
        tracing::error!(%error, "templates did not parse");
        Vec::new()
    })
}

fn runnable(entry: &Entry) -> bool {
    entry
        .channels
        .iter()
        .all(|channel| sdrmm_channels::descriptor(channel.settings.params.type_id()).is_some())
}

fn info(entry: Entry) -> TemplateInfo {
    let freqs = entry.channels.iter().map(|c| c.settings.frequency_hz);
    let min = freqs.clone().fold(f64::INFINITY, f64::min);
    let max = freqs.fold(f64::NEG_INFINITY, f64::max);
    TemplateInfo {
        patch: Some(patch(&entry.channels)),
        channels: entry.channels.into_iter().map(|c| c.settings).collect(),
        min_freq_hz: min.min(entry.center_hz),
        max_freq_hz: max.max(entry.center_hz),
        id: entry.id,
        name: entry.name,
        description: entry.description,
        explainer: entry.explainer,
        center_hz: entry.center_hz,
        sample_rate: entry.sample_rate,
        direction: sdrmm_wire::Direction::Rx,
        supported_devices: Vec::new(),
    }
}

pub(crate) fn all() -> &'static [TemplateInfo] {
    static BUILT: LazyLock<Vec<TemplateInfo>> =
        LazyLock::new(|| entries().into_iter().filter(runnable).map(info).collect());
    &BUILT
}

#[must_use]
pub(crate) fn get(id: &str) -> Option<&'static TemplateInfo> {
    all().iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{MAX_SQUELCH_AUTO_MARGIN_DB, MIN_SQUELCH_AUTO_MARGIN_DB, WorkspaceSnapshot};

    use super::*;

    #[test]
    fn every_template_runs_on_one_radio() {
        for template in all() {
            let patch = template.patch.as_ref().expect("a template draws a patch");
            let devices = patch
                .nodes
                .iter()
                .filter(|node| matches!(node.body, NodeBody::Device(_)))
                .count();
            assert_eq!(devices, 1, "{} opens {devices} radios", template.id);
            assert!(
                !patch
                    .nodes
                    .iter()
                    .any(|node| matches!(node.body, NodeBody::Array(_))),
                "{} builds an array; that is an example, not a template",
                template.id
            );
        }
    }

    #[test]
    fn ids_are_unique_and_slug_shaped() {
        let mut seen = std::collections::HashSet::new();
        for template in all() {
            assert!(seen.insert(&template.id), "duplicate id {}", template.id);
            assert!(
                template
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "unslugly id {}",
                template.id
            );
            assert!(!template.name.is_empty());
            assert!(!template.explainer.is_empty());
        }
    }

    #[test]
    fn every_channel_fits_the_templates_window() {
        for template in all() {
            let usable = sdrmm_dsp::flat_bandwidth_hz(template.sample_rate) / 2.0;
            for channel in &template.channels {
                let offset_hz = channel.frequency_hz - template.center_hz;
                assert!(
                    offset_hz.abs() <= usable,
                    "{}: channel at {offset_hz} Hz is outside the flat ±{usable} Hz",
                    template.id
                );
            }
            assert!(template.min_freq_hz <= template.max_freq_hz);
        }
    }

    #[test]
    fn every_template_names_a_known_channel_type() {
        let descriptors = sdrmm_engine::channel_types();
        for template in all() {
            for channel in &template.channels {
                let type_id = channel.params.type_id();
                assert!(
                    descriptors.iter().any(|d| d.type_id == type_id),
                    "{}: unknown channel {type_id}",
                    template.id
                );
            }
        }
    }

    #[test]
    fn automatic_squelch_margins_are_inside_the_accepted_range() {
        for template in all() {
            for channel in &template.channels {
                let Some(margin) = channel.squelch.auto_margin_db() else {
                    continue;
                };
                assert!(
                    (MIN_SQUELCH_AUTO_MARGIN_DB..=MAX_SQUELCH_AUTO_MARGIN_DB).contains(&margin),
                    "{}: squelch margin {margin} dB",
                    template.id
                );
            }
        }
    }

    #[test]
    fn every_channel_gets_at_least_its_decoder_rate() {
        for template in all() {
            for channel in &template.channels {
                let rate = sdrmm_channels::input_rate(&channel.params);
                assert!(
                    template.sample_rate >= rate,
                    "{}: {} needs {rate} Hz",
                    template.id,
                    channel.params.type_id()
                );
            }
        }
    }

    #[test]
    fn a_decoder_too_wide_to_decimate_runs_the_device_at_its_rate() {
        let mut wide = 0;
        for template in all() {
            for channel in &template.channels {
                let rate = sdrmm_channels::input_rate(&channel.params);
                if rate * 2.0 > template.sample_rate {
                    wide += 1;
                    assert_eq!(template.sample_rate, rate, "{}", template.id);
                    assert_eq!(channel.frequency_hz, template.center_hz, "{}", template.id);
                }
            }
        }
        assert!(wide > 0, "no template runs a wide decoder");
    }

    #[test]
    fn a_radio_that_only_runs_wide_gets_a_rate_for_every_template() {
        let mut transceiver = sdrmm_wire::DeviceProfile {
            freq_ranges: vec![sdrmm_wire::Range {
                min: 70_000_000.0,
                max: 6_000_000_000.0,
                step: None,
            }],
            ..sdrmm_wire::DeviceProfile::default()
        };
        transceiver.sample_rate_ranges = vec![sdrmm_wire::Range {
            min: 2_100_000.0,
            max: 61_440_000.0,
            step: None,
        }];

        for template in all() {
            if template.min_freq_hz < 70_000_000.0 {
                continue;
            }
            let rate = template
                .rate_on(&transceiver)
                .unwrap_or_else(|| panic!("{}: no rate", template.id));
            assert!(
                rate >= 2_100_000.0,
                "{}: {rate} Hz is below the floor",
                template.id
            );
            assert_eq!(template.unmet_by(&transceiver), None, "{}", template.id);
        }
    }

    #[test]
    fn a_narrow_radio_runs_a_template_at_the_widest_rate_it_has() {
        for template in all() {
            let narrow = sdrmm_wire::DeviceProfile {
                sample_rates: vec![template.sample_rate / 4.0, template.sample_rate / 2.0],
                ..sdrmm_wire::DeviceProfile::default()
            };
            assert_eq!(template.unmet_by(&narrow), None, "{}", template.id);
            assert_eq!(
                template.rate_on(&narrow),
                Some(template.sample_rate / 2.0),
                "{}",
                template.id
            );
        }
    }

    #[test]
    fn every_template_patch_is_a_valid_workspace() {
        let descriptors = sdrmm_engine::Engine::new(None).channel_types();
        for template in all() {
            let patch = template.patch.clone().expect("templates carry a patch");
            patch
                .validate_against(&descriptors)
                .unwrap_or_else(|e| panic!("{}: {e}", template.id));

            let mut workspace = WorkspaceSnapshot::starter();
            workspace.merge_patch(&patch, &format!("template:{}:", template.id), None);
            workspace
                .validate()
                .unwrap_or_else(|e| panic!("{} merged: {e}", template.id));
        }
    }

    #[test]
    fn every_template_and_the_starter_resolve_without_a_refused_wire() {
        let descriptors = sdrmm_engine::Engine::new(None).channel_types();
        let ctx = sdrmm_wire::patch::resolve::ResolveCtx {
            descriptors: Some(&descriptors),
            live: None,
        };
        let graphs = all()
            .iter()
            .map(|template| {
                (
                    template.id.clone(),
                    template.patch.clone().expect("templates carry a patch"),
                )
            })
            .chain([("starter".to_owned(), WorkspaceSnapshot::starter().graph)]);
        for (id, graph) in graphs {
            let (_, refused) = sdrmm_wire::patch::resolve::compile_draft(&graph, &ctx);
            assert!(refused.is_empty(), "{id}: {refused:?}");
        }
    }

    #[test]
    fn a_channel_reaches_every_sink_it_asks_for_and_no_other() {
        for entry in entries() {
            let patch = patch(&entry.channels);
            for sink in ORDER {
                let wanted = entry.channels.iter().any(|c| c.sinks.contains(sink));
                let drawn = patch.nodes.iter().any(|node| node.id == sink.node());
                assert_eq!(drawn, wanted, "{}: {}", entry.id, sink.node());
            }
            for (index, channel) in entry.channels.iter().enumerate() {
                let id = format!("ch{index}");
                let mut wired: Vec<&str> = patch
                    .edges
                    .iter()
                    .filter(|edge| edge.from.node == id)
                    .map(|edge| edge.to.node.as_str())
                    .collect();
                wired.dedup();
                let wanted: Vec<&str> = channel.sinks.iter().map(|sink| sink.node()).collect();
                assert_eq!(wired, wanted, "{}: {id}", entry.id);
            }
        }
    }

    #[test]
    fn every_patch_draws_the_channels_the_template_creates() {
        for template in all() {
            let patch = template.patch.clone().expect("templates carry a patch");
            let resolved = sdrmm_wire::patch::resolve::compile(&patch, &Default::default())
                .expect("templates compile");
            let drawn: Vec<&str> = patch
                .nodes
                .iter()
                .filter_map(|node| match &node.body {
                    NodeBody::Channel(channel) => {
                        let lanes: Vec<&str> = resolved
                            .feeds(&node.id, "iq")
                            .iter()
                            .filter(|feed| feed.source == "dev")
                            .map(|feed| feed.port.as_str())
                            .collect();
                        (!lanes.is_empty()).then(|| {
                            assert_eq!(lanes, ["iq"], "{} wires a non-zero stream", template.id);
                            channel.channel_type.as_str()
                        })
                    }
                    _ => None,
                })
                .collect();
            let created: Vec<&str> = template
                .channels
                .iter()
                .map(|c| c.params.type_id())
                .collect();
            assert_eq!(drawn, created, "{}", template.id);
        }
    }

    #[test]
    fn unknown_ids_are_none() {
        assert!(get("nope").is_none());
        assert!(get("").is_none());
    }
}
