//! The control-plane half of every processor whose descriptor declares a report enricher: it
//! gathers what the patch around the node knows, lets the enricher complete each report and
//! publishes the events that come back.

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
};

use jiff::Timestamp;
use sdrmm_channels::array_processor::{
    ArrayPlace, EnrichCtx, MakeEnricher, Raised, Refusal, ReportEnricher, descriptor_for,
    processor_descriptor,
};
use sdrmm_wire::{
    ARRAY_PORT, ArrayFailure, ArrayStatus, Capability, DecodedRecord, EventOrigin, NO_CHANNEL,
    PatchGraph, PatchNode, Payload, PortDirection, PositionFix, ProcessorAction, ProcessorParams,
    ProcessorReading, ResolvedPatch, ServerEvent, present::TrackFix,
};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::{AppState, array, decoded::Decoded, resolved::Resolved};

#[cfg(test)]
mod tests;

/// The nodes a processor reads from: its array and the sources of its position inputs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Wiring {
    array: Option<String>,
    positions: Vec<(String, String)>,
}

impl Wiring {
    fn of(patch: &ResolvedPatch, node: &PatchNode) -> Self {
        let source = |port: &str| {
            patch
                .first_feed(&node.id, port)
                .map(|feed| feed.source.clone())
        };
        Self {
            array: source(ARRAY_PORT),
            positions: node
                .body
                .ports(None)
                .into_iter()
                .filter(|port| {
                    port.direction == PortDirection::In && matches!(port.payload, Payload::Position)
                })
                .filter_map(|port| source(&port.name).map(|from| (port.name, from)))
                .collect(),
        }
    }
}

struct Held {
    enricher: Box<dyn ReportEnricher>,
    wiring: Wiring,
    params: ProcessorParams,
    refused: Option<Refusal>,
}

struct Context {
    params: ProcessorParams,
    array: Option<ArrayPlace>,
    positions: Vec<(String, PositionFix)>,
}

impl Context {
    fn of(state: &AppState, wiring: &Wiring, params: ProcessorParams) -> Self {
        Self {
            params,
            array: wiring
                .array
                .as_deref()
                .and_then(|array| array::array_pose(state, array))
                .map(|pose| ArrayPlace {
                    lat: pose.lat,
                    lon: pose.lon,
                    altitude_m: pose.altitude_m,
                    heading_deg: pose.heading_deg,
                }),
            positions: wiring
                .positions
                .iter()
                .filter_map(|(port, from)| state.gps.fix(from).map(|fix| (port.clone(), fix)))
                .collect(),
        }
    }

    fn ctx(&self) -> EnrichCtx<'_> {
        EnrichCtx {
            params: &self.params,
            array: self.array,
            positions: &self.positions,
        }
    }
}

fn enricher_of(params: &ProcessorParams) -> Option<MakeEnricher> {
    descriptor_for(params)?.enricher
}

#[derive(Default)]
pub(crate) struct ReportHub {
    nodes: Mutex<HashMap<String, Held>>,
    latest: Mutex<HashMap<String, ProcessorReading>>,
}

impl ReportHub {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Held>> {
        self.nodes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn shown(&self) -> MutexGuard<'_, HashMap<String, ProcessorReading>> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps `reading` as the one its node shows now.
    pub(crate) fn keep(&self, node: &str, reading: &ProcessorReading) {
        self.shown().insert(node.to_owned(), reading.clone());
    }

    pub(crate) fn latest(&self, node: &str) -> Option<ProcessorReading> {
        self.shown().get(node).cloned()
    }

    fn show(&self, state: &AppState, node: &str, reading: ProcessorReading) {
        self.keep(node, &reading);
        state
            .engine
            .emit_event(ServerEvent::processor_update(node.to_owned(), reading));
    }

    fn held(&self, node: &str) -> Option<(Wiring, ProcessorParams)> {
        self.lock()
            .get(node)
            .map(|held| (held.wiring.clone(), held.params.clone()))
    }

