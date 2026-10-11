pub mod graph;
mod header;
mod menu;
mod placement;
mod ports;
mod remove;
mod sync;

use std::collections::{HashMap, HashSet};

use sdrmm_wire::{
    device::Coherence,
    patch::{
        ARRAY_LANE_PORT, NodeBody, PatchEdge, PatchGraph, PatchNode, PortBacking, PortDirection,
        PortRef, Position, REFUSAL_NO_PORT, REFUSAL_SHARED_CLOCK, port_stream,
    },
};
use zgui::{prelude::*, reactive::RenderEffect};
use zgui_flow::prelude::{
    BackgroundProps, ChangeSet, CommitOptions, Connection, ConnectionMode, ControlsProps,
    DragHandleProps, EdgeChange, FlowAction, FlowConfig, FlowEvents, FlowHandle, FlowProps, Graph,
    Interaction, KeyBinding, KeyMap, MiniMapProps, NodeChange, NodeId, NodeProps, NodeResizerProps,
    NodeTypes, PendingConnection, Point, Rect, Size, SnapGrid, ZoomLimits,
};

use self::sync::Held;
use crate::{
    store::Store,
    ui::{
        faces::{
            self,
            channel::alert::{RefusalSource, face_alert},
        },
        node::{self, Place},
        widgets::close_menus,
    },
};

pub type Canvas = FlowHandle<(), ()>;

const GRID_STEP: f64 = 24.0;
const COARSE_BELOW: f64 = 0.5;
const FINE_FROM: f64 = 1.25;

#[must_use]
pub fn grid_step(zoom: f64) -> f64 {
    if zoom < COARSE_BELOW {
        GRID_STEP * 2.0
    } else if zoom >= FINE_FROM {
        GRID_STEP / 2.0
    } else {
        GRID_STEP
    }
}

pub fn places_of(store: Store, node: &PatchNode) -> Vec<Place> {
    match &node.body {
        NodeBody::Channel(channel) => match store.descriptor_of(&channel.channel_type) {
            Some(descriptor) => node::places(node, Some(PortBacking::Channel(&descriptor))),
            None => node::places(node, None),
        },
        NodeBody::Device(_) => match store
            .device_set_of(&node.id)
            .and_then(|id| store.set_of(id))
        {
            Some(set) => node::places(node, Some(PortBacking::Device(&set.capabilities))),
            None => node::places(node, None),
        },
        NodeBody::Array(_) => {
            let wired = store.graph.with(|graph| graph.array_wired_lanes(&node.id));
            node::places(node, Some(PortBacking::Lanes(wired)))
        }
        _ => node::places(node, None),
    }
}

#[must_use]
pub fn shape_of(node: &PatchNode) -> String {
    match &node.body {
        NodeBody::Channel(channel) => format!("channel/{}", channel.channel_type),
        body => body.kind().to_owned(),
    }
}

fn edge_of(connection: &Connection) -> Option<PatchEdge> {
    Some(PatchEdge {
        from: PortRef {
            node: connection.source.to_string(),
            port: connection.source_handle.as_ref()?.to_string(),
        },
        to: PortRef {
            node: connection.target.to_string(),
            port: connection.target_handle.as_ref()?.to_string(),
        },
    })
}

pub fn refusal(store: Store, graph: &PatchGraph, connection: &Connection) -> Option<String> {
    let Some(edge) = edge_of(connection) else {
        return Some(String::from(REFUSAL_NO_PORT));
    };
    let descriptors = store.channel_types.get_untracked();
    graph
        .connection_refusal(&edge, Some(&descriptors))
        .or_else(|| shared_clock_refusal(store, graph, &edge))
}

fn shared_clock_refusal(store: Store, graph: &PatchGraph, edge: &PatchEdge) -> Option<String> {
    let lands_on_array = graph
        .node(&edge.to.node)
        .is_some_and(|node| matches!(node.body, NodeBody::Array(_)))
        && port_stream(ARRAY_LANE_PORT, &edge.to.port).is_some();
    let same_radio = graph.edges.iter().any(|other| {
        other.to.node == edge.to.node
            && other.from.node == edge.from.node
            && port_stream(ARRAY_LANE_PORT, &other.to.port).is_some()
    });
    let unclocked = store
        .device_set_of(&edge.from.node)
        .and_then(|id| store.set_of(id))
        .is_some_and(|set| set.capabilities.coherence == Coherence::None);
    (lands_on_array && same_radio && unclocked).then(|| REFUSAL_SHARED_CLOCK.to_owned())
}

