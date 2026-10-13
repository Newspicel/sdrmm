use std::time::Duration;

use sdrmm_engine::{Lane, Placeable};
use sdrmm_wire::{
    ChannelInfo, DeviceSet, Feed, IQ_PORT, NodeBody, PatchApplyReport, PatchGraph, PatchRefusal,
    ResolvedPatch, Role, ServerEvent, StateScope, StateSnapshot, WorkspaceState,
};
use tokio::{sync::broadcast::error::RecvError, time::Instant};

use crate::{AppState, workspace};

const SETTLE_IDLE: Duration = Duration::from_millis(150);
const SETTLE_MAX_WAIT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Carried {
    lane: Lane,
    channel: u32,
}

struct Slot {
    decoder: Placeable,
    carried: Option<Carried>,
    fixed: bool,
}

pub(crate) struct Plan {
    slots: Vec<Slot>,
    refused: Vec<(bool, PatchRefusal)>,
}

impl Plan {
    fn decoders(&self) -> Vec<Placeable> {
        self.slots
            .iter()
            .filter(|slot| !slot.fixed)
            .map(|slot| slot.decoder.clone())
            .collect()
    }

    fn flexible_only(mut self) -> Self {
        self.slots
            .retain(|slot| !slot.fixed && slot.decoder.lanes.len() > 1);
        self
    }

    fn only(mut self, fixed: bool) -> Self {
        self.slots.retain(|slot| slot.fixed == fixed);
        self.refused.retain(|(held, _)| *held == fixed);
        self
    }
}

fn carried_by<'a>(
    state: &'a StateSnapshot,
    node: &str,
    channel_type: &str,
) -> Option<(Carried, &'a DeviceSet, &'a ChannelInfo)> {
    state.device_sets.iter().find_map(|set| {
        set.channels
            .iter()
            .find(|channel| {
                channel.node.as_deref() == Some(node)
                    && channel.settings.params.type_id() == channel_type
            })
            .map(|channel| {
                let carried = Carried {
                    lane: Lane {
                        device_set: set.id,
                        stream: channel.stream,
                    },
                    channel: channel.id,
                };
                (carried, set, channel)
            })
    })
}

fn holds_channel(set: &DeviceSet, channel: &ChannelInfo) -> bool {
    set.scanners
        .iter()
        .any(|scan| scan.settings.channel == channel.id)
        || set
            .hunts
            .iter()
            .any(|hunt| hunt.settings.channel == channel.id)
        || !channel.audio_recordings.is_empty()
        || channel.baseband_recording.is_some()
        || channel.network_export.is_some()
}

fn opens_radio(patch: &ResolvedPatch, feed: &Feed) -> bool {
    patch
        .node(&feed.source)
        .is_some_and(|source| source.kind.role() == Role::OpensRadio)
}

fn radio_feeds<'a>(patch: &'a ResolvedPatch, node: &str) -> impl Iterator<Item = &'a Feed> {
    patch
        .feeds(node, IQ_PORT)
        .iter()
        .filter(|feed| opens_radio(patch, feed))
}

fn lanes_of<'a>(feeds: impl Iterator<Item = &'a Feed>) -> Vec<Lane> {
    feeds
        .filter_map(|feed| feed.tap.as_ref()?.lane())
        .map(|(device_set, stream)| Lane { device_set, stream })
        .collect()
}

fn wired_lanes(patch: &ResolvedPatch, node: &str) -> (Vec<Lane>, bool) {
    let radio = lanes_of(radio_feeds(patch, node));
    if !radio.is_empty() {
        return (radio, false);
    }
    let fixed = lanes_of(
        patch
            .feeds(node, IQ_PORT)
            .iter()
            .filter(|feed| !opens_radio(patch, feed)),
    );
    (fixed.into_iter().take(1).collect(), true)
}

fn lane_center(state: &StateSnapshot, lane: Lane) -> Option<f64> {
    state
        .device_sets
        .iter()
        .find(|set| set.id == lane.device_set)?
        .virtual_lanes
        .iter()
        .find(|held| held.stream == lane.stream)
        .map(|held| held.center_hz)
}

