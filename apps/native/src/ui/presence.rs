use std::time::Duration;

use sdrmm_wire::{
    patch::Position,
    presence::{DraggedNode, MAX_PEER_NAME_LEN, Peer, Pointer},
};
use zgui::{
    geom::{Css, CssPx},
    prelude::*,
};
use zgui_flow::prelude::{FlowView, NodeId, Point, Rect};

use crate::{
    presence::{Person, initial_of, peer_color, pointer_ms},
    store::Store,
    ui::{
        kit_shell::{Entry, button, entry, hover},
        shell::{Menu, Shell},
    },
};

const STACKED: usize = 3;
const OUTLINE_GAP: f64 = 4.0;
const CURSOR: &str = "M1 1 L1 15 L5 11 L8 17 L10 16 L7 10 L13 10 Z";

const SHEET: &str = css!(
    r#"
.pr { display: flex; align-items: center; height: 24px; padding: 0 4px; border-radius: 12px; }
.pr:hover, .pr.on { background-color: var(--panel-3); }
.pr__more { margin-left: 4px; font-family: var(--mono); font-size: 10px; color: var(--ink-dim); }
.pr__avatar { display: flex; align-items: center; justify-content: center; width: 18px; height: 18px; border-radius: 9px; border: 2px solid var(--panel); font-size: 9px; font-weight: 600; color: var(--bg); flex: 0 0 auto; }
.pr__avatar.stacked { margin-left: -6px; }
.pr__avatar.first { margin-left: 0; }
.pm {
    position: absolute;
    right: 120px;
    top: 42px;
    z-index: 150;
    width: 240px;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--line-strong);
    border-radius: 7px;
    background-color: var(--panel);
    box-shadow: 0 18px 44px rgba(0, 0, 0, 0.5);
}
.pm__list { flex-direction: column; gap: 2px; max-height: 240px; overflow: auto; }
.pm__row { align-items: center; gap: 8px; height: 24px; font-size: 12px; color: var(--ink); }
.pm__name { flex: 1 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.pm__note { font-family: var(--mono); font-size: 10px; color: var(--ink-faint); }
.pm__form { align-items: center; gap: 4px; padding-top: 8px; border-top: 1px solid var(--line); }
.peers { position: absolute; left: 0; top: 0; right: 0; bottom: 0; pointer-events: none; overflow: hidden; z-index: 40; }
.peers__outline { position: absolute; border-width: 2px; border-style: solid; border-radius: 6px; pointer-events: none; }
.peers__tag { position: absolute; padding: 0 4px; border-radius: 3px; font-size: 10px; white-space: nowrap; color: var(--bg); pointer-events: none; }
.peers__cursor { position: absolute; width: 16px; height: 18px; pointer-events: none; }
"#
);

fn install() {
    install_stylesheet("shell-presence", SHEET);
}

pub fn start(store: Store, shell: Shell) {
    store.present_as(&shell.prefs().name);
}

fn avatar(person: &Person, stacked: Option<bool>) -> impl IntoView {
    let colour = peer_color(person.hue);
    view! {
        box(
            class = "pr__avatar",
            class:stacked = stacked.is_some(),
            class:first = stacked == Some(true),
            style:background-color = Some(colour)
        ) {
            text() {{initial_of(&person.name)}}
        }
    }
}

pub fn stack(store: Store, shell: Shell) -> impl IntoView {
    install();
    move || {
        let people = store.presence.get().people();
        let me = people.me?;
        let shown: Vec<Person> = if people.others.is_empty() {
            vec![me]
        } else {
            people.others.iter().take(STACKED).cloned().collect()
        };
        let hidden = people.others.len().saturating_sub(shown.len());
        let title = if people.others.is_empty() {
            String::from("Only you")
        } else {
            format!("{} here", people.others.len() + 1)
        };
        let avatars: Vec<AnyView> = shown
            .iter()
            .enumerate()
            .map(|(at, person)| AnyView::new(avatar(person, Some(at == 0))))
            .collect();
        let more = (hidden > 0)
            .then(|| AnyView::new(view! { text(class = "pr__more") {{format!("+{hidden}")}} }));
        let face = view! {
            control(
                class = "pr",
                class:on = move || shell.menu.get() == Some(Menu::People),
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                a11y:label = title.clone(),
                on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                on:click:stop = move |_| shell.toggle_menu(Menu::People)
            ) {
                {avatars}
                {more}
            }
        };
        Some(hover(title, face))
    }
}

fn person_row(person: &Person, you: bool) -> AnyView {
    let tabs = (person.tabs > 1).then(|| {
        AnyView::new(view! { text(class = "pm__note") {{format!("{} tabs", person.tabs)}} })
    });
    let note = you.then(|| AnyView::new(view! { text(class = "pm__note") {"you"} }));
    AnyView::new(view! {
        row(class = "pm__row") {
            {avatar(person, None)}
            text(class = "pm__name") {{person.name.clone()}}
            {tabs}
            {note}
        }
    })
}

pub fn people(store: Store, shell: Shell) -> impl IntoView {
    install();
    let rows = move || {
        let people = store.presence.get().people();
        let mut rows: Vec<AnyView> = people.me.iter().map(|me| person_row(me, true)).collect();
        rows.extend(people.others.iter().map(|person| person_row(person, false)));
        rows
    };
    view! {
        column(
            class = "pm",
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation()
        ) {
            text(class = "legend") {"Here now"}
            column(class = "pm__list") {{rows}}
            {name_form(store, shell)}
        }
    }
}

