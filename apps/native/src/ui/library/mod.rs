mod bands;
mod bookmarks;
mod occupancy;
mod phones;
mod playback;
mod presets;
mod qr;
mod recordings;
mod remote;
mod rows;
mod templates;
mod wav;

use std::sync::Arc;

use sdrmm_wire::{
    array::ArrayTuneRequest,
    patch::NodeBody,
    state::DeviceSet,
    ws::{ServerEvent, StateScope},
};
use zgui::prelude::*;

use crate::{
    shell::{
        library_target::{TuneTarget, bound_sets, library_target},
        tuner::{self, Reach},
    },
    store::Store,
    ui::{
        faces::channel::edit::descriptor_for,
        shell::{LibraryTab, Shell},
        tools,
    },
};

const SHEET: &str = css!(
    r#"
.lib {
    position: absolute;
    right: 44px;
    top: 42px;
    z-index: 150;
    width: 700px;
    max-width: 92%;
    flex-direction: column;
    border: 1px solid var(--line-strong);
    border-radius: 7px;
    background-color: var(--panel);
    box-shadow: 0 18px 44px rgba(0, 0, 0, 0.5);
    overflow: hidden;
}
.lib__tabs { align-items: center; gap: 2px; padding: 4px; border-bottom: 1px solid var(--line); background-color: var(--panel-2); flex: 0 0 auto; }
.lib__tab { padding: 3px 9px; border-radius: 4px; font-size: 12px; color: var(--ink-dim); }
.lib__tab:hover { color: var(--ink); }
.lib__tab.on { background-color: var(--panel-3); color: var(--ink); }
.lib__body { max-height: 460px; overflow: auto; flex-direction: column; }
"#
);

const TABS: [(LibraryTab, &str); 6] = [
    (LibraryTab::Templates, "Templates"),
    (LibraryTab::Presets, "Presets"),
    (LibraryTab::Bookmarks, "Bookmarks"),
    (LibraryTab::Bands, "Bands"),
    (LibraryTab::Occupancy, "Occupancy"),
    (LibraryTab::Recordings, "Recordings"),
];

const AFTER_TOOLS: [(LibraryTab, &str); 2] = [
    (LibraryTab::Phones, "Phones"),
    (LibraryTab::Remote, "Remote"),
];

