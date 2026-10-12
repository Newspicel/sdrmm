use sdrmm_wire::workspace::{MAX_NAME_LEN, WorkspaceInfo};
use zgui::prelude::*;

use crate::{
    shell::workspace_file::{export_file_name, parse_export},
    store::Store,
    ui::{
        files,
        kit_shell::{self, Entry, button, entry, glyph, hover, titled_action, titled_button},
        shell::Shell,
    },
};

const SHEET: &str = css!(
    r#"
.wm {
    position: absolute;
    left: 58px;
    top: 42px;
    z-index: 150;
    width: 330px;
    flex-direction: column;
    gap: 4px;
    padding: 10px;
    border: 1px solid var(--line-strong);
    border-radius: 7px;
    background-color: var(--panel);
    box-shadow: 0 18px 44px rgba(0, 0, 0, 0.5);
}
.wm__row { align-items: center; gap: 4px; min-height: 28px; }
.wm__name { flex: 1 1 auto; min-width: 0; padding: 4px 8px; border-radius: 4px; font-family: var(--mono); font-size: 12px; color: var(--ink-dim); overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.wm__name:hover { background-color: var(--panel-2); color: var(--ink); }
.wm__name.on { background-color: color-mix(in oklab, var(--accent) 16%, transparent); color: var(--accent); }
.wm__count { width: 18px; text-align: right; font-family: var(--mono); font-size: 10px; color: var(--ink-faint); }
.wm__confirm { align-items: center; gap: 6px; min-height: 28px; padding: 0 8px; border: 1px solid var(--danger); border-radius: 4px; background-color: color-mix(in oklab, var(--danger) 10%, transparent); }
.wm__confirm_text { flex: 1 1 auto; min-width: 0; font-size: 12px; color: var(--danger); overflow: hidden; white-space: nowrap; }
.wm__foot { flex-direction: column; gap: 6px; margin-top: 4px; padding-top: 8px; border-top: 1px solid var(--line); }
.wm__add { align-items: center; gap: 6px; }
.wm__switch { border-color: var(--accent); background-color: color-mix(in oklab, var(--accent) 10%, transparent); }
.wm__switch .wm__confirm_text { color: var(--ink); }
"#
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Idle,
    Rename,
    Confirm,
    Switch,
}

pub fn menu(store: Store, shell: Shell) -> impl IntoView {
    install_stylesheet("shell-workspaces", SHEET);
    kit_shell::install();
    view! {
        column(
            class = "wm",
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation()
        ) {
            text(class = "legend") {"Workspaces"}
            for info in move || store.workspaces.get().to_vec(), key = |info: &WorkspaceInfo| info.id {
                {workspace_row(store, shell, info.id)}
            }
            {footer(store, shell)}
        }
    }
}

fn live(store: Store, id: i64) -> Option<WorkspaceInfo> {
    store
        .workspaces
        .get()
        .iter()
        .find(|found| found.id == id)
        .cloned()
}

fn workspace_row(store: Store, shell: Shell, id: i64) -> impl IntoView {
    let mode = RwSignal::new(Mode::Idle);
    let name = move || live(store, id).map(|info| info.name).unwrap_or_default();
    move || match mode.get() {
        Mode::Rename => AnyView::new(rename_row(store, id, mode)),
        Mode::Confirm => AnyView::new(confirm_row(store, id, mode, name())),
        Mode::Switch => AnyView::new(switch_row(store, shell, id, mode)),
        Mode::Idle => AnyView::new(idle_row(store, shell, id, mode)),
    }
}

fn idle_row(store: Store, shell: Shell, id: i64, mode: RwSignal<Mode>) -> impl IntoView {
    let name = move || live(store, id).map(|info| info.name).unwrap_or_default();
    let nodes = move || live(store, id).map_or(0, |info| info.nodes).to_string();
    view! {
        row(class = "wm__row") {
            control(
                class = "wm__name",
                class:on = move || store.workspace.get() == Some(id),
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                on:click:stop = move |_| pick(store, shell, id, mode)
            ) {
                {name}
            }
            {hover(format!("{} nodes", nodes()), view! { text(class = "wm__count") {{nodes}} })}
            {titled_action(glyph::PENCIL, "Rename", "Rename this workspace", false, move || mode.set(Mode::Rename))}
            {titled_action(glyph::COPY, "Duplicate", "Copy this workspace, its patch and the tuning it was left on", false, move || store.duplicate(id))}
            {titled_action(glyph::DOWNLOAD, "Export", "Save this workspace as a file", false, move || export(store, id))}
            {titled_action(glyph::CROSS, "Delete", "Delete this workspace", true, move || mode.set(Mode::Confirm))}
        }
    }
}

fn pick(store: Store, shell: Shell, id: i64, mode: RwSignal<Mode>) {
    if store.workspace.get_untracked() == Some(id) {
        return;
    }
    if store.presence.get_untracked().people().others.is_empty() {
        shell.menu.set(None);
        store.activate(id);
    } else {
        mode.set(Mode::Switch);
    }
}

fn switch_row(store: Store, shell: Shell, id: i64, mode: RwSignal<Mode>) -> impl IntoView {
    let others = store.presence.get_untracked().people().others.len();
    view! {
        row(class = "wm__confirm wm__switch") {
            text(class = "wm__confirm_text") {{switch_question(others)}}
            {titled_button("Switch", "sm", "One workspace runs per server", move || {
                mode.set(Mode::Idle);
                shell.menu.set(None);
                store.activate(id);
            })}
            {button("Stay", "sm", move || mode.set(Mode::Idle))}
        }
    }
}

#[must_use]
pub fn switch_question(others: usize) -> String {
    if others == 1 {
        String::from("Switch 1 other too?")
    } else {
        format!("Switch {others} others too?")
    }
}

fn rename_row(store: Store, id: i64, mode: RwSignal<Mode>) -> impl IntoView {
    let original = live(store, id).map(|info| info.name).unwrap_or_default();
    let draft = RwSignal::new_local(original.clone());
    let commit = move || {
        let next: String = draft
            .get_untracked()
            .trim()
            .chars()
            .take(MAX_NAME_LEN)
            .collect();
        mode.set(Mode::Idle);
        if !next.is_empty() && next != original {
            store.rename(id, next);
        }
    };
    view! {
        row(class = "wm__row") {
            {entry(Entry::new(draft, "Name").focused(), commit, move || mode.set(Mode::Idle))}
        }
    }
}

fn confirm_row(store: Store, id: i64, mode: RwSignal<Mode>, name: String) -> impl IntoView {
    view! {
        row(class = "wm__confirm") {
            text(class = "wm__confirm_text") {{format!("Delete {name}?")}}
            {titled_button("Delete", "danger", "The workspace, its layout and its history are removed", move || {
                mode.set(Mode::Idle);
                store.remove(id);
            })}
            {button("Keep", "sm", move || mode.set(Mode::Idle))}
        }
    }
}

fn footer(store: Store, shell: Shell) -> impl IntoView {
    let name = RwSignal::new_local(String::new());
    let create = move || {
        let typed = name.get_untracked().trim().to_owned();
        if typed.is_empty() {
            return;
        }
        name.set(String::new());
        shell.menu.set(None);
        store.create(typed.chars().take(MAX_NAME_LEN).collect());
    };
    let enabled = Signal::derive_local(move || !name.get().trim().is_empty());
    view! {
        column(class = "wm__foot") {
            row(class = "wm__add") {
                {entry(Entry::new(name, "New workspace"), create, || {})}
                {kit_shell::gated_button("Add", "quiet", enabled, create)}
            }
            {titled_button("Import a workspace file", "quiet", "Read a workspace file exported here or on another machine", move || import(store, shell))}
        }
    }
}

fn import(store: Store, shell: Shell) {
    zgui::task::spawn_local(async move {
        let Some(text) = files::open_text("Workspace", &["json"]).await else {
            return;
        };
        match parse_export(&text) {
            Ok(export) => {
                shell.menu.set(None);
                store.import(export);
            }
            Err(message) => store.say(message),
        }
    });
}

fn export(store: Store, id: i64) {
    let name = live(store, id).map(|info| info.name).unwrap_or_default();
    zgui::task::spawn_local(async move {
        let bytes = match store
            .api()
            .bytes(&format!("/api/workspaces/{id}/export"))
            .await
        {
            Ok(bytes) => bytes,
            Err(error) => {
                store.fail("Cannot export the workspace", &error);
                return;
            }
        };
        files::save(store, &export_file_name(&name), &bytes).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_to_switch_the_others_by_count() {
        assert_eq!(switch_question(1), "Switch 1 other too?");
        assert_eq!(switch_question(3), "Switch 3 others too?");
    }
}