fn name_form(store: Store, shell: Shell) -> impl IntoView {
    let current = store
        .presence
        .get_untracked()
        .people()
        .me
        .map(|me| me.name)
        .unwrap_or_default();
    let saved = shell.prefs().name;
    let typed = RwSignal::new_local(if saved.is_empty() { current } else { saved });
    let save = move || {
        let wanted: String = typed
            .get_untracked()
            .chars()
            .take(MAX_PEER_NAME_LEN)
            .collect();
        let name = store.present_as(&wanted);
        shell.remember(move |prefs| prefs.name = name);
        shell.menu.set(None);
    };
    view! {
        row(class = "pm__form") {
            {entry(Entry::new(typed, "Your name").focused(), save, move || shell.menu.set(None))}
            {button("Save", "quiet", save)}
        }
    }
}

pub fn pointer_moved(store: Store, view: FlowView, window: zgui::geom::Point<CssPx, Css>) {
    let at = view.window_to_flow(window);
    store.point_at(Some(Position {
        x: at.x as f32,
        y: at.y as f32,
    }));
}

pub fn pointer_left(store: Store) {
    store.point_at(None);
}

fn publish_selection(store: Store, view: FlowView) {
    let dragging: Vec<NodeId> = view.dragging().get();
    let (selected, dragged) = view.with_geometry(|geometry| {
        let mut selected: Vec<String> = geometry
            .summaries
            .iter()
            .filter(|(_, summary)| summary.selected)
            .map(|(id, _)| id.to_string())
            .collect();
        selected.sort();
        let dragged: Vec<DraggedNode> = dragging
            .iter()
            .filter_map(|id| {
                let rect = geometry.summaries.get(id)?.rect;
                Some(DraggedNode {
                    node: id.to_string(),
                    position: Position {
                        x: rect.x0 as f32,
                        y: rect.y0 as f32,
                    },
                })
            })
            .collect();
        (selected, dragged)
    });
    store.point_selection(selected, dragged);
}

pub fn layer(store: Store, view: FlowView) -> impl IntoView {
    install();
    let publishing = zgui::reactive::RenderEffect::new(move |_| publish_selection(store, view));
    let flushing = set_interval(Duration::from_millis(pointer_ms()), move || {
        store.flush_pointer();
    });
    on_cleanup_local(move || {
        drop(publishing);
        drop(flushing);
        store.point_at(None);
    });
    view! {
        box(class = "peers") {
            {move || marks(store, view)}
        }
    }
}

fn marks(store: Store, view: FlowView) -> Vec<AnyView> {
    let room = store.presence.get();
    let dragged = room.dragged_by_others();
    let mut drawn = Vec::new();
    for peer in room.others() {
        let Some(pointer) = room.pointers.get(&peer.id) else {
            continue;
        };
        for node in &pointer.selected {
            if let Some(rect) = node_rect(view, node, dragged.get(node)) {
                drawn.push(outline(view, &peer, rect));
            }
        }
        drawn.extend(cursor(view, &peer, pointer));
    }
    drawn
}

fn node_rect(view: FlowView, node: &str, dragged: Option<&Position>) -> Option<Rect> {
    let id: NodeId = node.into();
    let rect =
        view.with_geometry(|geometry| geometry.summaries.get(&id).map(|summary| summary.rect))?;
    Some(match dragged {
        Some(at) => rect.with_origin(Point::new(f64::from(at.x), f64::from(at.y))),
        None => rect,
    })
}

fn px(value: f64) -> Option<String> {
    Some(format!("{value:.1}px"))
}

fn outline(view: FlowView, peer: &Peer, rect: Rect) -> AnyView {
    let zoom = view.viewport().get().zoom;
    let gap = OUTLINE_GAP * zoom;
    let corner = view.project(Point::new(rect.x0, rect.y0));
    let colour = peer_color(peer.hue);
    let left = corner.x - gap;
    let top = corner.y - gap;
    AnyView::new(view! {
        box(
            class = "peers__outline",
            style:left = px(left),
            style:top = px(top),
            style:width = px(rect.width() * zoom + gap * 2.0),
            style:height = px(rect.height() * zoom + gap * 2.0),
            style:border-color = Some(colour.clone())
        ) {}
        text(
            class = "peers__tag",
            style:left = px(left),
            style:top = px(top - 16.0),
            style:background-color = Some(colour)
        ) {{peer.name.clone()}}
    })
}

fn cursor(view: FlowView, peer: &Peer, pointer: &Pointer) -> Option<AnyView> {
    let at = pointer.at?;
    let spot = view.project(Point::new(f64::from(at.x), f64::from(at.y)));
    let colour = peer_color(peer.hue);
    let arrow = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 18"><path d="{CURSOR}" fill="{colour}" stroke="black" stroke-width="1"/></svg>"#
    );
    Some(AnyView::new(view! {
        box(class = "peers__cursor", style:left = px(spot.x), style:top = px(spot.y)) {
            {zgui::elements::vector().document(&arrow)}
        }
        text(
            class = "peers__tag",
            style:left = px(spot.x + 12.0),
            style:top = px(spot.y + 16.0),
            style:background-color = Some(colour)
        ) {{peer.name.clone()}}
    }))
}