pub fn remove_selection(store: Store, nodes: Vec<String>, edges: Vec<String>) {
    if nodes.is_empty() && edges.is_empty() {
        return;
    }
    let graph = store.graph.get_untracked();
    let closing = remove::teardowns(store, &graph, &nodes);
    zgui::task::spawn_local(async move {
        if !remove::close(store, closing).await {
            return;
        }
        store.edit_graph(move |graph| {
            *graph = graph::remove_edges(&graph::remove_nodes(graph, &nodes), &edges);
        });
    });
}

fn occupied(canvas: Canvas) -> Vec<placement::Rect> {
    canvas.with_graph(|shown| {
        shown
            .nodes()
            .map(|node| {
                let size = node.size_or_zero();
                placement::Rect::new(node.position.x, node.position.y, size.width, size.height)
            })
            .collect()
    })
}

pub fn place_new(canvas: Canvas, kind: &str) -> (f32, f32) {
    let view = canvas.view();
    let viewport = view.viewport().get_untracked();
    let pane = view.pane_size().get_untracked();
    let size = placement::drop_size(kind);
    if pane.width <= 0.0 || pane.height <= 0.0 || viewport.zoom <= 0.0 {
        let room = placement::Rect::new(0.0, 0.0, 1200.0, 800.0);
        let at = placement::drop_position(&room, size, &occupied(canvas), placement::SCREEN_GAP);
        return (at.0 as f32, at.1 as f32);
    }
    let visible = viewport.visible_rect(pane);
    let shown = placement::Rect::new(visible.x0, visible.y0, visible.width(), visible.height());
    let gap = placement::SCREEN_GAP / viewport.zoom;
    let at = placement::drop_position(&shown, size, &occupied(canvas), gap);
    let face = placement::Rect::new(
        at.0 - gap,
        at.1 - gap,
        size.0 + 2.0 * gap,
        size.1 + 2.0 * gap,
    );
    if !shown.encloses(&face) {
        let both = shown.union(&face);
        view.fit_rect(
            Rect::new(both.x, both.y, both.x + both.w, both.y + both.h),
            0.0,
        );
    }
    (at.0 as f32, at.1 as f32)
}

#[must_use]
pub fn canvas() -> Canvas {
    let mut config = FlowConfig::default();
    config.connect.auto_add = false;
    config.render.only_visible = false;
    config.render.low_detail_zoom = 0.0;
    config.pan.on_scroll = true;
    config.zoom.scroll_modifier = Some(Modifiers::CONTROL | Modifiers::META);
    config.zoom.on_double_click = false;
    config.zoom.limits = ZoomLimits::new(0.15, 2.0);
    config.zoom.fit_max = Some(1.0);
    config.snap.grid = Some(SnapGrid::square(GRID_STEP));
    config.edges.reconnectable = false;
    config.keys = KeyMap::empty()
        .bind(
            KeyBinding::named(NamedKey::Delete, Modifiers::NONE),
            FlowAction::Delete,
        )
        .bind(
            KeyBinding::named(NamedKey::Backspace, Modifiers::NONE),
            FlowAction::Delete,
        )
        .bind(
            KeyBinding::named(NamedKey::Escape, Modifiers::NONE),
            FlowAction::Escape,
        );
    FlowHandle::with_config(Graph::new(), config)
}

fn held(canvas: Canvas) -> Held {
    let view = canvas.view();
    Held {
        dragging: view
            .dragging()
            .get_untracked()
            .iter()
            .map(ToString::to_string)
            .collect(),
        resizing: view.interaction().get_untracked() == Interaction::Resizing,
    }
}

