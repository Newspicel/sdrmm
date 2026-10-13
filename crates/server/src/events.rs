use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use sdrmm_wire::{
    DecodedRecord, EventFilterNode, EventKind, Keeps, KindSet, PatchGraph, Payload, PortType,
    ResolvedNode, ResolvedPatch, Role, present::Presentation,
};

mod stations;

pub(crate) use stations::Stations;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Route {
    pub sink: String,
    pub filters: Vec<EventFilterNode>,
    pub accepts: KindSet,
}

impl Route {
    fn passes(&self, kind: EventKind, view: &Presentation) -> bool {
        self.filters.iter().all(|filter| filter.admits(kind, view))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct EventRoutes {
    pub workspace: Option<i64>,
    by_source: HashMap<String, Vec<Route>>,
    history: HashSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteOutcome {
    Delivered,
    Ignored,
    Unrouted,
    Unattributed,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Routed {
    pub record: DecodedRecord,
    pub source: Option<String>,
    pub workspace: Option<i64>,
    pub history: bool,
}

#[cfg(test)]
impl Routed {
    pub(crate) fn unattributed(record: DecodedRecord) -> Self {
        Self {
            source: record.origin.as_ref().map(|origin| origin.node.clone()),
            record,
            workspace: None,
            history: true,
        }
    }
}

fn accepts(node: &ResolvedNode, port: &str) -> KindSet {
    match node.input(port).map(|port| &port.spec.payload) {
        Some(Payload::Events { kinds }) => *kinds,
        _ => KindSet::EMPTY,
    }
}

fn filters_on(via: &[String], graph: &PatchGraph) -> Vec<EventFilterNode> {
    via.iter()
        .filter_map(|id| graph.node(id)?.body.event_rule().cloned())
        .collect()
}

impl EventRoutes {
    pub(crate) fn resolve(patch: &ResolvedPatch, graph: &PatchGraph) -> Self {
        let mut by_source: HashMap<String, Vec<Route>> = HashMap::new();
        let mut history = HashSet::new();
        let sinks = patch
            .nodes
            .iter()
            .filter(|node| !matches!(node.kind.role(), Role::Pass(_)));
        for node in sinks {
            for port in node
                .inputs()
                .filter(|port| port.spec.port_type == PortType::Events)
            {
                if port.spec.keeps.contains(&Keeps::History) {
                    history.insert(node.id.clone());
                }
                for feed in &port.feeds {
                    by_source
                        .entry(feed.source.clone())
                        .or_default()
                        .push(Route {
                            sink: node.id.clone(),
                            filters: filters_on(&feed.via, graph),
                            accepts: accepts(node, &port.spec.name),
                        });
                }
            }
        }
        Self {
            workspace: patch.workspace,
            by_source,
            history,
        }
    }

    pub(crate) fn keeps_history(&self, sinks: &[String]) -> bool {
        sinks.iter().any(|sink| self.history.contains(sink))
    }

    pub(crate) fn routes_from(&self, source: &str) -> &[Route] {
        self.by_source.get(source).map_or(&[], Vec::as_slice)
    }

    #[cfg(test)]
    pub(crate) fn reaches(&self, source: &str, sink: &str) -> bool {
        self.routes_from(source)
            .iter()
            .any(|route| route.sink == sink)
    }

    pub(crate) fn admits(
        &self,
        source: &str,
        sink: &str,
        kind: EventKind,
        view: &Presentation,
    ) -> bool {
        self.routes_from(source).iter().any(|route| {
            route.sink == sink
                && route.accepts.contains(kind)
                && route.filters.iter().all(|filter| filter.admits(kind, view))
        })
    }

    pub(crate) fn route(
        &self,
        record: &mut DecodedRecord,
        stations: &mut Stations,
    ) -> RouteOutcome {
        let Some(source) = record.origin.as_ref().map(|origin| origin.node.clone()) else {
            record.sinks.clear();
            return RouteOutcome::Unattributed;
        };
        let view = record.event.present();
        let judged = stations.judged(&source, record, &view, Instant::now());
        record.view = Some(view);
        let kind = record.event.kind();
        let mut sinks: Vec<String> = Vec::new();
        let mut ignored = false;
        for route in self.routes_from(&source) {
            if sinks.contains(&route.sink) || !route.passes(kind, &judged) {
                continue;
            }
            if route.accepts.contains(kind) {
                sinks.push(route.sink.clone());
            } else {
                ignored = true;
            }
        }
        record.sinks = sinks;
        match (record.sinks.is_empty(), ignored) {
            (false, _) => RouteOutcome::Delivered,
            (true, true) => RouteOutcome::Ignored,
            (true, false) => RouteOutcome::Unrouted,
        }
    }
}

#[cfg(test)]
mod tests;
