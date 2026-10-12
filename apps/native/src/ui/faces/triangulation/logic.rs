use std::collections::HashSet;

use jiff::Timestamp;
use sdrmm_wire::{
    fusion::{
        DfEstimate, DfFusionState, DfStation, FusionDecay, NavMode, NavTargetKind,
        TriangulationParams,
    },
    limits::FUSION_LIMITS,
    patch::{NodeBody, PatchGraph},
};

pub const NO_SOURCES: &str = "Wire direction finders in";
pub const NO_BEARINGS: &str = "No bearings yet";
pub const FADE_TITLE: &str = "How fast old bearings count less";

const BEARING_KINDS: [&str; 3] = ["df", "hunt", "event_filter"];

#[must_use]
pub fn half_life_title(seconds: u32) -> String {
    if seconds.is_multiple_of(60) {
        format!("Half life {} min", seconds / 60)
    } else {
        format!("Half life {seconds} s")
    }
}

#[must_use]
pub fn decay_options() -> [(FusionDecay, &'static str); 4] {
    [
        (FusionDecay::Auto, "Auto"),
        (FusionDecay::Fixed, "Fixed"),
        (FusionDecay::Moving, "Moving"),
        (
            FusionDecay::HalfLife {
                seconds: FUSION_LIMITS.half_life_seed_s,
            },
            "Set",
        ),
    ]
}

#[must_use]
pub fn decay_option_title(decay: FusionDecay) -> String {
    match decay {
        FusionDecay::Auto => "Fixed or moving, from the stations".to_owned(),
        FusionDecay::Fixed => half_life_title(FUSION_LIMITS.fixed_half_life_s),
        FusionDecay::Moving => half_life_title(FUSION_LIMITS.moving_half_life_s),
        FusionDecay::HalfLife { .. } => "Pick the half life".to_owned(),
    }
}

#[must_use]
pub fn decay_choice(decay: FusionDecay) -> FusionDecay {
    match decay {
        FusionDecay::HalfLife { .. } => FusionDecay::HalfLife {
            seconds: FUSION_LIMITS.half_life_seed_s,
        },
        other => other,
    }
}

#[must_use]
pub fn decay_with(picked: FusionDecay, current: FusionDecay) -> FusionDecay {
    match (picked, current) {
        (FusionDecay::HalfLife { .. }, FusionDecay::HalfLife { .. }) => current,
        _ => picked,
    }
}

#[must_use]
pub fn half_life_of(decay: FusionDecay) -> Option<u32> {
    match decay {
        FusionDecay::HalfLife { seconds } => Some(seconds),
        _ => None,
    }
}

#[must_use]
pub fn fade_title(fusion: Option<&DfFusionState>) -> String {
    fusion
        .map(|fusion| fusion.half_life_s)
        .filter(|seconds| *seconds > 0)
        .map_or_else(
            || FADE_TITLE.to_owned(),
            |seconds| format!("Old bearings count half after {seconds} s"),
        )
}

pub const NAV_OPTIONS: [(NavMode, &str); 3] = [
    (NavMode::Auto, "Auto"),
    (NavMode::Direct, "Direct"),
    (NavMode::Off, "Off"),
];

#[must_use]
pub fn nav_title(mode: NavMode) -> Option<&'static str> {
    match mode {
        NavMode::Auto => Some("Cross the bearings first, then drive at the fix"),
        NavMode::Direct => Some("Drive at the fix as soon as there is one"),
        NavMode::Off => None,
    }
}

#[must_use]
pub fn fusion_sources(graph: &PatchGraph, node: &str) -> usize {
    graph
        .edges
        .iter()
        .filter(|edge| edge.to.node == node && edge.to.port == "events")
        .filter_map(|edge| graph.node(&edge.from.node))
        .filter(|found| BEARING_KINDS.contains(&found.body.kind()))
        .map(|found| found.id.as_str())
        .collect::<HashSet<_>>()
        .len()
}

#[must_use]
pub fn subtitle(reporting: usize, sources: usize) -> String {
    format!("{reporting} of {sources} reporting")
}

#[must_use]
pub fn settings_of(body: &NodeBody) -> Option<&TriangulationParams> {
    match body {
        NodeBody::Triangulation(node) => Some(&node.settings),
        _ => None,
    }
}