fn sync(store: Store, canvas: Canvas, graph: &PatchGraph) {
    let places: HashMap<String, Vec<Place>> = graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), places_of(store, node)))
        .collect();
    let dragged = store.presence.with(|room| room.dragged_by_others());
    let drawn: Vec<sync::Drawn> = graph
        .nodes
        .iter()
        .map(|node| sync::moved_by_peer(sync::drawn(node), dragged.get(&node.id)))
        .collect();
    let wires = sync::wires(graph, &places);
    let changes = canvas.with_graph(|shown| sync::diff(&drawn, &wires, shown, &held(canvas)));
    if !changes.is_empty() {
        canvas.commit_with(
            changes,
            CommitOptions {
                record: false,
                observed: false,
                ..CommitOptions::default()
            },
        );
    }
}

fn persist_positions(store: Store, canvas: Canvas, ids: &[NodeId]) {
    let mut moved = false;
    for id in ids {
        if let Some(at) = canvas.node_position(id) {
            store.move_node(id.to_string(), at.x as f32, at.y as f32);
            moved = true;
        }
    }
    if moved {
        store.commit_layout();
    }
}

fn persist_sizes(store: Store, canvas: Canvas, ids: &HashSet<String>) {
    for id in ids {
        if let Some(rect) = canvas.node_rect(&id.as_str().into()) {
            store.resize_node(
                id.clone(),
                rect.x0 as f32,
                rect.y0 as f32,
                rect.width() as f32,
                rect.height() as f32,
            );
        }
    }
}

fn removals(set: &mut ChangeSet<(), ()>) -> (Vec<String>, Vec<String>) {
    let nodes: Vec<String> = set
        .nodes
        .iter()
        .filter_map(|change| match change {
            NodeChange::Remove(id) => Some(id.to_string()),
            _ => None,
        })
        .collect();
    let edges: Vec<String> = set
        .edges
        .iter()
        .filter_map(|change| match change {
            EdgeChange::Remove(id) => Some(id.to_string()),
            _ => None,
        })
        .collect();
    set.nodes
        .retain(|change| !matches!(change, NodeChange::Remove(_)));
    set.edges
        .retain(|change| !matches!(change, EdgeChange::Remove(_)));
    (nodes, edges)
}

#[derive(Clone, Copy)]
struct Gestures {
    resized: StoredValue<HashSet<String>, LocalStorage>,
    pending: StoredValue<Option<PendingConnection>, LocalStorage>,
    selecting: StoredValue<Vec<NodeId>, LocalStorage>,
}

fn attempted(pending: &PendingConnection) -> Option<Connection> {
    let target = pending.target.as_ref()?;
    Connection::orient(&pending.from, &target.handle, ConnectionMode::Loose)
}

fn say_refusal(store: Store, gestures: Gestures) {
    let Some(pending) = gestures.pending.try_update_value(Option::take).flatten() else {
        return;
    };
    let Some(connection) = attempted(&pending) else {
        return;
    };
    if let Some(reason) = refusal(store, &store.graph.get_untracked(), &connection) {
        refuse(store, &connection, reason);
    }
}

fn remember_pending(canvas: Canvas, gestures: Gestures) -> RenderEffect<()> {
    RenderEffect::new(move |_| {
        if let Some(pending) = canvas.view().pending_connection().get() {
            gestures.pending.set_value(Some(pending));
        }
    })
}

fn select_pasted(canvas: Canvas, gestures: Gestures) {
    let wanted = gestures.selecting.get_value();
    if wanted.is_empty() {
        return;
    }
    let landed = canvas.with_graph(|shown| wanted.iter().all(|id| shown.node(id).is_some()));
    if landed {
        gestures.selecting.set_value(Vec::new());
        canvas.select(&wanted, &[]);
    }
}

fn observe_changes(canvas: Canvas, gestures: Gestures, changes: &[NodeChange<()>]) {
    if canvas.view().interaction().get_untracked() != Interaction::Resizing {
        return;
    }
    gestures.resized.update_value(|held| {
        held.extend(changes.iter().filter_map(|change| match change {
            NodeChange::Size { id, .. } => Some(id.to_string()),
            _ => None,
        }));
    });
}

fn finish_resizes(store: Store, canvas: Canvas, gestures: Gestures) -> RenderEffect<()> {
    RenderEffect::new(move |_| {
        if canvas.view().interaction().get() == Interaction::Resizing {
            return;
        }
        let done = gestures
            .resized
            .try_update_value(std::mem::take)
            .unwrap_or_default();
        if !done.is_empty() {
            persist_sizes(store, canvas, &done);
        }
    })
}