fn tab_views(shell: Shell, tabs: &[(LibraryTab, &'static str)]) -> Vec<AnyView> {
    tabs.iter()
        .map(|&(tab, label)| {
            AnyView::new(view! {
                control(
                    class = "lib__tab",
                    class:on = move || shell.library_tab.get() == tab,
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Tab,
                    on:click:stop = move |_| shell.library_tab.set(tab)
                ) {
                    {label}
                }
            })
        })
        .collect()
}

pub fn popover(store: Store, shell: Shell) -> impl IntoView {
    install_stylesheet("shell-library", SHEET);
    let tabs = tab_views(shell, &TABS);
    let later = tab_views(shell, &AFTER_TOOLS);
    let body = move || match shell.library_tab.get() {
        LibraryTab::Templates => AnyView::new(templates::panel(store)),
        LibraryTab::Presets => AnyView::new(presets::panel(store)),
        LibraryTab::Bookmarks => AnyView::new(bookmarks::panel(store, shell)),
        LibraryTab::Bands => AnyView::new(bands::panel(store, shell)),
        LibraryTab::Occupancy => AnyView::new(occupancy::panel(store, shell)),
        LibraryTab::Recordings => AnyView::new(recordings::panel(store, shell)),
        LibraryTab::Phones => AnyView::new(phones::panel(store)),
        LibraryTab::Remote => AnyView::new(remote::panel(store)),
    };
    view! {
        column(
            class = "lib",
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation()
        ) {
            row(class = "lib__tabs", a11y:role = Role::TabList) {
                {tabs}
                control(
                    class = "lib__tab",
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Tab,
                    on:click:stop = move |_| {
                        shell.menu.set(None);
                        tools::open(store, None);
                    }
                ) {
                    "Tools"
                }
                {later}
            }
            column(class = "lib__body") {{body}}
        }
    }
}

pub(crate) fn target(store: Store) -> Signal<Option<TuneTarget>> {
    Signal::derive(move || {
        let graph = store.graph.get();
        let sets = bound_sets(&graph, &store.state.get().device_sets);
        library_target(&graph, &sets, store.selected.get().as_deref())
    })
}

pub(crate) fn active_set(store: Store) -> Signal<Option<DeviceSet>> {
    Signal::derive(move || {
        let graph = store.graph.get();
        let sets = bound_sets(&graph, &store.state.get().device_sets);
        let selected = store.selected.get();
        let picked = selected
            .as_deref()
            .and_then(|selected| sets.iter().find(|(node, _)| node == selected));
        match (picked, sets.as_slice()) {
            (Some((_, set)), _) | (None, [(_, set)]) => Some(set.clone()),
            _ => None,
        }
    })
}

pub(crate) fn frequency_of(store: Store, target: &TuneTarget) -> Option<f64> {
    match target {
        TuneTarget::Device { set, .. } => tuner::center_hz(set, 0),
        TuneTarget::Channel { node, .. } => store
            .channel_settings(node)
            .map(|settings| settings.frequency_hz),
    }
}

pub(crate) fn channel_type_of(store: Store, target: &TuneTarget) -> Option<String> {
    let TuneTarget::Channel { node, .. } = target else {
        return None;
    };
    match store
        .graph
        .get_untracked()
        .node(node)
        .map(|found| &found.body)
    {
        Some(NodeBody::Channel(channel)) => Some(channel.channel_type.clone()),
        _ => None,
    }
}

pub(crate) fn tune(store: Store, shell: Shell, target: &TuneTarget, hz: f64) {
    if target.locked() {
        return;
    }
    match target {
        TuneTarget::Device { set, .. } => tune_radio(store, shell, set, hz),
        TuneTarget::Channel { node, set, .. } => {
            store.edit_channel(node, |settings| settings.frequency_hz = hz);
            if let Some(set) = set {
                reach(store, node, set, hz);
            }
        }
    }
}

fn reach(store: Store, node: &str, set: &DeviceSet, hz: f64) {
    let graph = store.graph.get_untracked();
    let devices = bound_sets(&graph, &store.state.get_untracked().device_sets);
    let types = store.channel_types.get_untracked();
    let descriptor = descriptor_for(&graph, &types, node);
    match tuner::reach_for(&graph, node, set, &devices, descriptor, hz) {
        Reach::Stay => {}
        Reach::Radio(id, delta) => store.set_device(id, *delta),
        Reach::Array(array) => retune_array(store, array, hz),
    }
}

fn retune_array(store: Store, array: String, hz: f64) {
    zgui::task::spawn_local(async move {
        let body = ArrayTuneRequest {
            center_hz: Some(hz),
            gain: None,
        };
        let path = format!("/api/arrays/{array}/tune");
        if let Err(error) = store
            .api()
            .patch::<_, serde::de::IgnoredAny>(&path, &body)
            .await
        {
            store.fail("Cannot tune the array", &error);
        }
    });
}

pub(crate) fn tune_radio(store: Store, shell: Shell, set: &DeviceSet, hz: f64) {
    let delta = tuner::tune_delta(&set.capabilities, 0, hz);
    let id = set.id;
    shell.leave_auto(tuner::auto_tuning(set, 0), move || {
        store.set_device(id, delta)
    });
}

pub(crate) fn suggest_mode(
    store: Store,
    mode: Option<&str>,
    channel_type: Option<&str>,
    what: &str,
    ask_without_channel: bool,
) {
    if let Some(hint) = tuner::mode_hint(mode, channel_type, what, ask_without_channel) {
        store.note(hint);
    }
}

pub(crate) fn load<T>(store: Store, path: String) -> RwSignal<Option<Result<Arc<T>, String>>>
where
    T: serde::de::DeserializeOwned + Send + Sync + 'static,
{
    let into = RwSignal::new(None);
    reload(store, path, into);
    into
}

pub(crate) fn reload<T>(store: Store, path: String, into: RwSignal<Option<Result<Arc<T>, String>>>)
where
    T: serde::de::DeserializeOwned + Send + Sync + 'static,
{
    zgui::task::spawn_local(async move {
        let result = store.api().get::<T>(&path).await;
        into.set(Some(
            result.map(Arc::new).map_err(|error| error.to_string()),
        ));
    });
}

pub(crate) fn on_scope(store: Store, scope: StateScope, again: impl Fn() + 'static) {
    store.on_event(move |event| {
        if let ServerEvent::StateChanged { scope: changed } = event
            && (*changed == scope || *changed == StateScope::All)
        {
            again();
        }
    });
}
