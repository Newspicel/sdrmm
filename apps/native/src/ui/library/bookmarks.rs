use sdrmm_wire::{
    rest::{Bookmark, CreateBookmarkRequest},
    ws::StateScope,
};
use zgui::prelude::*;

use crate::{
    shell::recordings::format_mhz,
    store::Store,
    ui::{
        kit_shell::{Entry, entry, glyph, hint, list},
        library::{
            channel_type_of, frequency_of, load, on_scope, reload,
            rows::{Badge, LibRow, Pending, Pick, act, icon_act, lib_row},
            suggest_mode, target, tune,
        },
        shell::Shell,
    },
};

const PATH: &str = "/api/bookmarks";

pub fn panel(store: Store, shell: Shell) -> impl IntoView {
    let bookmarks = load::<Vec<Bookmark>>(store, PATH.to_owned());
    let again = move || reload(store, PATH.to_owned(), bookmarks);
    on_scope(store, StateScope::Bookmarks, again);
    let aimed = target(store);
    let label = RwSignal::new_local(String::new());
    let mode = RwSignal::new_local(String::new());
    let saving = Pending::new();
    let deleting = Pending::new();
    let save = move || {
        let Some(freq_hz) = aimed
            .get_untracked()
            .and_then(|aimed| frequency_of(store, &aimed))
        else {
            return;
        };
        let typed = label.get_untracked().trim().to_owned();
        if typed.is_empty() {
            return;
        }
        let picked = mode.get_untracked().trim().to_owned();
        saving.run(async move {
            let body = CreateBookmarkRequest {
                label: typed,
                freq_hz,
                mode: (!picked.is_empty()).then_some(picked),
                group: None,
            };
            match store
                .api()
                .post::<_, serde::de::IgnoredAny>(PATH, &body)
                .await
            {
                Ok(_) => {
                    label.set(String::new());
                    mode.set(String::new());
                }
                Err(error) => store.fail("Cannot save the bookmark", &error),
            }
            again();
        });
    };
    let enabled = Signal::derive_local(move || {
        !saving.busy()
            && !label.get().trim().is_empty()
            && aimed
                .get()
                .and_then(|aimed| frequency_of(store, &aimed))
                .is_some()
    });
    let sorted = move || {
        let mut found = bookmarks
            .get()
            .and_then(Result::ok)
            .map(|found| found.to_vec())
            .unwrap_or_default();
        found.sort_by(|a, b| a.freq_hz.total_cmp(&b.freq_hz));
        found
    };
    let ready = Signal::derive_local(move || aimed.get().is_some_and(|aimed| !aimed.locked()));
    let rows = move || {
        sorted()
            .into_iter()
            .map(|bookmark| {
                AnyView::new(bookmark_row(
                    store, shell, bookmark, aimed, ready, deleting, again,
                ))
            })
            .collect::<Vec<_>>()
    };
    let pick_first = move || {
        aimed
            .get()
            .is_none()
            .then(|| hint("Select a device or decoder first."))
    };
    let hints = move || {
        let locked = aimed.get().is_some_and(|aimed| aimed.locked()) && !sorted().is_empty();
        let empty = matches!(bookmarks.get(), Some(Ok(ref found)) if found.is_empty());
        let failed = matches!(bookmarks.get(), Some(Err(_)));
        [
            locked.then(|| hint("Tuning is locked here.")),
            empty.then(|| hint("No bookmarks yet.")),
            failed.then(|| hint("Could not load the bookmarks.")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
    };
    view! {
        column(class = "sk-panel") {
            {pick_first}
            row(class = "sk-toolbar") {
                {entry(Entry::new(label, "Label current frequency"), save, || {})}
                {entry(Entry::new(mode, "mode").narrow(), save, || {})}
                {act("Save", "", enabled, None, save)}
            }
            {hints}
            {move || (!sorted().is_empty()).then(|| list(None, rows))}
        }
    }
}

fn bookmark_row(
    store: Store,
    shell: Shell,
    bookmark: Bookmark,
    aimed: Signal<Option<crate::shell::library_target::TuneTarget>>,
    ready: Signal<bool, LocalStorage>,
    deleting: Pending,
    again: impl Fn() + Copy + 'static,
) -> impl IntoView {
    let id = bookmark.id;
    let freq_hz = bookmark.freq_hz;
    let saved_mode = bookmark.mode.clone().filter(|mode| !mode.is_empty());
    let recall = move || {
        let Some(aimed) = aimed.get_untracked() else {
            return;
        };
        tune(store, shell, &aimed, freq_hz);
        suggest_mode(
            store,
            saved_mode.as_deref(),
            channel_type_of(store, &aimed).as_deref(),
            "bookmark",
            false,
        );
    };
    let remove = move || {
        deleting.run(async move {
            if let Err(error) = store.api().delete(&format!("{PATH}/{id}")).await {
                store.fail("Cannot delete the bookmark", &error);
            }
            again();
        });
    };
    let badge = bookmark
        .mode
        .clone()
        .filter(|mode| !mode.is_empty())
        .map_or(Badge::None, Badge::Chip);
    let label = format!("Delete {}", bookmark.label);
    lib_row(
        LibRow {
            primary: format_mhz(bookmark.freq_hz),
            badge,
            secondary: Some(bookmark.label),
            ..LibRow::default()
        },
        Some(Pick {
            on_pick: Box::new(recall),
            enabled: ready,
        }),
        icon_act(glyph::TRASH, label, true, deleting.idle(), remove),
        AnyView::new(()),
    )
}