fn refuse(store: Store, connection: &Connection, reason: String) {
    store.flag_refusal(
        connection.target.as_str(),
        reason.clone(),
        RefusalSource::Wire,
    );
    store.say(reason);
}

fn connect(store: Store, connection: &Connection) {
    if let Some(reason) = refusal(store, &store.graph.get_untracked(), connection) {
        refuse(store, connection, reason);
        return;
    }
    if let Some(edge) = edge_of(connection) {
        store.edit_graph(move |graph| *graph = graph::add_edge(graph, edge));
    }
}

fn select(store: Store, nodes: &[NodeId]) {
    close_menus();
    let first = nodes.first().map(ToString::to_string);
    if store.selected.get_untracked() != first {
        store.selected.set(first);
    }
}

fn events(
    store: Store,
    canvas: Canvas,
    open: RwSignal<Option<menu::Menu>>,
    gestures: Gestures,
) -> FlowEvents<(), ()> {
    let shown = move |target: menu::Target, flow: Point| {
        close_menus();
        let view = canvas.view();
        let origin = view.pane_origin();
        let screen = view.flow_to_screen(flow);
        open.set(Some(menu::Menu {
            target,
            window: Point::new(origin.x + screen.x, origin.y + screen.y),
            flow,
        }));
    };
    FlowEvents::new()
        .on_before_change(move |mut set| {
            let (nodes, edges) = removals(&mut set);
            remove_selection(store, nodes, edges);
            set
        })
        .on_nodes_change(move |changes| observe_changes(canvas, gestures, &changes))
        .on_connect_end(move |made| {
            if made.is_none() {
                say_refusal(store, gestures);
            }
        })
        .on_viewport_change(move |_| close_menus())
        .on_connect(move |connection| connect(store, &connection))
        .on_node_drag_stop(move |ids| persist_positions(store, canvas, &ids))
        .on_selection_change(move |change| select(store, &change.nodes))
        .on_pane_click(move |_| close_menus())
        .on_pane_double_click(move |at| store.open_palette_at(at.flow.x as f32, at.flow.y as f32))
        .on_pane_context_menu(move |at| shown(menu::Target::Pane, at.flow))
        .on_node_context_menu(move |at| shown(menu::Target::Node(at.node.to_string()), at.flow))
        .on_edge_context_menu(move |at| shown(menu::Target::Edge(at.edge.to_string()), at.flow))
}

#[derive(Clone, Copy)]
struct Clipboard {
    held: StoredValue<Option<graph::Clipboard>, LocalStorage>,
    pastes: StoredValue<u32, LocalStorage>,
}

fn copy(store: Store, canvas: Canvas, clipboard: Clipboard) -> bool {
    let ids: Vec<String> = canvas
        .selected_nodes()
        .iter()
        .map(ToString::to_string)
        .collect();
    let Some(copied) = graph::copy_nodes(&store.graph.get_untracked(), &ids) else {
        return false;
    };
    let count = copied.nodes.len();
    clipboard.held.set_value(Some(copied));
    clipboard.pastes.set_value(0);
    store.note(if count == 1 {
        String::from("Copied 1 node")
    } else {
        format!("Copied {count} nodes")
    });
    true
}

fn paste(store: Store, clipboard: Clipboard, gestures: Gestures) -> bool {
    let Some(held) = clipboard.held.get_value() else {
        return false;
    };
    let current = store.graph.get_untracked();
    if let Some(reason) = graph::paste_refusal(&current, &held) {
        store.say(reason);
        return true;
    }
    let pastes = clipboard.pastes.get_value() + 1;
    clipboard.pastes.set_value(pastes);
    let step = graph::PASTE_OFFSET_PX * pastes as f32;
    let (next, ids) = graph::paste_nodes(&current, &held, Position { x: step, y: step });
    store.edit_graph(move |graph| *graph = next);
    store.selected.set(ids.first().cloned());
    gestures
        .selecting
        .set_value(ids.iter().map(|id| id.as_str().into()).collect());
    true
}

fn chord(key: &Key, held: Modifiers) -> Option<char> {
    if !(held.control() || held.meta()) || held.alt() || held.shift() {
        return None;
    }
    key.as_str()
        .and_then(|text| text.chars().next())
        .map(|letter| letter.to_ascii_lowercase())
        .filter(|letter| matches!(letter, 'c' | 'v'))
}