#[must_use]
pub fn settings_mut(body: &mut NodeBody) -> Option<&mut TriangulationParams> {
    match body {
        NodeBody::Triangulation(node) => Some(&mut node.settings),
        _ => None,
    }
}

fn metres(value: f64) -> String {
    if value >= 1_000.0 {
        format!("{:.1} km", value / 1_000.0)
    } else {
        format!("{:.0} m", value.round())
    }
}

#[must_use]
pub fn estimate_label(estimate: Option<&DfEstimate>) -> String {
    estimate.map_or_else(
        || "-".to_owned(),
        |estimate| format!("{:.5}, {:.5}", estimate.lat, estimate.lon),
    )
}

#[must_use]
pub fn spread_label(estimate: Option<&DfEstimate>) -> String {
    estimate.map_or_else(
        || "-".to_owned(),
        |estimate| {
            format!(
                "{} \u{d7} {}",
                metres(estimate.ellipse_major_m),
                metres(estimate.ellipse_minor_m)
            )
        },
    )
}

#[must_use]
pub fn nav_text(kind: NavTargetKind) -> &'static str {
    match kind {
        NavTargetKind::Probe => "Drive across",
        NavTargetKind::Estimate => "Drive at it",
    }
}

#[must_use]
pub fn guidance(fusion: Option<&DfFusionState>) -> (String, Option<String>) {
    if let Some(nav) = fusion.and_then(|fusion| fusion.nav) {
        return (
            format!(
                "{} \u{b7} {:.0}\u{b0}",
                nav_text(nav.kind),
                nav.bearing_deg.round()
            ),
            Some(format!("{} away", metres(nav.distance_m))),
        );
    }
    let title = match fusion {
        Some(fusion) if fusion.no_guide_position => Some("Wire a GPS in to guide it"),
        Some(fusion) if fusion.no_bearings => Some("No bearings to steer by yet"),
        _ => None,
    };
    ("-".to_owned(), title.map(str::to_owned))
}

#[must_use]
pub fn readout_rows(fusion: Option<&DfFusionState>) -> Vec<(&'static str, String, Option<String>)> {
    let estimate = fusion.and_then(|fusion| fusion.estimate.as_ref());
    let (guide, guide_title) = guidance(fusion);
    vec![
        ("Estimate", estimate_label(estimate), None),
        (
            "Spread",
            spread_label(estimate),
            Some("One sigma error ellipse".to_owned()),
        ),
        ("Guidance", guide, guide_title),
        (
            "Bearings",
            fusion.map_or(0, |fusion| fusion.samples).to_string(),
            None,
        ),
    ]
}

#[must_use]
pub fn station_age(station: &DfStation, now: Timestamp) -> String {
    let Ok(seen) = station.last_seen.parse::<Timestamp>() else {
        return "just now".to_owned();
    };
    let seconds = ((now.as_millisecond() - seen.as_millisecond()).max(0) as f64 / 1_000.0).round();
    if seconds < 5.0 {
        "just now".to_owned()
    } else if seconds < 90.0 {
        format!("{seconds:.0}s ago")
    } else {
        format!("{:.0}m ago", (seconds / 60.0).round())
    }
}

#[must_use]
pub fn station_bearing(station: &DfStation) -> String {
    let deg = station.last_bearing_deg;
    if deg.is_finite() {
        format!("{deg:.1}\u{b0}")
    } else {
        "-".to_owned()
    }
}

#[must_use]
pub fn station_sigma(station: &DfStation) -> String {
    let sigma = station.sigma_deg;
    if sigma.is_finite() && sigma > 0.0 {
        format!("{sigma:.1}\u{b0}")
    } else {
        "-".to_owned()
    }
}

#[must_use]
pub fn station_align(station: &DfStation) -> String {
    match station.align_deg.filter(|deg| deg.is_finite()) {
        Some(deg) => format!("{}{deg:.1}\u{b0}", if deg > 0.0 { "+" } else { "" }),
        None => "-".to_owned(),
    }
}

#[must_use]
pub fn station_cells(station: &DfStation, now: Timestamp) -> [String; 5] {
    [
        station.station_id.clone(),
        station_bearing(station),
        station_sigma(station),
        station_align(station),
        station_age(station, now),
    ]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub label: &'static str,
    pub count: u64,
    pub title: &'static str,
}