    fn bind(
        &self,
        node: &str,
        wiring: Wiring,
        params: ProcessorParams,
        make: MakeEnricher,
    ) -> bool {
        let mut nodes = self.lock();
        let Some(held) = nodes.get_mut(node) else {
            nodes.insert(
                node.to_owned(),
                Held {
                    enricher: make(),
                    wiring,
                    params,
                    refused: None,
                },
            );
            return true;
        };
        let rewired = held.wiring != wiring;
        held.wiring = wiring;
        held.params = params;
        rewired || held.refused.is_some()
    }

    fn nodes(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    #[cfg(test)]
    pub(crate) fn knows(&self, node: &str) -> bool {
        self.lock().contains_key(node)
    }

    fn forget(&self, node: &str) {
        self.lock().remove(node);
    }

    /// Binds a node whose report beat the patch apply to it.
    fn adopt(&self, state: &AppState, node: &str) -> Option<(Wiring, ProcessorParams)> {
        let resolved = state.resolved.current();
        let patch_node = resolved.graph.node(node)?;
        let params = patch_node.body.processor_params()?;
        let make = enricher_of(&params)?;
        let wiring = Wiring::of(&resolved.patch, patch_node);
        self.bind(node, wiring.clone(), params.clone(), make);
        Some((wiring, params))
    }

    /// Completes `reading` when its node has an enricher, publishing the events it raises.
    pub(crate) fn on_report(&self, state: &AppState, node: &str, reading: &mut ProcessorReading) {
        if !enriched(reading) {
            return;
        }
        let Some((wiring, params)) = self.held(node).or_else(|| self.adopt(state, node)) else {
            tracing::warn!(node, "a report from a processor that is gone");
            return;
        };
        let context = Context::of(state, &wiring, params);
        let raised = {
            let mut nodes = self.lock();
            let Some(held) = nodes.get_mut(node) else {
                return;
            };
            held.refused = None;
            held.enricher.complete(&context.ctx(), reading)
        };
        publish(state, node, anchor(state, wiring.array.as_deref()), raised);
    }

    fn refuse(&self, state: &AppState, node: &str, refusal: Refusal) {
        let refused = {
            let mut nodes = self.lock();
            let Some(held) = nodes.get_mut(node) else {
                return;
            };
            if held.refused.as_ref() == Some(&refusal) {
                return;
            }
            let refused = held.enricher.refused(&refusal);
            held.refused = Some(refusal);
            refused
        };
        let (reading, raised) = refused;
        publish(state, node, 0, raised);
        self.show(state, node, reading);
    }

    fn wait(&self, state: &AppState, node: &str) {
        let Some((wiring, params)) = self.held(node) else {
            return;
        };
        let context = Context::of(state, &wiring, params);
        let reading = {
            let mut nodes = self.lock();
            let Some(held) = nodes.get_mut(node) else {
                return;
            };
            held.refused = None;
            held.enricher.waiting(&context.ctx())
        };
        self.show(state, node, reading);
    }

    pub(crate) fn act(&self, node: &str, action: ProcessorAction) {
        if let Some(held) = self.lock().get_mut(node) {
            held.enricher.act(action, self.shown().get_mut(node));
        }
    }

    pub(crate) fn lagged(&self, count: u64) {
        for held in self.lock().values_mut() {
            held.enricher.lagged(count);
        }
    }

    fn observe(&self, node: &str, at: Timestamp, fix: &TrackFix) {
        if let Some(held) = self.lock().get_mut(node) {
            held.enricher.observe(at, fix);
        }
    }
}

fn enriched(reading: &ProcessorReading) -> bool {
    processor_descriptor(reading.type_id()).is_some_and(|descriptor| descriptor.enricher.is_some())
}

fn anchor(state: &AppState, array: Option<&str>) -> u32 {
    let Some(array) = array else {
        return 0;
    };
    state
        .engine
        .array_statuses()
        .into_iter()
        .find(|status| status.node == array)
        .and_then(|status| status.anchor)
        .or_else(|| {
            let spec = state.arrays.binding(array)?.spec?;
            spec.lanes
                .into_iter()
                .flatten()
                .next()
                .map(|lane| lane.device_set)
        })
        .unwrap_or(0)
}

fn publish(state: &AppState, node: &str, anchor: u32, raised: Vec<Raised>) {
    for raised in raised {
        state.engine.publish_decoded(DecodedRecord {
            origin: Some(EventOrigin {
                node: node.to_owned(),
                transmission: 0,
            }),
            device_set: anchor,
            channel: NO_CHANNEL,
            at: raised.at,
            freq_hz: raised.freq_hz,
            event: raised.event,
            sinks: Vec::new(),
            callsign: None,
            view: None,
        });
    }
}

fn refusal(
    state: &AppState,
    statuses: &[ArrayStatus],
    node: &str,
    wiring: &Wiring,
) -> Option<Refusal> {
    let Some(array) = wiring.array.as_deref() else {
        return Some(Refusal::NoArray);
    };
    let refused = statuses
        .iter()
        .flat_map(|status| &status.processors)
        .find(|processor| processor.node == node)
        .and_then(|processor| processor.error.clone());
    if let Some(refused) = refused {
        return Some(Refusal::Refused(refused));
    }
    match state.arrays.binding(array) {
        Some(held) if held.spec.is_some() => None,
        held => Some(Refusal::Refused(
            held.and_then(|held| held.problem)
                .unwrap_or_else(|| ArrayFailure::Unwired.to_string()),
        )),
    }
}

pub(crate) fn reconcile(state: &AppState, resolved: &Resolved) -> Vec<(String, String)> {
    release(state, &resolved.graph);
    let statuses = state.engine.array_statuses();
    for node in &resolved.graph.nodes {
        let Some(params) = node.body.processor_params() else {
            continue;
        };
        let Some(make) = enricher_of(&params) else {
            continue;
        };
        let wiring = Wiring::of(&resolved.patch, node);
        let fresh = state.reports.bind(&node.id, wiring.clone(), params, make);
        match refusal(state, &statuses, &node.id, &wiring) {
            Some(refusal) => state.reports.refuse(state, &node.id, refusal),
            None if fresh => state.reports.wait(state, &node.id),
            None => {}
        }
    }
    Vec::new()
}

pub(crate) fn release(state: &AppState, graph: &PatchGraph) {
    for node in state.reports.nodes() {
        let enriched = graph
            .node(&node)
            .and_then(|found| found.body.processor_params())
            .and_then(|params| enricher_of(&params))
            .is_some();
        if !enriched {
            state.reports.forget(&node);
        }
    }
    state.reports.shown().retain(|node, _| {
        graph
            .node(node)
            .is_some_and(|found| found.body.is_array_processor())
    });
}

pub(crate) fn start(state: &AppState) {
    let records = state.decoded.subscribe();
    let fed = state.clone();
    let _detached = crate::spawn_task("sdrmm-report-truth", move || feed_tracks(fed, records));
}

async fn feed_tracks(state: AppState, mut records: broadcast::Receiver<Decoded>) {
    loop {
        match records.recv().await {
            Ok(Decoded::Record(routed)) => observe(&state, &routed.record),
            Ok(Decoded::Lost(count)) | Err(RecvError::Lagged(count)) => {
                tracing::warn!(count, "processors missed decoded tracks");
            }
            Err(RecvError::Closed) => break,
        }
    }
}

fn observe(state: &AppState, record: &DecodedRecord) {
    if !record.event.kind.has(Capability::Track) {
        return;
    }
    let view = record
        .view
        .clone()
        .unwrap_or_else(|| record.event.present());
    let Some(fix) = &view.track else {
        return;
    };
    let at = record
        .at
        .parse::<Timestamp>()
        .unwrap_or_else(|_| Timestamp::now());
    for sink in &record.sinks {
        state.reports.observe(sink, at, fix);
    }
}
