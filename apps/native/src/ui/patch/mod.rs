pub mod graph;
mod header;
mod menu;
mod ports;
mod remove;
mod sync;

use std::collections::{HashMap, HashSet};

use sdrmm_wire::patch::{
    NodeBody, PatchEdge, PatchGraph, PatchNode, PortBacking, PortDirection, PortRef, Position,
};
use zgui::{prelude::*, reactive::RenderEffect};
use zgui_flow::prelude::{
    BackgroundProps, ChangeSet, CommitOptions, Connection, ControlsProps, DragHandleProps,
    EdgeChange, FlowAction, FlowConfig, FlowEvents, FlowHandle, FlowProps, Graph, Interaction,
    KeyMap, MiniMapProps, NodeChange, NodeId, NodeProps, NodeResizerProps, NodeTypes, Point, Size,
    SnapGrid, ZoomLimits,
};

use self::sync::Held;
use crate::{
    store::Store,
    ui::{
        faces,
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
    if connection.source == connection.target {
        return Some(String::from("a node cannot wire to itself"));
    }
    let Some(edge) = edge_of(connection) else {
        return Some(String::from("pick a port"));
    };
    if graph.edges.contains(&edge) {
        return Some(String::from("already wired"));
    }
    let mut tentative = graph.clone();
    tentative.edges.push(edge);
    let descriptors = store.channel_types.get_untracked();
    tentative
        .validate_against(&descriptors)
        .err()
        .map(|error| error.to_string())
}

pub fn remove_selection(store: Store, nodes: Vec<String>, edges: Vec<String>) {
    if nodes.is_empty() && edges.is_empty() {
        return;
    }
    let graph = store.graph.get_untracked();
    let closing = remove::teardowns(store, &graph, &nodes);
    zgui::task::spawn_local(async move {
        remove::close(store, closing).await;
        store.edit_graph(move |graph| {
            *graph = graph::remove_edges(&graph::remove_nodes(graph, &nodes), &edges);
        });
    });
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
    config.keys = KeyMap::default()
        .unbind(FlowAction::Undo)
        .unbind(FlowAction::Redo);
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
    let drawn: Vec<sync::Drawn> = graph.nodes.iter().map(sync::drawn).collect();
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
}

fn observe_changes(store: Store, canvas: Canvas, gestures: Gestures, changes: &[NodeChange<()>]) {
    match canvas.view().interaction().get_untracked() {
        Interaction::Resizing => gestures.resized.update_value(|held| {
            held.extend(changes.iter().filter_map(|change| match change {
                NodeChange::Size { id, .. } => Some(id.to_string()),
                _ => None,
            }));
        }),
        Interaction::Idle => {
            let nudged: Vec<NodeId> = changes
                .iter()
                .filter_map(|change| match change {
                    NodeChange::Position { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect();
            persist_positions(store, canvas, &nudged);
        }
        _ => {}
    }
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

fn connect(store: Store, connection: &Connection) {
    if let Some(reason) = refusal(store, &store.graph.get_untracked(), connection) {
        store.say(reason);
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
        .on_nodes_change(move |changes| observe_changes(store, canvas, gestures, &changes))
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
    store.say(if count == 1 {
        String::from("Copied 1 node")
    } else {
        format!("Copied {count} nodes")
    });
    true
}

fn paste(store: Store, clipboard: Clipboard) -> bool {
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

fn follow_selection(store: Store, canvas: Canvas) -> RenderEffect<()> {
    RenderEffect::new(move |_| {
        let wanted = store.selected.get();
        let held = canvas.selected_nodes();
        let same = match &wanted {
            Some(id) => held.len() == 1 && held[0].as_str() == id,
            None => held.is_empty(),
        };
        if !same {
            let ids: Vec<NodeId> = wanted.iter().map(|id| id.as_str().into()).collect();
            canvas.select(&ids, &[]);
        }
    })
}

pub fn pane(store: Store, canvas: Canvas) -> impl IntoView {
    let syncing = RenderEffect::new(move |_| {
        let graph = store.graph.get();
        store.state.with(|_| ());
        store.channel_types.with(|_| ());
        sync(store, canvas, &graph);
    });
    on_cleanup_local(move || drop(syncing));
    let following = follow_selection(store, canvas);
    on_cleanup_local(move || drop(following));
    let zooming = follow_zoom(canvas);
    on_cleanup_local(move || drop(zooming));
    let gap = Signal::derive_local(move || grid_step(canvas.view().viewport().get().zoom));
    let gestures = Gestures {
        resized: StoredValue::new_local(HashSet::new()),
    };
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
            Some('v') => paste(store, clipboard),
            _ => false,
        };
        if done {
            ev.prevent_default();
        }
    };
    view! {
        box(class = "patch", on:key_down = keys) {
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
            {menu::view(store, canvas, open)}
        }
    }
}

fn node_width(node: &PatchNode) -> f32 {
    node.size
        .map_or(graph::natural_size(node.body.kind()).w, |size| size.w)
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
    let width = move || {
        node.with(|node| {
            node.as_ref()
                .filter(|node| !graph::is_resizable(node.body.kind()))
                .map(|node| format!("{:.0}px", node_width(node)))
        })
    };
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
    view! {
        column(class = "node", attr:data-category = category, style:width = width) {
            DragHandle(class = "node__grip") {
                {header::header(store, id, remove)}
            }
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