fn follow_zoom(canvas: Canvas) -> RenderEffect<()> {
    RenderEffect::new(move |_| {
        let step = grid_step(canvas.view().viewport().get().zoom);
        let wanted = Some(SnapGrid::square(step));
        if canvas
            .config()
            .with_untracked(|config| config.snap.grid != wanted)
        {
            canvas.config().update(|config| config.snap.grid = wanted);
        }
    })
}

fn refit_on_switch(store: Store, canvas: Canvas) -> RenderEffect<Option<i64>> {
    let owed = StoredValue::new_local(false);
    RenderEffect::new(move |shown: Option<Option<i64>>| {
        let workspace = store.workspace.get();
        if shown.is_some_and(|before| before != workspace) {
            owed.set_value(true);
        }
        let view = canvas.view();
        view.with_geometry(|_| ());
        let measured = canvas.with_graph(|graph| {
            graph.node_count() > 0 && graph.nodes().all(|node| node.measured.is_some())
        });
        if owed.get_value() && measured {
            owed.set_value(false);
            view.fit_view();
        }
        workspace
    })
}

fn follow_selection(store: Store, canvas: Canvas) -> RenderEffect<()> {
    RenderEffect::new(move |_| {
        let wanted = store.selected.get();
        let held = canvas.selected_nodes();
        let same = match &wanted {
            Some(id) => held.iter().any(|shown| shown.as_str() == id),
            None => held.is_empty(),
        };
        if !same {
            let ids: Vec<NodeId> = wanted.iter().map(|id| id.as_str().into()).collect();
            canvas.select(&ids, &[]);
        }
    })
}

pub fn pane(store: Store, canvas: Canvas) -> impl IntoView {
    let gestures = Gestures {
        resized: StoredValue::new_local(HashSet::new()),
        pending: StoredValue::new_local(None),
        selecting: StoredValue::new_local(Vec::new()),
    };
    let remembering = remember_pending(canvas, gestures);
    on_cleanup_local(move || drop(remembering));
    let syncing = RenderEffect::new(move |_| {
        let graph = store.graph.get();
        store.state.with(|_| ());
        store.channel_types.with(|_| ());
        sync(store, canvas, &graph);
        select_pasted(canvas, gestures);
    });
    on_cleanup_local(move || drop(syncing));
    let following = follow_selection(store, canvas);
    on_cleanup_local(move || drop(following));
    let refitting = refit_on_switch(store, canvas);
    on_cleanup_local(move || drop(refitting));
    let zooming = follow_zoom(canvas);
    on_cleanup_local(move || drop(zooming));
    let gap = Signal::derive_local(move || grid_step(canvas.view().viewport().get().zoom));
    let resizes = finish_resizes(store, canvas, gestures);
    on_cleanup_local(move || drop(resizes));

    let open = RwSignal::new(None::<menu::Menu>);
    let clipboard = Clipboard {
        held: StoredValue::new_local(None),
        pastes: StoredValue::new_local(0),
    };
    canvas.set_connection_validator(move |_, connection| {
        refusal(store, &store.graph.get_untracked(), connection).is_none()
    });
    let types = NodeTypes::new().fallback(move |props: NodeProps<(), ()>| card(store, props));
    let keys = move |ev: &mut EventCx<'_, events::KeyDown>| {
        if focus_is_text_entry() {
            return;
        }
        let done = match chord(&ev.key_without_modifiers, ev.modifiers) {
            Some('c') => copy(store, canvas, clipboard),
            Some('v') => paste(store, clipboard, gestures),
            _ => false,
        };
        if done {
            ev.prevent_default();
        }
    };
    view! {
        box(
            class = "patch",
            on:key_down = keys,
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                crate::ui::presence::pointer_moved(store, canvas.view(), ev.position);
            },
            on:pointer_leave = move |_| crate::ui::presence::pointer_left(store)
        ) {
            Flow(
                handle = canvas,
                events = events(store, canvas, open, gestures),
                node_types = types,
                fit_view = true
            ) {
                Background(gap = gap)
                Controls(show_lock = false)
                MiniMap()
            }
            {crate::ui::presence::layer(store, canvas.view())}
            {menu::view(store, canvas, open)}
        }
    }
}

