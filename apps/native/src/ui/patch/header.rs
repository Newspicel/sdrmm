use zgui::{prelude::*, reactive::RenderEffect};

use sdrmm_wire::{patch::PatchNode, workspace::MAX_NAME_LEN};
use zgui_ui::prelude::*;

use crate::{
    head::{NodeHead, Tone},
    shell::rack_grid,
    store::Store,
    ui::{
        kit_shell::{hover, svg},
        node,
    },
};

const MAXIMIZE: &str =
    r#"<path d="M15 3h6v6"/><path d="m21 3-7 7"/><path d="m3 21 7-7"/><path d="M9 21H3v-6"/>"#;
const MINIMIZE: &str =
    r#"<path d="m14 10 7-7"/><path d="M20 10h-6V4"/><path d="m3 21 7-7"/><path d="M4 14h6v6"/>"#;
const PIN: &str = r#"<path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/>"#;
const CROSS: &str = r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#;

const SHEET: &str = css!(
    r#"
.node__dot { width: 6px; height: 6px; border-radius: 50%; flex: 0 0 auto; background-color: var(--ink-faint); }
.node[data-category="source"] .node__dot { background-color: oklch(0.72 0.11 228); }
.node[data-category="channel"] .node__dot { background-color: oklch(0.74 0.12 158); }
.node[data-category="tool"] .node__dot { background-color: oklch(0.78 0.11 85); }
.node[data-category="output"] .node__dot { background-color: oklch(0.76 0.1 300); }

.node__title { cursor: text; }
.node__rename { width: 150px; height: 20px; font-size: 11px; }
.node__badge { flex: 0 0 auto; font-family: var(--mono); font-size: 10px; color: var(--ink-faint); }
.node__subtitle {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 10.5px;
    color: var(--ink-faint);
}
.node__subtitle.warn { color: oklch(0.8 0.13 80); }
.node__subtitle.danger { color: var(--danger); }

.node__buttons { align-items: center; gap: 2px; flex: 0 0 auto; cursor: default; }

.node__btn {
    width: 18px;
    height: 18px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--ink-faint);
}

.node__btn:hover { background-color: var(--panel-3); color: var(--ink); }
.node__btn.on { background-color: color-mix(in oklab, var(--accent) 15%, transparent); color: var(--accent); }
.node__btn.danger:hover { color: var(--danger); }
.node__btn-icon { width: 12px; height: 12px; pointer-events: none; }
"#
);

fn icon(body: &'static str) -> impl IntoView {
    zgui::elements::vector()
        .class("node__btn-icon")
        .document(&svg(body))
}

fn button(
    body: &'static str,
    label: Signal<String>,
    on: Signal<bool>,
    danger: bool,
    press: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        control(
            class = "node__btn",
            class:on = on,
            class:danger = danger,
            a11y:role = Role::Button,
            a11y:label = move || zgui::vocab::SharedString::from(label.get()),
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
            on:click:stop = move |_| press()
        ) {
            {icon(body)}
        }
    }
}

#[must_use]
pub fn default_title(node: &PatchNode) -> String {
    node::title_of(&PatchNode {
        label: None,
        ..node.clone()
    })
}

#[must_use]
pub fn node_label(typed: &str, title: &str) -> Option<String> {
    let trimmed: String = typed.trim().chars().take(MAX_NAME_LEN).collect();
    let trimmed = trimmed.trim_end();
    (!trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(title)).then(|| trimmed.to_owned())
}

fn rename(store: Store, id: &str, typed: &str) {
    let Some(node) = store.graph.get_untracked().node(id).cloned() else {
        return;
    };
    let label = node_label(typed, &default_title(&node));
    if label == node.label {
        return;
    }
    let id = id.to_owned();
    store.edit_graph_quiet(move |graph| {
        if let Some(found) = graph.nodes.iter_mut().find(|found| found.id == id) {
            found.label = label;
        }
    });
}

