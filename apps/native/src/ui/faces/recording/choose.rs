use std::path::PathBuf;

use sdrmm_wire::RecordingInfo;
use zgui::prelude::*;

use super::{
    Library,
    library::{
        check_upload, claimed_recordings, describe_recording, recording_choices,
        recording_provenance, recording_title, upload_field,
    },
    pick,
};
use crate::{
    store::Store,
    ui::{
        faces::device::actions::Busy,
        files,
        kit_sources::{Tone, button, draft_field, icons, tip, titled_icon_button},
    },
};

pub const UPLOAD_TIP: &str = "Add a SigMF recording from this computer: a .sigmf archive, or a .sigmf-meta and .sigmf-data pair";
const SIGMF_EXTENSIONS: [&str; 3] = ["sigmf", "sigmf-meta", "sigmf-data"];

#[must_use]
pub fn library_said(
    library: Option<&Result<Vec<RecordingInfo>, String>>,
    matches: bool,
) -> Option<String> {
    match library {
        None => Some("Reading the library…".to_owned()),
        Some(Err(error)) => Some(error.clone()),
        Some(Ok(all)) if all.is_empty() => Some("No recordings yet.".to_owned()),
        Some(Ok(_)) if !matches => Some("Nothing free matches that.".to_owned()),
        Some(Ok(_)) => None,
    }
}

#[derive(Clone, Copy)]
struct Upload {
    store: Store,
    busy: Busy,
    said: RwSignal<Option<String>>,
    sending: RwSignal<bool>,
    compact: Signal<bool>,
}

impl Upload {
    fn report(self, message: String) {
        if self.compact.get_untracked() {
            self.store.say(message);
        } else {
            self.said.set(Some(message));
        }
    }

    fn send(self, node: String, parts: Vec<(String, Vec<u8>)>) {
        let names: Vec<String> = parts.iter().map(|(name, _)| name.clone()).collect();
        if let Some(problem) = check_upload(&names) {
            self.report(problem.said().to_owned());
            return;
        }
        let store = self.store;
        self.said.set(None);
        self.sending.set(true);
        zgui::task::spawn_local(async move {
            let mut form = reqwest::multipart::Form::new();
            for (name, bytes) in parts {
                let field = upload_field(&name);
                form = form.part(
                    field,
                    reqwest::multipart::Part::bytes(bytes).file_name(name),
                );
            }
            let sent = store
                .api()
                .multipart::<RecordingInfo>("/api/recordings", form)
                .await;
            self.sending.try_set(false);
            match sent {
                Ok(recording) => pick(store, node, self.busy, recording),
                Err(error) => self.report(format!("{error:#}")),
            }
        });
    }

    fn browse(self, node: String) {
        zgui::task::spawn_local(async move {
            let picked = files::open_many("SigMF", &SIGMF_EXTENSIONS).await;
            if picked.is_empty() {
                return;
            }
            let parts = picked
                .into_iter()
                .map(|file| (file.name, file.bytes))
                .collect();
            self.send(node, parts);
        });
    }

    fn dropped(self, node: String, paths: Vec<PathBuf>) {
        zgui::task::spawn_local(async move {
            let read = zgui::task::blocking(move || {
                paths
                    .into_iter()
                    .map(|path| {
                        let name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        std::fs::read(&path).map(|bytes| (name, bytes))
                    })
                    .collect::<std::io::Result<Vec<_>>>()
            })
            .await;
            match read {
                Ok(parts) => self.send(node, parts),
                Err(error) => self.report(format!("Cannot read the files: {error}")),
            }
        });
    }
}

fn upload_button(upload: Upload, node: String) -> impl IntoView {
    let blocked = Signal::derive(move || upload.busy.busy.get() || upload.sending.get());
    let press = move || upload.browse(node.clone());
    move || {
        let press = press.clone();
        if upload.compact.get() {
            titled_icon_button(
                icons::UPLOAD,
                "Upload SigMF",
                || UPLOAD_TIP.to_owned(),
                Signal::stored(false),
                blocked,
                press,
            )
        } else {
            let said = upload.said;
            AnyView::new(view! {
                column(class = "kit-upload") {
                    {tip(|| UPLOAD_TIP.to_owned(), AnyView::new(button(move || if upload.sending.get() { "Uploading…".to_owned() } else { "Upload SigMF".to_owned() }, Tone::Plain, blocked, press)))}
                    {move || said.get().map(|said| AnyView::new(view! { text(class = "kit-alert") {{said}} }))}
                }
            })
        }
    }
}

fn choice(store: Store, node: String, busy: Busy, recording: RecordingInfo) -> AnyView {
    let meta = describe_recording(&recording);
    let title = recording_title(&recording);
    let provenance = recording_provenance(&recording);
    tip(
        move || provenance.clone(),
        AnyView::new(view! {
            control(
                class = "kit-choice",
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                state:disabled = busy.busy,
                on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                on:click:stop = move |_| {
                    if !busy.busy.get_untracked() {
                        pick(store, node.clone(), busy, recording.clone());
                    }
                }
            ) {
                text {{title}}
                text(class = "kit-choice__meta") {{meta}}
            }
        }),
    )
}

pub fn pick_view(store: Store, node: String, shelf: Library, busy: Busy) -> impl IntoView {
    let search = RwSignal::new_local(String::new());
    let choices = {
        let node = node.clone();
        Signal::derive_local(move || {
            let all = shelf.get().and_then(Result::ok).unwrap_or_default();
            let claimed = store.graph.with(|graph| claimed_recordings(graph, &node));
            recording_choices(&all, &claimed, &search.get())
        })
    };
    let count = Signal::derive(move || {
        shelf.with(|shelf| {
            shelf
                .as_ref()
                .and_then(|s| s.as_ref().ok())
                .map_or(0, Vec::len)
        })
    });
    let upload = Upload {
        store,
        busy,
        said: RwSignal::new(None),
        sending: RwSignal::new(false),
        compact: Signal::derive(move || count.get() > 0),
    };
    let list = {
        let node = node.clone();
        move || {
            choices
                .get()
                .into_iter()
                .map(|recording| choice(store, node.clone(), busy, recording))
                .collect::<Vec<_>>()
        }
    };
    let said =
        move || shelf.with(|shelf| library_said(shelf.as_ref(), !choices.with(Vec::is_empty)));
    let dropped = {
        let node = node.clone();
        move |ev: &mut EventCx<'_, events::Drop>| upload.dropped(node.clone(), ev.paths.clone())
    };
    view! {
        column(class = "face kit-drop", on:drop = dropped) {
            row(class = "kit-pop__row") {
                box(class = "kit-grow", hidden = move || count.get() == 0) {
                    {draft_field("Search recordings", search, "Search recordings", Signal::derive_local(|| false), || {})}
                }
                {upload_button(upload, node)}
            }
            column(class = "kit-list") {{list}}
            {move || said().map(|said| AnyView::new(view! { text(class = "kit-note") {{said}} }))}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_says_what_it_is_doing_or_why_nothing_shows() {
        assert_eq!(
            library_said(None, false).as_deref(),
            Some("Reading the library…")
        );
        assert_eq!(
            library_said(Some(&Err("offline".to_owned())), false).as_deref(),
            Some("offline")
        );
        assert_eq!(
            library_said(Some(&Ok(Vec::new())), false).as_deref(),
            Some("No recordings yet.")
        );
    }
}