pub(crate) fn plan(
    graph: &PatchGraph,
    patch: &ResolvedPatch,
    state: &StateSnapshot,
    saved: &WorkspaceState,
) -> Plan {
    let mut slots = Vec::new();
    let mut refused = Vec::new();
    for node in &graph.nodes {
        let NodeBody::Channel(channel) = &node.body else {
            continue;
        };
        let (lanes, fixed) = wired_lanes(patch, &node.id);
        let Some(&first) = lanes.first() else {
            continue;
        };
        let live = carried_by(state, &node.id, &channel.channel_type);
        let center_hz = fixed.then(|| lane_center(state, first)).flatten();
        let settings = live
            .map(|(_, _, channel)| channel.settings.clone())
            .or_else(|| {
                workspace::channel_settings(&node.id, &channel.channel_type, saved, center_hz)
            });
        let Some(settings) = settings else {
            refused.push((
                fixed,
                PatchRefusal {
                    node: node.id.clone(),
                    reason: format!("this build has no channel type {:?}", channel.channel_type),
                },
            ));
            continue;
        };
        slots.push(Slot {
            decoder: Placeable {
                node: node.id.clone(),
                settings,
                lanes,
                held: live.map(|(carried, _, _)| carried.lane),
                pinned: live.is_some_and(|(_, set, channel)| holds_channel(set, channel)),
            },
            carried: live.map(|(carried, _, _)| carried),
            fixed,
        });
    }
    Plan { slots, refused }
}

fn open(app: &AppState, slot: &Slot, lane: Lane, report: &mut PatchApplyReport) -> Option<Carried> {
    let decoder = &slot.decoder;
    match app.engine.add_channel_for(
        lane.device_set,
        lane.stream,
        decoder.settings.clone(),
        Some(&decoder.node),
    ) {
        Ok(channel) => {
            report.created += 1;
            Some(Carried { lane, channel })
        }
        Err(err) => {
            report.refused.push(PatchRefusal {
                node: decoder.node.clone(),
                reason: err.to_string(),
            });
            None
        }
    }
}

fn close(app: &AppState, slot: &Slot, carried: Carried, report: &mut PatchApplyReport) -> bool {
    match app
        .engine
        .remove_channel(carried.lane.device_set, carried.channel)
    {
        Ok(()) => {
            report.closed += 1;
            true
        }
        Err(err) if err.is_not_found() => true,
        Err(err) => {
            report.refused.push(PatchRefusal {
                node: slot.decoder.node.clone(),
                reason: err.to_string(),
            });
            false
        }
    }
}

fn settle(app: &AppState, plan: &Plan, report: &mut PatchApplyReport) {
    report.placement = None;
    report
        .refused
        .extend(plan.refused.iter().map(|(_, refusal)| refusal.clone()));
    let allocation = app.engine.place_channels(&plan.decoders());
    let placements = &allocation.placements;
    let refused = report.refused.len();
    if !allocation.coverage.optimal() {
        tracing::warn!(
            heard = allocation.coverage.heard,
            upper_bound = allocation.coverage.upper_bound,
            "decoder allocation search stopped before proving optimality"
        );
    }
    for slot in plan.slots.iter().filter(|slot| !slot.fixed) {
        let target = placements
            .iter()
            .find(|placement| placement.node == slot.decoder.node)
            .map(|placement| placement.lane);
        match (slot.carried, target) {
            (Some(carried), Some(lane)) if carried.lane == lane => {}
            (Some(carried), Some(lane)) => {
                if let Some(created) = open(app, slot, lane, report)
                    && !close(app, slot, carried, report)
                {
                    close(app, slot, created, report);
                }
            }
            (None, Some(lane)) => {
                open(app, slot, lane, report);
            }
            (None, None) => {
                if let Some(lane) = slot.decoder.lanes.first() {
                    open(app, slot, *lane, report);
                }
            }
            (Some(_), None) => report.refused.push(PatchRefusal {
                node: slot.decoder.node.clone(),
                reason: "no wired receive stream is available".to_owned(),
            }),
        }
    }
    if report.refused.len() == refused {
        report.placement = Some(allocation.coverage);
    }
}