const AUDIO_FACE_W: f32 = 460.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct CardSize {
    width: Option<f32>,
    min_width: Option<f32>,
    max_width: Option<f32>,
    min_height: f32,
}

fn card_size(store: Store, node: &PatchNode, places: &[Place]) -> CardSize {
    let kind = node.body.kind();
    let inputs = places
        .iter()
        .filter(|place| place.direction == PortDirection::In)
        .count();
    let (_, min_height) = graph::min_size(kind, inputs, places.len() - inputs);
    let audio = match &node.body {
        NodeBody::Channel(channel) => store
            .descriptor_of(&channel.channel_type)
            .is_some_and(|descriptor| descriptor.has_audio),
        _ => false,
    };
    sized(kind, audio, min_height)
}

fn sized(kind: &str, audio: bool, min_height: f32) -> CardSize {
    if audio {
        return CardSize {
            width: Some(AUDIO_FACE_W),
            min_width: None,
            max_width: None,
            min_height,
        };
    }
    let fit = graph::fit_width(kind);
    CardSize {
        width: None,
        min_width: fit.map(|(min, _)| min),
        max_width: fit.map(|(_, max)| max),
        min_height,
    }
}

fn resizer(node: &PatchNode, places: &[Place]) -> Option<AnyView> {
    let kind = node.body.kind();
    if !graph::is_resizable(kind) {
        return None;
    }
    let inputs = places
        .iter()
        .filter(|place| place.direction == PortDirection::In)
        .count();
    let (w, h) = graph::min_size(kind, inputs, places.len() - inputs);
    let floor = Size::new(f64::from(w), f64::from(h));
    Some(AnyView::new(view! { NodeResizer(min_size = floor) }))
}

