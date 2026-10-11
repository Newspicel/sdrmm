use sdrmm_wire::patch::NodeBody;
use zgui::{prelude::*, reactive::RenderEffect};
use zgui_flow::prelude::Point;

use super::{Canvas, graph};
use crate::{shell::rack_grid, store::Store, ui::faces::channel::picker};

const SHEET: &str = css!(
    r#"
.canvas-menu {
    position: absolute;
    min-width: 200px;
    padding: 4px;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--line);
    border-radius: 8px;
    background-color: var(--panel);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5);
    pointer-events: auto;
}

.canvas-menu__item {
    padding: 5px 10px;
    border-radius: 5px;
    color: var(--ink-dim);
    font-size: 12px;
}

.canvas-menu__item:hover { background-color: var(--panel-2); color: var(--ink); }
.canvas-menu__item.danger:hover { color: var(--danger); }

.canvas-menu__hint {
    padding: 4px 10px 2px 10px;
    font-size: 10px;
    color: var(--ink-faint);
}
"#
);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Pane,
    Node(String),
    Edge(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub target: Target,
    pub window: Point,
    pub flow: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    AddHere,
    Replace,
    ShowFull,
    LeaveFull,
    Pin,
    Unpin,
    ResetSize,
    DeleteWire,
    Fit,
}

impl Action {
    const fn label(self) -> &'static str {
        match self {
            Self::AddHere => "Add node here",
            Self::Replace => "Replace with\u{2026}",
            Self::ShowFull => "Show full screen",
            Self::LeaveFull => "Leave full screen",
            Self::Pin => "Pin to the rack",
            Self::Unpin => "Unpin from the rack",
            Self::ResetSize => "Reset size",
            Self::DeleteWire => "Delete wire",
            Self::Fit => "Fit the patch on screen",
        }
    }

    const fn danger(self) -> bool {
        matches!(self, Self::DeleteWire)
    }
}

const ITEM_PX: f64 = 28.0;
const MENU_W: f64 = 208.0;
const EDGE_PX: f64 = 4.0;

#[must_use]
pub fn within(start: f64, extent: f64, low: f64, high: f64) -> f64 {
    start.min(high - extent - EDGE_PX).max(low + EDGE_PX)
}

fn node_actions(store: Store, id: &str) -> Vec<Action> {
    let graph = store.graph.get_untracked();
    let Some(node) = graph.node(id) else {
        return Vec::new();
    };
    let mut listed = Vec::new();
    if matches!(node.body, NodeBody::Channel(_)) {
        listed.push(Action::Replace);
    }
    listed.push(if store.expanded.get_untracked().as_deref() == Some(id) {
        Action::LeaveFull
    } else {
        Action::ShowFull
    });
    listed.push(if rack_grid::is_pinned(&store.rack.get_untracked(), id) {
        Action::Unpin
    } else {
        Action::Pin
    });
    if node.size.is_some() && graph::is_resizable(node.body.kind()) {
        listed.push(Action::ResetSize);
    }
    listed
}

fn actions(store: Store, target: &Target) -> Vec<Action> {
    let mut listed = match target {
        Target::Pane => vec![Action::AddHere],
        Target::Edge(_) => vec![Action::DeleteWire],
        Target::Node(id) => node_actions(store, id),
    };
    listed.push(Action::Fit);
    listed
}

fn run(
    store: Store,
    canvas: Canvas,
    menu: &Menu,
    action: Action,
    replacing: RwSignal<Option<String>>,
) {
    match (action, &menu.target) {
        (Action::AddHere, _) => store.open_palette_at(menu.flow.x as f32, menu.flow.y as f32),
        (Action::DeleteWire, Target::Edge(id)) => {
            let key = id.clone();
            store.edit_graph(move |graph| *graph = graph::remove_edges(graph, &[key]));
        }
        (Action::Replace, Target::Node(id)) => replacing.set(Some(id.clone())),
        (Action::ShowFull, Target::Node(id)) => store.expanded.set(Some(id.clone())),
        (Action::LeaveFull, _) => store.expanded.set(None),
        (Action::Pin | Action::Unpin, Target::Node(id)) => {
            let id = id.clone();
            store.edit_rack(move |rack| rack_grid::toggle_pin(rack, &id));
        }
        (Action::ResetSize, Target::Node(id)) => {
            let id = id.clone();
            store.edit_graph_quiet(move |graph| {
                if let Some(node) = graph.nodes.iter_mut().find(|node| node.id == id) {
                    node.size = None;
                }
            });
        }
        (Action::Fit, _) => canvas.view().fit_view(),
        _ => {}
    }
}

