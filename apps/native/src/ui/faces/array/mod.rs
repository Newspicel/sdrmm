#[cfg(test)]
mod fixtures;
mod geometry;
mod geometry_view;
mod labels;
mod lanes;
mod make;
mod rows;
mod settings;
mod sheet;
mod status;
mod tune;

use std::time::{Duration, SystemTime};

use sdrmm_wire::{
    array::{ArrayNode, ArrayStatus},
    patch::NodeBody,
};
use zgui::prelude::*;

pub use make::make_array_button;

use self::rows::{LaneRow, Sets, lane_rows, member_devices};
use crate::{
    binding,
    store::Store,
    ui::{
        faces::processor::{self, head},
        kit_sources::install,
    },
};

const TICK: Duration = Duration::from_secs(1);

#[derive(Clone)]
struct Array {
    store: Store,
    node: String,
    settings: Memo<ArrayNode>,
    status: Memo<Option<ArrayStatus>>,
    members: Memo<Vec<String>>,
    sets: Memo<Sets>,
    rows: Memo<Vec<LaneRow>>,
    wired: Memo<usize>,
    now: RwSignal<SystemTime>,
}

fn array_body(body: &mut NodeBody) -> Option<&mut ArrayNode> {
    match body {
        NodeBody::Array(array) => Some(array),
        _ => None,
    }
}

fn settings_of(store: Store, node: &str) -> ArrayNode {
    store.graph.with(|graph| {
        graph
            .node(node)
            .and_then(|found| match &found.body {
                NodeBody::Array(array) => Some(array.clone()),
                _ => None,
            })
            .unwrap_or_default()
    })
}

fn member_sets(store: Store, node: &str) -> Sets {
    let graph = store.graph.get();
    let state = store.state.get();
    let bound = binding::device_sets(&graph, &state.device_sets);
    member_devices(&graph, node)
        .into_iter()
        .filter_map(|member| {
            let id = *bound.get(&member)?;
            let set = state.device_sets.iter().find(|set| set.id == id)?.clone();
            Some((member, set))
        })
        .collect()
}

impl Array {
    fn new(store: Store, node: String) -> Self {
        let settings = {
            let node = node.clone();
            Memo::new(move |_| settings_of(store, &node))
        };
        let status = {
            let node = node.clone();
            Memo::new(move |_| {
                let seeded = store.state.get();
                store
                    .arrays
                    .with(|book| book.array(&node, &seeded.arrays).cloned())
            })
        };
        let members = {
            let node = node.clone();
            Memo::new(move |_| store.graph.with(|graph| member_devices(graph, &node)))
        };
        let sets = {
            let node = node.clone();
            Memo::new(move |_| member_sets(store, &node))
        };
        let rows = {
            let node = node.clone();
            Memo::new(move |_| {
                store.graph.with(|graph| {
                    sets.with(|sets| {
                        status.with(|status| lane_rows(graph, sets, &node, status.as_ref()))
                    })
                })
            })
        };
        let wired = {
            let node = node.clone();
            Memo::new(move |_| {
                store
                    .graph
                    .with(|graph| graph.array_wired_lanes(&node) as usize)
            })
        };
        let now = RwSignal::new(SystemTime::now());
        let tick = set_interval(TICK, move || now.set(SystemTime::now()));
        on_cleanup_local(move || drop(tick));
        Self {
            store,
            node,
            settings,
            status,
            members,
            sets,
            rows,
            wired,
            now,
        }
    }

    fn edit(&self, change: impl FnOnce(&mut ArrayNode)) {
        processor::edit(self.store, &self.node, array_body, change);
    }

    fn read<T>(&self, pick: impl Fn(&ArrayNode) -> T + Send + Sync + 'static) -> Signal<T>
    where
        T: Send + Sync + 'static,
    {
        let settings = self.settings;
        Signal::derive(move || settings.with(&pick))
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    let titled = node.clone();
    install();
    sheet::install();
    let cx = Array::new(store, node);
    let subtitle = {
        let (status, rows) = (cx.status, cx.rows);
        Signal::derive(move || {
            status.with(|status| labels::subtitle(status.as_ref(), rows.with(Vec::len)))
        })
    };
    head(store, titled, subtitle);
    view! {
        column(class = "face af") {
            {status::dial(&cx)}
            {settings::chips(&cx)}
            {status::readouts(&cx)}
            {status::failure(&cx)}
            {lanes::lanes(&cx)}
            {settings::folds(&cx)}
            {status::footer(&cx)}
        }
    }
}