#[must_use]
pub fn stats(fusion: Option<&DfFusionState>) -> Vec<Stat> {
    let Some(fusion) = fusion else {
        return Vec::new();
    };
    [
        Stat {
            label: "Dropped",
            count: fusion.dropped,
            title: "Bearings lost, the queue was full",
        },
        Stat {
            label: "Refused",
            count: fusion.refused,
            title: "Bearings without a place or below Min conf",
        },
    ]
    .into_iter()
    .filter(|stat| stat.count > 0)
    .collect()
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        fusion::NavTarget,
        patch::{PatchEdge, PatchNode, PortRef, Position, TriangulationNode},
    };

    use super::*;

    fn station(last_seen: &str) -> DfStation {
        DfStation {
            station_id: "north".into(),
            lat: 51.5,
            lon: 7.0,
            bearings: 3,
            last_seen: last_seen.into(),
            last_bearing_deg: 44.0,
            sigma_deg: 3.0,
            align_deg: Some(4.5),
            ..DfStation::default()
        }
    }

    fn estimate() -> DfEstimate {
        DfEstimate {
            lat: 51.5,
            lon: 7.0,
            ellipse_major_m: 2_400.0,
            ellipse_minor_m: 180.0,
            ellipse_bearing_deg: 45.0,
            converged: false,
            samples: 6,
            mass: 1.0,
        }
    }

    fn node(id: &str, kind: &str) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body: NodeBody::default_for(kind).expect("a node kind"),
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn edge(from: &str, to: &str, port: &str) -> PatchEdge {
        PatchEdge {
            from: PortRef {
                node: from.to_owned(),
                port: "events".to_owned(),
            },
            to: PortRef {
                node: to.to_owned(),
                port: port.to_owned(),
            },
        }
    }

    #[test]
    fn reads_the_ellipse_in_units_a_driver_can_judge() {
        assert_eq!(spread_label(Some(&estimate())), "2.4 km \u{d7} 180 m");
        assert_eq!(spread_label(None), "-");
        assert_eq!(estimate_label(Some(&estimate())), "51.50000, 7.00000");
        assert_eq!(estimate_label(None), "-");
    }

    #[test]
    fn says_how_long_ago_a_finder_last_reported() {
        let now: Timestamp = "2026-01-01T00:10:00Z".parse().expect("a time");
        assert_eq!(
            station_age(&station("2026-01-01T00:09:58Z"), now),
            "just now"
        );
        assert_eq!(
            station_age(&station("2026-01-01T00:09:30Z"), now),
            "30s ago"
        );
        assert_eq!(station_age(&station("2026-01-01T00:05:00Z"), now), "5m ago");
        assert_eq!(station_age(&station("who knows"), now), "just now");
    }

    #[test]
    fn a_station_row_reads_bearing_spread_and_learned_offset() {
        let now: Timestamp = "2026-01-01T00:10:00Z".parse().expect("a time");
        assert_eq!(
            station_cells(&station("2026-01-01T00:09:30Z"), now),
            ["north", "44.0\u{b0}", "3.0\u{b0}", "+4.5\u{b0}", "30s ago"]
        );
        let blank = DfStation {
            last_bearing_deg: f32::NAN,
            sigma_deg: 0.0,
            align_deg: None,
            ..station("")
        };
        assert_eq!(station_bearing(&blank), "-");
        assert_eq!(station_sigma(&blank), "-");
        assert_eq!(station_align(&blank), "-");
        let west = DfStation {
            align_deg: Some(-2.0),
            ..station("")
        };
        assert_eq!(station_align(&west), "-2.0\u{b0}");
    }

    #[test]
    fn guidance_says_where_to_drive_or_why_it_cannot() {
        let fusion = DfFusionState {
            nav: Some(NavTarget {
                lat: 51.5,
                lon: 7.0,
                kind: NavTargetKind::Probe,
                revision: 1,
                distance_m: 1_500.0,
                bearing_deg: 134.6,
            }),
            ..DfFusionState::default()
        };
        assert_eq!(
            guidance(Some(&fusion)),
            (
                "Drive across \u{b7} 135\u{b0}".to_owned(),
                Some("1.5 km away".to_owned())
            )
        );
        let blind = DfFusionState {
            no_guide_position: true,
            no_bearings: true,
            ..DfFusionState::default()
        };
        assert_eq!(
            guidance(Some(&blind)).1.as_deref(),
            Some("Wire a GPS in to guide it")
        );
        let quiet = DfFusionState {
            no_bearings: true,
            ..DfFusionState::default()
        };
        assert_eq!(
            guidance(Some(&quiet)).1.as_deref(),
            Some("No bearings to steer by yet")
        );
        assert_eq!(guidance(None), ("-".to_owned(), None));
    }

    #[test]
    fn the_readout_lists_estimate_spread_guidance_and_bearings() {
        let fusion = DfFusionState {
            estimate: Some(estimate()),
            samples: 6,
            ..DfFusionState::default()
        };
        let rows = readout_rows(Some(&fusion));
        let labels: Vec<_> = rows.iter().map(|row| row.0).collect();
        assert_eq!(labels, ["Estimate", "Spread", "Guidance", "Bearings"]);
        assert_eq!(rows[3].1, "6");
        assert_eq!(readout_rows(None)[3].1, "0");
    }

    #[test]
    fn picking_set_keeps_a_half_life_already_chosen() {
        let held = FusionDecay::HalfLife { seconds: 120 };
        let set = decay_options()[3].0;
        assert_eq!(decay_with(set, held), held);
        assert_eq!(
            decay_with(set, FusionDecay::Auto),
            FusionDecay::HalfLife {
                seconds: FUSION_LIMITS.half_life_seed_s
            }
        );
        assert_eq!(decay_with(FusionDecay::Fixed, held), FusionDecay::Fixed);
        assert_eq!(decay_choice(held), set);
        assert_eq!(half_life_of(held), Some(120));
        assert_eq!(half_life_of(FusionDecay::Moving), None);
    }

    #[test]
    fn half_lives_read_in_minutes_when_they_divide() {
        assert_eq!(half_life_title(60), "Half life 1 min");
        assert_eq!(half_life_title(45), "Half life 45 s");
        assert_eq!(decay_option_title(FusionDecay::Moving), "Half life 30 min");
        assert_eq!(fade_title(None), FADE_TITLE);
        let fusion = DfFusionState {
            half_life_s: 90,
            ..DfFusionState::default()
        };
        assert_eq!(
            fade_title(Some(&fusion)),
            "Old bearings count half after 90 s"
        );
    }

    #[test]
    fn only_bearing_producers_wired_to_events_count_as_sources() {
        let graph = PatchGraph {
            nodes: vec![
                node("df", "df"),
                node("hunt", "hunt"),
                node("filter", "event_filter"),
                node("map", "map"),
                node("cross", "triangulation"),
            ],
            edges: vec![
                edge("df", "cross", "events"),
                edge("df", "cross", "events"),
                edge("hunt", "cross", "events"),
                edge("filter", "cross", "events"),
                edge("map", "cross", "events"),
                edge("df", "map", "events"),
                edge("hunt", "cross", "position"),
            ],
        };
        assert_eq!(fusion_sources(&graph, "cross"), 3);
        assert_eq!(subtitle(1, 3), "1 of 3 reporting");
        assert_eq!(nav_title(NavMode::Off), None);
    }

    #[test]
    fn settings_are_read_and_written_on_a_triangulation_only() {
        let mut body = NodeBody::Triangulation(TriangulationNode::default());
        assert!(settings_of(&body).is_some_and(|params| params.valid()));
        if let Some(params) = settings_mut(&mut body) {
            params.align = false;
        }
        assert!(settings_of(&body).is_some_and(|params| !params.align));
        assert!(settings_of(&NodeBody::Map).is_none());
    }

    #[test]
    fn only_nonzero_counters_become_stats() {
        assert!(stats(None).is_empty());
        let fusion = DfFusionState {
            refused: 3,
            ..DfFusionState::default()
        };
        let shown = stats(Some(&fusion));
        assert_eq!(shown.len(), 1);
        assert_eq!((shown[0].label, shown[0].count), ("Refused", 3));
    }
}