fn settle_fixed(app: &AppState, plan: &Plan, report: &mut PatchApplyReport) {
    report
        .refused
        .extend(plan.refused.iter().map(|(_, refusal)| refusal.clone()));
    for slot in &plan.slots {
        let Some(&lane) = slot.decoder.lanes.first() else {
            continue;
        };
        match slot.carried {
            Some(carried) if carried.lane == lane => {}
            Some(carried) => {
                if let Some(created) = open(app, slot, lane, report)
                    && !close(app, slot, carried, report)
                {
                    close(app, slot, created, report);
                }
            }
            None => {
                open(app, slot, lane, report);
            }
        }
    }
}

pub(crate) fn settle_workspace(
    app: &AppState,
    graph: &PatchGraph,
    saved: &WorkspaceState,
    report: &mut PatchApplyReport,
) {
    let state = app.engine.snapshot();
    let patch = workspace::resolve(graph, &state);
    settle(app, &plan(graph, &patch, &state, saved).only(false), report);
}

pub(crate) fn settle_lanes(
    app: &AppState,
    graph: &PatchGraph,
    saved: &WorkspaceState,
    report: &mut PatchApplyReport,
) {
    let state = app.engine.snapshot();
    let patch = workspace::resolve(graph, &state);
    settle_fixed(app, &plan(graph, &patch, &state, saved).only(true), report);
}

pub(crate) fn settle_active(app: &AppState) -> bool {
    let resolved = app.resolved.current();
    let Some(workspace) = resolved.patch.workspace else {
        return false;
    };
    let graph = &resolved.graph;
    let flexible = graph.nodes.iter().any(|node| {
        matches!(node.body, NodeBody::Channel(_))
            && radio_feeds(&resolved.patch, &node.id).count() > 1
    });
    if !flexible {
        return false;
    }
    let saved = match app.store.workspace_state(workspace) {
        Ok(saved) => saved,
        Err(err) => {
            tracing::warn!(%err, "could not read the workspace to place its decoders");
            return false;
        }
    };
    let state = app.engine.snapshot();
    let patch = workspace::resolve(graph, &state);
    let plan = plan(graph, &patch, &state, &saved).flexible_only();
    let mut report = PatchApplyReport::default();
    settle(app, &plan, &mut report);
    for refusal in &report.refused {
        tracing::warn!(
            node = refusal.node,
            reason = refusal.reason,
            "a decoder could not be placed"
        );
    }
    report.created > 0 || report.closed > 0
}

fn moves_a_radio(event: &ServerEvent) -> bool {
    matches!(
        event,
        ServerEvent::StateChanged {
            scope: StateScope::All | StateScope::DeviceSet(_)
        }
    )
}

pub(crate) fn spawn_settling(state: &AppState) {
    let mut events = state.engine.subscribe_events();
    let state = state.clone();
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!("no runtime in context: decoders will not follow their radios");
        return;
    };
    let _guard = handle.enter();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) if !moves_a_radio(&event) => continue,
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
            let hard = Instant::now() + SETTLE_MAX_WAIT;
            let mut idle = Instant::now() + SETTLE_IDLE;
            let mut open = true;
            while open {
                tokio::select! {
                    () = tokio::time::sleep_until(idle.min(hard)) => break,
                    received = events.recv() => match received {
                        Ok(event) => {
                            if moves_a_radio(&event) {
                                idle = Instant::now() + SETTLE_IDLE;
                            }
                        }
                        Err(RecvError::Lagged(_)) => idle = Instant::now() + SETTLE_IDLE,
                        Err(RecvError::Closed) => open = false,
                    },
                }
            }
            let settling = state.clone();
            if let Err(err) = tokio::task::spawn_blocking(move || {
                let _serialized = settling
                    .apply_gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                settle_active(&settling)
            })
            .await
            {
                tracing::warn!(%err, "decoder placement panicked");
            }
            if !open {
                break;
            }
        }
    });
}

#[cfg(test)]
#[path = "tests/placement.rs"]
mod tests;