fn card(store: Store, props: NodeProps<(), ()>) -> impl IntoView {
    let id = props.id.to_string();
    let node = {
        let id = id.clone();
        Memo::new(move |_| store.graph.with(|graph| graph.node(&id).cloned()))
    };
    let shape = Memo::new(move |_| node.with(|node| node.as_ref().map(shape_of)));
    let places = Signal::derive(move || {
        node.get()
            .map(|node| places_of(store, &node))
            .unwrap_or_default()
    });
    let face = move || {
        shape.get()?;
        let node = node.get_untracked()?;
        Some(faces::face(store, &node))
    };
    let sized = Memo::new(move |_| {
        node.with(|node| {
            node.as_ref()
                .map(|node| card_size(store, node, &places.get()))
        })
    });
    let px = |value: Option<f32>| value.map(|value| format!("{value:.0}px"));
    let width = move || px(sized.get().and_then(|size| size.width));
    let min_width = move || px(sized.get().and_then(|size| size.min_width));
    let max_width = move || px(sized.get().and_then(|size| size.max_width));
    let min_height = move || px(sized.get().map(|size| size.min_height));
    let category = node.with_untracked(|node| {
        node.as_ref()
            .map_or("", |node| node::category_class(node.body.category()))
    });
    let resize = node
        .get_untracked()
        .and_then(|node| resizer(&node, &places.get_untracked()));
    let remove = {
        let id = id.clone();
        move || remove_selection(store, vec![id.clone()], Vec::new())
    };
    let chosen = id.clone();
    let alert = face_alert(store, id.clone());
    view! {
        column(
            on:pointer_down:capture = move |_| {
                if store.selected.get_untracked().as_deref() != Some(chosen.as_str()) {
                    store.selected.set(Some(chosen.clone()));
                }
            },
            class = "node",
            attr:data-category = category,
            style:width = width,
            style:min-width = min_width,
            style:max-width = max_width,
            style:min-height = min_height
        ) {
            DragHandle(class = "node__grip") {
                {header::header(store, id, remove)}
            }
            {alert}
            column(class = "node__body") {
                {face}
            }
            {ports::ports(places)}
            {resize}
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::patch::{ChannelNode, DeviceNode, PortType};

    use super::*;

    fn node(id: &str, body: NodeBody) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn channel(kind: &str) -> NodeBody {
        NodeBody::Channel(ChannelNode {
            channel_type: kind.to_owned(),
            tuning_locked: false,
        })
    }

    fn connection(source: &str, source_port: &str, target: &str, target_port: &str) -> Connection {
        Connection {
            source: source.into(),
            source_handle: Some(source_port.into()),
            target: target.into(),
            target_handle: Some(target_port.into()),
        }
    }

    #[test]
    fn a_channel_that_changes_decoder_gets_a_face_of_its_own() {
        assert_ne!(
            shape_of(&node("ch", channel("nfm"))),
            shape_of(&node("ch", channel("wfm")))
        );
        assert_ne!(
            shape_of(&node("ch", channel("nfm"))),
            shape_of(&node("ch", NodeBody::Scope))
        );
    }

    #[test]
    fn a_moved_node_keeps_its_face() {
        let mut moved = node("device", NodeBody::Device(DeviceNode::default()));
        let before = shape_of(&moved);
        moved.position = Position { x: 400.0, y: 90.0 };
        assert_eq!(shape_of(&moved), before);
    }

    #[test]
    fn a_connection_becomes_the_edge_between_its_ports() {
        let edge = edge_of(&connection("device", "iq", "scope", "iq")).expect("an edge");
        assert_eq!(edge.from.node, "device");
        assert_eq!(edge.from.port, "iq");
        assert_eq!(edge.to.node, "scope");
        assert_eq!(edge.to.port, "iq");
        let mut loose = connection("device", "iq", "scope", "iq");
        loose.target_handle = None;
        assert_eq!(edge_of(&loose), None);
    }

    #[test]
    fn a_deletion_is_taken_out_of_the_change_and_reported() {
        let mut set: ChangeSet<(), ()> = ChangeSet {
            nodes: vec![
                NodeChange::Remove("scope".into()),
                NodeChange::Select {
                    id: "scope".into(),
                    selected: false,
                },
            ],
            edges: vec![EdgeChange::Remove("wire".into())],
        };
        let (nodes, edges) = removals(&mut set);
        assert_eq!(nodes, ["scope"]);
        assert_eq!(edges, ["wire"]);
        assert_eq!(set.nodes.len(), 1);
        assert!(set.edges.is_empty());
    }

    #[test]
    fn only_a_plain_command_c_or_v_is_a_chord() {
        let key = |text: &str| Key::Character(text.into());
        assert_eq!(chord(&key("c"), Modifiers::CONTROL), Some('c'));
        assert_eq!(chord(&key("V"), Modifiers::META), Some('v'));
        assert_eq!(chord(&key("c"), Modifiers::NONE), None);
        assert_eq!(
            chord(&key("c"), Modifiers::CONTROL | Modifiers::SHIFT),
            None
        );
        assert_eq!(chord(&key("x"), Modifiers::CONTROL), None);
    }

    #[test]
    fn an_audio_channel_gets_the_wider_face_and_others_fit_their_content() {
        assert_eq!(sized("channel", true, 80.0).width, Some(AUDIO_FACE_W));
        let decoder = sized("channel", false, 80.0);
        assert_eq!(
            (decoder.min_width, decoder.max_width),
            (Some(420.0), Some(440.0))
        );
        let scope = sized("scope", false, 80.0);
        assert_eq!((scope.width, scope.min_width), (None, None));
        assert_eq!(scope.min_height, 80.0);
    }

    #[test]
    fn the_grid_coarsens_zoomed_out_and_refines_zoomed_in() {
        assert_eq!(grid_step(0.3), 48.0);
        assert_eq!(grid_step(0.5), 24.0);
        assert_eq!(grid_step(1.0), 24.0);
        assert_eq!(grid_step(1.25), 12.0);
        assert_eq!(grid_step(2.0), 12.0);
    }

    #[test]
    fn every_port_type_has_a_wire_colour() {
        let all = [
            PortType::Iq,
            PortType::Baseband,
            PortType::Audio,
            PortType::Events,
            PortType::Video,
            PortType::Control,
            PortType::Position,
            PortType::Tx,
            PortType::Array,
        ];
        for port in all {
            let rule = format!(".zf-edge.{} {{ --wire:", port.as_str());
            assert!(crate::theme::SHEET.contains(&rule), "{port:?}");
        }
    }
}
