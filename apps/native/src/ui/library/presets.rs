use sdrmm_wire::{
    rest::{CreatePresetRequest, PresetInfo},
    ws::StateScope,
};
use zgui::prelude::*;

use crate::{
    store::Store,
    ui::{
        kit_shell::{Entry, entry, glyph, hint, list},
        library::{
            load, on_scope, reload,
            rows::{LibRow, Pending, act, icon_act, lib_row},
        },
    },
};

const PATH: &str = "/api/presets";

pub fn panel(store: Store) -> impl IntoView {
    let presets = load::<Vec<PresetInfo>>(store, PATH.to_owned());
    let again = move || reload(store, PATH.to_owned(), presets);
    on_scope(store, StateScope::Presets, again);
    let name = RwSignal::new_local(String::new());
    let saving = Pending::new();
    let applying = Pending::new();
    let deleting = Pending::new();
    let save = move || {
        let typed = name.get_untracked().trim().to_owned();
        if typed.is_empty() {
            return;
        }
        saving.run(async move {
            let body = CreatePresetRequest { name: typed };
            match store
                .api()
                .post::<_, serde::de::IgnoredAny>(PATH, &body)
                .await
            {
                Ok(_) => name.set(String::new()),
                Err(error) => store.fail("Cannot save the preset", &error),
            }
            again();
        });
    };
    let enabled = Signal::derive_local(move || !saving.busy() && !name.get().trim().is_empty());
    let listed = move || {
        presets
            .get()
            .and_then(Result::ok)
            .map(|found| found.to_vec())
            .unwrap_or_default()
    };
    let rows = move || {
        listed()
            .into_iter()
            .map(|preset| AnyView::new(preset_row(store, preset, (applying, deleting), again)))
            .collect::<Vec<_>>()
    };
    let empty = move || match presets.get() {
        Some(Ok(found)) if found.is_empty() => {
            Some(hint("A preset saves every open radio as it is now."))
        }
        Some(Err(_)) => Some(hint("Could not load the presets.")),
        _ => None,
    };
    view! {
        column(class = "sk-panel") {
            row(class = "sk-toolbar") {
                {entry(Entry::new(name, "Name this bench"), save, || {})}
                {act("Save", "", enabled, None, save)}
            }
            {empty}
            {move || (!listed().is_empty()).then(|| list(None, rows))}
        }
    }
}

fn preset_row(
    store: Store,
    preset: PresetInfo,
    (applying, deleting): (Pending, Pending),
    again: impl Fn() + Copy + 'static,
) -> impl IntoView {
    let id = preset.id;
    let radios = format!(
        "{} radio{}",
        preset.devices,
        if preset.devices == 1 { "" } else { "s" }
    );
    let apply = move || {
        applying.run(async move {
            if let Err(error) = store
                .api()
                .post_empty::<serde::de::IgnoredAny>(&format!("{PATH}/{id}/apply"))
                .await
            {
                store.fail("Cannot apply the preset", &error);
            }
            store.refresh_state();
        });
    };
    let remove = move || {
        deleting.run(async move {
            if let Err(error) = store.api().delete(&format!("{PATH}/{id}")).await {
                store.fail("Cannot delete the preset", &error);
            }
            again();
        });
    };
    let label = format!("Delete {}", preset.name);
    lib_row(
        LibRow {
            primary: preset.name,
            secondary: Some(radios),
            ..LibRow::default()
        },
        None,
        AnyView::new(view! {
            {act("Apply", "sm", applying.idle(), None, apply)}
            {icon_act(glyph::TRASH, label, true, deleting.idle(), remove)}
        }),
        AnyView::new(()),
    )
}