fn name(store: Store, id: String) -> impl IntoView {
    let editing = RwSignal::new_local(false);
    let draft = RwSignal::new_local(String::new());
    let title = {
        let id = id.clone();
        move || {
            store
                .graph
                .get()
                .node(&id)
                .map(node::title_of)
                .unwrap_or_default()
        }
    };
    let start = {
        let title = title.clone();
        move || {
            draft.set(title());
            editing.set(true);
        }
    };
    let finish = move |keep: bool| {
        if editing.get_untracked() {
            editing.set(false);
            if keep {
                rename(store, &id, &draft.get_untracked());
            }
        }
    };
    move || {
        if editing.get() {
            let finish = finish.clone();
            let blur = finish.clone();
            let field = NodeRef::new();
            let focus = RenderEffect::new(move |_| {
                if field.get().is_some() {
                    field.focus();
                }
            });
            on_cleanup_local(move || drop(focus));
            AnyView::new(view! {
                Input(
                    value = draft,
                    class = "node__rename",
                    label = "Node name",
                    node_ref = field,
                    on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                    on:focus_out = move |_| blur(true),
                    on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                        ev.stop_propagation();
                        match ev.key {
                            Key::Named(NamedKey::Enter) => { finish(true); ev.prevent_default(); }
                            Key::Named(NamedKey::Escape) => { finish(false); ev.prevent_default(); }
                            _ => {}
                        }
                    },
                )
            })
        } else {
            let start = start.clone();
            let keyed = start.clone();
            let title = title.clone();
            hover("Double-click to rename", view! {
                control(
                    class = "node__title",
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:description = "Double-click to rename",
                    on:double_click = move |_| start(),
                    on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                        if matches!(ev.key, Key::Named(NamedKey::F2 | NamedKey::Enter)) {
                            ev.prevent_default();
                            ev.stop_propagation();
                            keyed();
                        }
                    }
                ) {{title}}
            })
        }
    }
}

fn head_view(store: Store, id: String) -> impl IntoView {
    let head = {
        let id = id.clone();
        Memo::new(move |_| store.heads.with(|heads| heads.get(&id).cloned().unwrap_or_default()))
    };
    view! {
        text(class = "node__badge", hidden = move || head.with(|head| head.badge.is_none())) {
            {move || head.get().badge.unwrap_or_default()}
        }
        {move || subtitle_view(head)}
    }
}

fn subtitle_view(head: Memo<NodeHead>) -> Option<AnyView> {
    let shown = head.get();
    let said = shown.subtitle?;
    let text = AnyView::new(view! {
        text(
            class = "node__subtitle",
            class:warn = shown.tone == Tone::Warn,
            class:danger = shown.tone == Tone::Danger
        ) {{said}}
    });
    Some(match shown.title {
        Some(title) => hover(title, text),
        None => text,
    })
}

pub fn header(store: Store, id: String, remove: impl Fn() + 'static) -> impl IntoView {
    install_stylesheet("node-header", SHEET);
    let full = {
        let id = id.clone();
        Signal::derive(move || store.expanded.get().as_deref() == Some(id.as_str()))
    };
    let pinned = {
        let id = id.clone();
        Signal::derive(move || rack_grid::is_pinned(&store.rack.get(), &id))
    };
    let expand = {
        let id = id.clone();
        move || {
            let next = if full.get_untracked() {
                None
            } else {
                Some(id.clone())
            };
            store.expanded.set(next);
        }
    };
    let pin = {
        let id = id.clone();
        move || store.edit_rack(|rack| rack_grid::toggle_pin(rack, &id))
    };
    let full_label = Signal::derive(move || {
        String::from(if full.get() {
            "Leave full screen"
        } else {
            "Show full screen"
        })
    });
    let pin_label = Signal::derive(move || {
        String::from(if pinned.get() {
            "Unpin from the rack"
        } else {
            "Pin to the rack"
        })
    });
    view! {
        row(class = "node__bar") {
            box(class = "node__dot") {}
            {name(store, id.clone())}
            {head_view(store, id.clone())}
            spacer()
            row(class = "node__buttons") {
                {move || AnyView::new(button(
                    if full.get() { MINIMIZE } else { MAXIMIZE },
                    full_label,
                    full,
                    false,
                    expand.clone(),
                ))}
                {button(PIN, pin_label, pinned, false, pin)}
                {button(CROSS, Signal::stored(String::from("Remove")), Signal::stored(false), true, remove)}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::patch::{NodeBody, Position};

    use super::*;

    fn scope(label: Option<&str>) -> PatchNode {
        PatchNode {
            id: "scope".to_owned(),
            body: NodeBody::Scope,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: label.map(str::to_owned),
        }
    }

    #[test]
    fn an_empty_or_default_name_clears_the_label() {
        let title = default_title(&scope(Some("Tower")));
        assert_eq!(title, "SCOPE");
        assert_eq!(node_label("   ", &title), None);
        assert_eq!(node_label("scope", &title), None);
        assert_eq!(node_label("  Tower  ", &title).as_deref(), Some("Tower"));
    }

    #[test]
    fn a_long_name_is_cut_to_what_the_server_keeps() {
        let long = "x".repeat(MAX_NAME_LEN + 9);
        assert_eq!(
            node_label(&long, "SCOPE").map(|label| label.chars().count()),
            Some(MAX_NAME_LEN)
        );
    }
}