fn placed(canvas: Canvas, menu: &Menu, items: usize, hint: bool) -> (f64, f64) {
    let view = canvas.view();
    let origin = view.pane_origin();
    let size = view.pane_size().get_untracked();
    let height = ITEM_PX * items as f64 + if hint { 24.0 } else { 8.0 };
    (
        within(menu.window.x, MENU_W, origin.x, origin.x + size.width),
        within(menu.window.y, height, origin.y, origin.y + size.height),
    )
}

fn replace_view(store: Store, replacing: RwSignal<Option<String>>) -> impl IntoView {
    move || {
        replacing.get().map(|node| {
            AnyView::new(picker::replace_decoder(store, node, move || {
                replacing.set(None)
            }))
        })
    }
}

fn item(
    store: Store,
    canvas: Canvas,
    open: RwSignal<Option<Menu>>,
    replacing: RwSignal<Option<String>>,
    chosen: Menu,
    action: Action,
) -> AnyView {
    AnyView::new(view! {
        control(
            class = "canvas-menu__item",
            class:danger = action.danger(),
            a11y:role = Role::MenuItem,
            on:click = move |_| {
                open.set(None);
                run(store, canvas, &chosen, action, replacing);
            }
        ) {{action.label()}}
    })
}

pub fn view(store: Store, canvas: Canvas, open: RwSignal<Option<Menu>>) -> impl IntoView {
    install_stylesheet("canvas-menu", SHEET);
    let replacing = RwSignal::new(None::<String>);
    let menu = move || {
        open.get().map(|menu| {
            let hint = matches!(menu.target, Target::Node(_));
            let listed = actions(store, &menu.target);
            let (left, top) = placed(canvas, &menu, listed.len(), hint);
            let chosen = menu.clone();
            let items = move || -> Vec<AnyView> {
                listed
                    .iter()
                    .map(|action| item(store, canvas, open, replacing, chosen.clone(), *action))
                    .collect()
            };
            let field = NodeRef::new();
            let focus = RenderEffect::new(move |_| {
                if field.get().is_some() {
                    field.focus();
                }
            });
            on_cleanup_local(move || drop(focus));
            AnyView::new(view! {
                Portal(layer = OverlayLayer::Popover) {
                    box(
                        class = "canvas-dismiss",
                        style = Some(String::from("position: absolute; inset: 0; pointer-events: auto;")),
                        on:pointer_down = move |_: &mut EventCx<'_, events::PointerDown>| open.set(None)
                    )
                    column(
                        node_ref = field,
                        class = "canvas-menu",
                        tabindex = Focus::Programmatic,
                        a11y:role = Role::Menu,
                        style:left = Some(format!("{left:.0}px")),
                        style:top = Some(format!("{top:.0}px")),
                        on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                        on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                            if matches!(ev.key, Key::Named(NamedKey::Escape)) {
                                ev.prevent_default();
                                ev.stop_propagation();
                                open.set(None);
                            }
                        }
                    ) {
                        {items()}
                        {hint.then(|| AnyView::new(view! { text(class = "canvas-menu__hint") {"Backspace deletes the selection: a node or a wire."} }))}
                    }
                }
            })
        })
    };
    view! {
        {menu}
        {replace_view(store, replacing)}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Action; 9] = [
        Action::AddHere,
        Action::Replace,
        Action::ShowFull,
        Action::LeaveFull,
        Action::Pin,
        Action::Unpin,
        Action::ResetSize,
        Action::DeleteWire,
        Action::Fit,
    ];

    #[test]
    fn every_action_has_a_short_label_without_dashes() {
        for action in ALL {
            let label = action.label();
            assert!(!label.is_empty() && label.len() < 32);
            assert!(!label.contains('\u{2014}'));
        }
    }

    #[test]
    fn only_deleting_a_wire_is_dangerous() {
        for action in ALL {
            assert_eq!(action.danger(), action == Action::DeleteWire);
        }
    }

    #[test]
    fn the_menu_stays_inside_the_pane() {
        assert_eq!(within(50.0, 208.0, 0.0, 1000.0), 50.0);
        assert_eq!(within(900.0, 208.0, 0.0, 1000.0), 788.0);
        assert_eq!(within(-20.0, 208.0, 0.0, 1000.0), 4.0);
    }
}
