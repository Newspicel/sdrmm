use std::time::Duration;

use sdrmm_wire::{
    about::AboutResponse,
    rest::{AudioRecordingInfo, AudioRecordingsResponse, RecordingInfo, RecordingsResponse},
    ws::StateScope,
};
use zgui::prelude::*;
use zgui_ui::prelude::TextareaProps;

use sdrmm_wire::rest::MAX_RECORDING_NAME_LEN;

use crate::{
    shell::recordings::{
        annotation, capped, check_upload, describe_audio, describe_recording, format_tags,
        formats_for, matches_search, open_position, recording_node_for, recording_provenance,
        recording_title, upload_field,
    },
    store::Store,
    ui::{
        files,
        kit_shell::{Entry, button, entry, glyph, hint, hover, list, typing},
        library::{
            load, on_scope,
            playback::{Player, play_button, transport},
            reload,
            rows::{LibRow, Pending, act, icon_act, lib_row},
        },
        patch::graph,
        shell::Shell,
    },
};

const IQ: &str = "/api/recordings";
const AUDIO: &str = "/api/audiorecordings";
const CLEAR_ARM: Duration = Duration::from_secs(3);

const SHEET: &str = css!(
    r#"
.rec-note { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sk-field.sk-field.rec-note-field { height: auto; min-height: 56px; }
.rec-form { padding: 0 0 0 8px; border-left: 2px solid var(--accent-dim); }
"#
);

#[derive(Clone, Copy)]
struct Lists {
    iq: RwSignal<Option<Result<std::sync::Arc<RecordingsResponse>, String>>>,
    audio: RwSignal<Option<Result<std::sync::Arc<AudioRecordingsResponse>, String>>>,
    deleting: Pending,
    saving: Pending,
    player: Player,
}

impl Lists {
    fn again(self, store: Store) {
        reload(store, IQ.to_owned(), self.iq);
        reload(store, AUDIO.to_owned(), self.audio);
    }

    fn iq(self) -> Vec<RecordingInfo> {
        self.iq
            .get()
            .and_then(Result::ok)
            .map(|found| found.recordings.clone())
            .unwrap_or_default()
    }

    fn audio(self) -> Vec<AudioRecordingInfo> {
        self.audio
            .get()
            .and_then(Result::ok)
            .map(|found| found.recordings.clone())
            .unwrap_or_default()
    }
}

pub fn panel(store: Store, shell: Shell) -> impl IntoView {
    install_stylesheet("library-recordings", SHEET);
    let lists = Lists {
        iq: load(store, IQ.to_owned()),
        audio: load(store, AUDIO.to_owned()),
        deleting: Pending::new(),
        saving: Pending::new(),
        player: Player::new(store),
    };
    on_scope(store, StateScope::Recordings, move || lists.again(store));
    let about = load::<AboutResponse>(store, String::from("/api/about"));
    let reveal = Signal::derive(move || {
        about
            .get()
            .and_then(Result::ok)
            .is_some_and(|about| about.reveal)
    });
    let search = RwSignal::new_local(String::new());
    let editing = RwSignal::new(None::<i64>);
    let shown = move || {
        let needle = search.get();
        lists
            .iq()
            .into_iter()
            .filter(|recording| matches_search(recording, &needle))
            .collect::<Vec<_>>()
    };
    let hints = move || {
        let listed = lists.iq();
        if matches!(lists.iq.get(), Some(Err(_))) || matches!(lists.audio.get(), Some(Err(_))) {
            Some(hint("Could not load the recordings."))
        } else if listed.is_empty() {
            Some(hint("No recordings yet."))
        } else if shown().is_empty() {
            Some(hint(format!(
                "No recording matches \u{201c}{}\u{201d}.",
                search.get()
            )))
        } else {
            None
        }
    };
    let rows = move || {
        shown()
            .into_iter()
            .map(|recording| {
                AnyView::new(iq_row(
                    store, shell, recording, lists, reveal, editing, search,
                ))
            })
            .collect::<Vec<_>>()
    };
    let folder = move || {
        let dir = lists
            .iq
            .get()
            .and_then(Result::ok)
            .and_then(|found| found.dir.clone())?;
        let shown = dir.clone();
        Some(AnyView::new(view! {
            row(class = "sk-toolbar") {
                {hover(dir, view! { text(class = "legend") {{shown}} })}
                spacer() {}
                {move || reveal.get().then(|| act("Show in folder", "sm", Signal::stored_local(true), Some(String::from("Open this folder in the machine's file manager")), move || post(store, format!("{IQ}/reveal"), "Cannot show the folder")))}
            }
        }))
    };
    view! {
        column(class = "sk-panel") {
            row(class = "sk-toolbar") {
                {move || (!lists.iq().is_empty()).then(|| AnyView::new(entry(Entry::new(search, "Search name, tag or note"), || {}, || {})))}
                {button("Upload SigMF", "", move || upload(store, lists))}
                {clear_all(store, lists)}
            }
            {folder}
            {hints}
            {move || (!shown().is_empty()).then(|| list(Some("IQ"), rows))}
            {audio_list(store, lists, reveal)}
        }
    }
}

fn post(store: Store, path: String, context: &'static str) {
    zgui::task::spawn_local(async move {
        if let Err(error) = store.api().post_empty::<serde::de::IgnoredAny>(&path).await {
            store.fail(context, &error);
        }
    });
}

fn clear_all(store: Store, lists: Lists) -> impl IntoView {
    let armed = RwSignal::new(false);
    let clearing = Pending::new();
    let disarm = StoredValue::new_local(None::<zgui::view::TimeoutHandle>);
    let clock = Timers::current();
    let press = move || {
        if !armed.get_untracked() {
            armed.set(true);
            if let Some(clock) = &clock {
                disarm.set_value(Some(clock.set_timeout(CLEAR_ARM, move || armed.set(false))));
            }
            return;
        }
        armed.set(false);
        let iq = lists.iq();
        let audio = lists.audio();
        clearing.run(async move {
            let api = store.api();
            let mut failed = 0usize;
            for recording in iq {
                failed += usize::from(api.delete(&format!("{IQ}/{}", recording.id)).await.is_err());
            }
            for recording in audio {
                failed += usize::from(
                    api.delete(&format!("{AUDIO}/{}", recording.file))
                        .await
                        .is_err(),
                );
            }
            if failed > 0 {
                store.say(format!(
                    "{failed} recording{} could not be deleted",
                    if failed == 1 { "" } else { "s" }
                ));
            }
            lists.again(store);
        });
    };
    move || {
        let press = press.clone();
        let any = !lists.iq().is_empty() || !lists.audio().is_empty();
        any.then(|| {
            hover(
                "Delete every IQ and audio recording",
                view! {
                    control(
                        class = "sk-btn",
                        class:danger = move || armed.get(),
                        state:disabled = move || clearing.busy(),
                        tabindex = Focus::Sequential,
                        a11y:role = Role::Button,
                        on:click:stop = move |_| if !clearing.busy() { press() }
                    ) {
                        {move || if armed.get() { "Confirm clear" } else { "Clear all" }}
                    }
                },
            )
        })
    }
}

fn upload(store: Store, lists: Lists) {
    zgui::task::spawn_local(async move {
        let picked = files::open_many("SigMF", &["sigmf", "sigmf-meta", "sigmf-data"]).await;
        if picked.is_empty() {
            return;
        }
        let names: Vec<String> = picked.iter().map(|file| file.name.clone()).collect();
        if let Some(problem) = check_upload(&names) {
            store.say(problem.said());
            return;
        }
        let mut form = reqwest::multipart::Form::new();
        for file in picked {
            let slot = upload_field(&file.name);
            form = form.part(
                slot,
                reqwest::multipart::Part::bytes(file.bytes).file_name(file.name),
            );
        }
        match store.api().post_form::<RecordingInfo>(IQ, form).await {
            Ok(recording) => store.note(format!("Added {}", recording_title(&recording))),
            Err(error) => store.fail("Cannot upload", &error),
        }
        lists.again(store);
        store.refresh_state();
    });
}

fn open_as_source(store: Store, shell: Shell, recording: &RecordingInfo) {
    let graph = store.graph.get_untracked();
    let id = graph::new_node_id("recording", &graph::node_ids(&graph));
    let node = recording_node_for(recording, id.clone(), open_position(&graph));
    store.edit_graph(move |graph| graph.nodes.push(node));
    store.selected.set(Some(id));
    shell.menu.set(None);
}

fn downloads(store: Store, recording: &RecordingInfo) -> Vec<AnyView> {
    let id = recording.id;
    formats_for(recording)
        .iter()
        .map(|format| {
            let path = format!("{IQ}/{id}/download?format={}", format.format);
            let name = format!("{}{}", recording.file, format.label);
            act(
                format.label,
                "sm",
                Signal::stored_local(true),
                Some(format.hint.to_owned()),
                move || {
                    let path = path.clone();
                    let name = name.clone();
                    zgui::task::spawn_local(
                        async move { files::download(store, &path, &name).await },
                    );
                },
            )
        })
        .collect()
}

fn delete(store: Store, lists: Lists, path: String) {
    lists.deleting.run(async move {
        if let Err(error) = store.api().delete(&path).await {
            store.fail("Cannot delete the recording", &error);
        }
        lists.again(store);
    });
}

fn iq_row(
    store: Store,
    shell: Shell,
    recording: RecordingInfo,
    lists: Lists,
    reveal: Signal<bool>,
    editing: RwSignal<Option<i64>>,
    search: RwSignal<String, LocalStorage>,
) -> impl IntoView {
    let id = recording.id;
    let title = recording_title(&recording);
    let opened = recording.clone();
    let idle = lists.deleting.idle();
    let actions = view! {
        {button("Open as source", "sm", move || open_as_source(store, shell, &opened))}
        {downloads(store, &recording)}
        {icon_act(glyph::PENCIL, format!("Annotate {title}"), false, Signal::stored_local(true), move || {
            editing.update(|open| *open = if *open == Some(id) { None } else { Some(id) });
        })}
        {move || reveal.get().then(|| icon_act(glyph::FOLDER, "Show in folder", false, Signal::stored_local(true), move || post(store, format!("{IQ}/{id}/reveal"), "Cannot show the recording")))}
        {icon_act(glyph::TRASH, format!("Delete {title}"), true, idle, move || delete(store, lists, format!("{IQ}/{id}")))}
    };
    let kept = recording.clone();
    let below = move || {
        if editing.get() == Some(id) {
            AnyView::new(annotation_form(store, kept.clone(), lists, editing))
        } else {
            AnyView::new(tags_line(&kept, search))
        }
    };
    lib_row(
        LibRow {
            primary: recording_title(&recording),
            secondary: Some(describe_recording(&recording)),
            hint: Some(recording_provenance(&recording)),
            ..LibRow::default()
        },
        None,
        AnyView::new(actions),
        AnyView::new(below),
    )
}

fn tags_line(recording: &RecordingInfo, search: RwSignal<String, LocalStorage>) -> impl IntoView {
    let chips: Vec<AnyView> = recording
        .tags
        .iter()
        .map(|tag| {
            let picked = tag.clone();
            let shown = tag.clone();
            hover(
                format!("Search for {tag}"),
                view! {
                    control(
                        class = "sk-chip",
                        tabindex = Focus::Sequential,
                        a11y:role = Role::Button,
                        on:click:stop = move |_| search.set(picked.clone())
                    ) {
                        {shown}
                    }
                },
            )
        })
        .collect();
    let note = recording
        .note
        .clone()
        .filter(|note| !note.is_empty())
        .map(|note| {
            let shown = note.lines().next().unwrap_or_default().to_owned();
            hover(note, view! { text(class = "sk-text rec-note") {{shown}} })
        });
    (!chips.is_empty() || note.is_some()).then(|| {
        AnyView::new(view! {
            row(class = "sk-toolbar") {
                {chips}
                {note}
            }
        })
    })
}

fn note_field(note: RwSignal<String, LocalStorage>) -> impl IntoView {
    view! {
        box(class = "sk-entry", on:key_down = typing) {
            Textarea(
                class = "sk-field rec-note-field",
                value = note,
                label = "Note",
                placeholder = "What was on the air, and what to remember about it"
            )
        }
    }
}

fn annotation_form(
    store: Store,
    recording: RecordingInfo,
    lists: Lists,
    editing: RwSignal<Option<i64>>,
) -> impl IntoView {
    let id = recording.id;
    let name = RwSignal::new_local(recording.name.clone().unwrap_or_default());
    let tags = RwSignal::new_local(format_tags(&recording.tags));
    let note = RwSignal::new_local(recording.note.clone().unwrap_or_default());
    let capping = zgui::reactive::RenderEffect::new(move |_| {
        if let Some(cut) = name.with(|typed| capped(typed, MAX_RECORDING_NAME_LEN)) {
            name.set(cut);
        }
    });
    on_cleanup_local(move || drop(capping));
    let save = move || {
        let body = annotation(
            &name.get_untracked(),
            &tags.get_untracked(),
            &note.get_untracked(),
        );
        lists.saving.run(async move {
            match store
                .api()
                .put::<_, serde::de::IgnoredAny>(&format!("{IQ}/{id}/annotation"), &body)
                .await
            {
                Ok(_) => editing.set(None),
                Err(error) => store.fail("Cannot save the note", &error),
            }
            lists.again(store);
        });
    };
    let cancel = move || editing.set(None);
    view! {
        column(class = "sk-panel rec-form") {
            {entry(Entry::new(name, "Name this recording").focused(), save, cancel)}
            {entry(Entry::new(tags, "Tags, comma separated"), save, cancel)}
            {note_field(note)}
            row(class = "sk-toolbar") {
                {act("Save", "sm", lists.saving.idle(), None, save)}
                {button("Cancel", "sm", cancel)}
            }
        }
    }
}

fn audio_list(store: Store, lists: Lists, reveal: Signal<bool>) -> impl IntoView {
    let rows = move || {
        lists
            .audio()
            .into_iter()
            .map(|recording| AnyView::new(audio_row(store, recording, lists, reveal)))
            .collect::<Vec<_>>()
    };
    move || (!lists.audio().is_empty()).then(|| list(Some("Channel audio"), rows))
}

fn audio_row(
    store: Store,
    recording: AudioRecordingInfo,
    lists: Lists,
    reveal: Signal<bool>,
) -> impl IntoView {
    let file = recording.file.clone();
    let revealed = file.clone();
    let fetched = file.clone();
    let removed = file.clone();
    let download = move || {
        let file = fetched.clone();
        zgui::task::spawn_local(async move {
            files::download(store, &format!("{AUDIO}/{file}/download"), &file).await;
        });
    };
    let actions = view! {
        {play_button(lists.player, file.clone())}
        {move || {
            let revealed = revealed.clone();
            reveal.get().then(|| icon_act(glyph::FOLDER, "Show in folder", false, Signal::stored_local(true), move || post(store, format!("{AUDIO}/{revealed}/reveal"), "Cannot show the recording")))
        }}
        {icon_act(glyph::DOWNLOAD, "Download WAV", false, Signal::stored_local(true), download)}
        {icon_act(glyph::TRASH, format!("Delete {file}"), true, lists.deleting.idle(), move || {
            lists.player.stop_if(&removed);
            delete(store, lists, format!("{AUDIO}/{removed}"));
        })}
    };
    lib_row(
        LibRow {
            primary: recording.file.clone(),
            secondary: Some(describe_audio(&recording)),
            ..LibRow::default()
        },
        None,
        AnyView::new(actions),
        AnyView::new(transport(lists.player, recording.file)),
    )
}
