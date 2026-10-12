mod choose;
pub mod library;
pub mod playback;

use sdrmm_wire::{
    RecordingInfo, RecordingsResponse,
    patch::NodeBody,
    state::DeviceSetStatus,
    ws::{ServerEvent, StateScope},
};
use zgui::prelude::*;

use self::{
    choose::pick_view,
    library::{describe_recording, find_recording, recording_device_id, recording_title},
};
use super::{
    array::make_array_button,
    device::{
        actions::{Busy, create_set, edit_body, release},
        devices::device_id,
        radio::Radio,
    },
    set_signal,
};
use crate::{
    head::{self, NodeHead, Tone as HeadTone},
    store::Store,
    ui::kit_sources::{
        Read, Tone, footer, health::source_health, install, readout_titled, titled_button, units,
    },
};

pub const FORGET_TIP: &str = "Free this node so you can pick another recording";
pub const CLOSE_TIP: &str = "Stop playing and free the node: the wires stay drawn";

#[must_use]
pub fn play_title(gone: bool) -> &'static str {
    if gone {
        "This recording is no longer in the library"
    } else {
        "Start playing this recording into whatever is wired to it"
    }
}

#[must_use]
pub fn recording_facts(known: &RecordingInfo) -> Vec<(&'static str, String, Option<&'static str>)> {
    let mut facts = Vec::new();
    if known.lanes > 1 {
        facts.push((
            "Lanes",
            known.lanes.to_string(),
            Some("Lanes recorded side by side"),
        ));
    }
    facts.extend([
        (
            "Centre",
            units::mhz(known.center_hz),
            Some("Recorded centre"),
        ),
        (
            "Rate",
            units::sample_rate(known.sample_rate),
            Some("Recorded sample rate"),
        ),
        ("Length", units::duration(known.duration_s), None),
        (
            "Size",
            units::bytes(known.bytes as f64),
            Some("Size on disk"),
        ),
    ]);
    facts
}

pub(crate) type Library = RwSignal<Option<Result<Vec<RecordingInfo>, String>>>;

fn library(store: Store) -> Library {
    let shelf: Library = RwSignal::new(None);
    let fetch = move || {
        zgui::task::spawn_local(async move {
            let fetched = store
                .api()
                .get::<RecordingsResponse>("/api/recordings")
                .await
                .map(|response| response.recordings)
                .map_err(|error| error.to_string());
            shelf.try_set(Some(fetched));
        });
    };
    fetch();
    store.on_event(move |event| {
        if matches!(
            event,
            ServerEvent::StateChanged {
                scope: StateScope::Recordings | StateScope::All
            }
        ) {
            fetch();
        }
    });
    shelf
}

fn stem_of(store: Store, node: &str) -> Option<String> {
    store.graph.with(|graph| {
        graph.node(node).and_then(|found| match &found.body {
            NodeBody::Recording(recording) => {
                recording.recording.clone().filter(|stem| !stem.is_empty())
            }
            _ => None,
        })
    })
}

fn name_recording(store: Store, node: String, stem: Option<String>) {
    edit_body(store, node, move |body| {
        if let NodeBody::Recording(recording) = body {
            recording.recording = stem;
        }
    });
}

#[derive(Clone, PartialEq)]
enum Phase {
    Unnamed,
    Named(String),
    Open(String),
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install();
    let set = set_signal(store, node.clone());
    let shelf = library(store);
    let busy = Busy::new();
    let phase = {
        let node = node.clone();
        Memo::new(
            move |_| match (stem_of(store, &node), set.with(Option::is_some)) {
                (None, _) => Phase::Unnamed,
                (Some(stem), false) => Phase::Named(stem),
                (Some(stem), true) => Phase::Open(stem),
            },
        )
    };
    move || match phase.get() {
        Phase::Unnamed => AnyView::new(pick_view(store, node.clone(), shelf, busy)),
        Phase::Named(stem) => AnyView::new(named(store, node.clone(), stem, shelf, busy)),
        Phase::Open(stem) => AnyView::new(playing(
            Radio { store, set },
            node.clone(),
            stem,
            shelf,
            busy,
        )),
    }
}

pub(crate) fn pick(store: Store, node: String, busy: Busy, recording: RecordingInfo) {
    let open = store.state.with_untracked(|state| {
        state
            .device_sets
            .iter()
            .any(|set| device_id(&set.device) == recording.device_id)
    });
    busy.run(store, async move {
        if !open {
            create_set(store, recording.device_id.clone()).await?;
        }
        name_recording(store, node, Some(recording.file));
        Ok(())
    });
}

fn named(store: Store, node: String, stem: String, shelf: Library, busy: Busy) -> impl IntoView {
    let known = {
        let stem = stem.clone();
        Signal::derive(move || {
            shelf
                .get()
                .and_then(Result::ok)
                .and_then(|all| find_recording(&all, Some(&stem)).cloned())
        })
    };
    let gone = Signal::derive(move || {
        shelf.with(|shelf| matches!(shelf, Some(Ok(_)))) && known.with(Option::is_none)
    });
    let blocked = Signal::derive(move || gone.get() || busy.busy.get());
    head::show(store, node.clone(), move || {
        if gone.get() {
            NodeHead::said("missing")
        } else {
            NodeHead::default()
        }
    });
    let forget_node = node.clone();
    let play = move || {
        let stem = stem.clone();
        busy.run(store, async move {
            create_set(store, recording_device_id(&stem)).await?;
            store.apply().await;
            Ok(())
        });
    };
    view! {
        column(class = "face") {
            text(class = "kit-mono") {{move || known.get().map(|known| known.file).unwrap_or_else(|| stem_of(store, &node).unwrap_or_default())}}
            text(class = "kit-meta") {{move || known.get().map(|known| describe_recording(&known)).unwrap_or_default()}}
            {footer(view! {
                {titled_button(|| "Forget recording".to_owned(), Tone::Quiet, busy.busy.into(), || FORGET_TIP.to_owned(), move || forget(store, forget_node.clone(), busy))}
                {titled_button(|| "Play".to_owned(), Tone::Primary, blocked, move || play_title(gone.get()).to_owned(), play)}
            })}
        }
    }
}

fn forget(store: Store, node: String, busy: Busy) {
    release(store, node, busy, |body| {
        if let NodeBody::Recording(recording) = body {
            recording.recording = None;
        }
    });
}

fn playing(radio: Radio, node: String, stem: String, shelf: Library, busy: Busy) -> impl IntoView {
    let store = radio.store;
    let known = {
        let stem = stem.clone();
        Signal::derive(move || {
            shelf
                .get()
                .and_then(Result::ok)
                .and_then(|all| find_recording(&all, Some(&stem)).cloned())
        })
    };
    let title = Signal::derive(move || {
        known
            .get()
            .map_or_else(|| stem.clone(), |known| recording_title(&known))
    });
    let failed = Signal::derive(move || {
        radio
            .read(|set| set.status == DeviceSetStatus::Error)
            .unwrap_or(false)
    });
    let has_transport =
        Memo::new(move |_| radio.read(|set| set.playback.is_some()).unwrap_or(false));
    let error = Signal::derive(move || radio.read(|set| set.error.clone()).flatten());
    let facts = move || {
        known.get().map(|known| {
            let rows = recording_facts(&known)
                .into_iter()
                .map(|(label, value, title)| {
                    let read = Read::new(label, AnyView::new(value));
                    match title {
                        Some(title) => read.titled(title),
                        None => read,
                    }
                })
                .collect();
            AnyView::new(readout_titled(rows))
        })
    };
    head::show(store, node.clone(), move || {
        if failed.get() {
            NodeHead::said("error").toned(HeadTone::Danger)
        } else {
            NodeHead::default()
        }
    });
    let make_array = make_array_button(store, node.clone(), radio.set);
    let closing = busy.busy;
    view! {
        column(class = "face") {
            text(class = "kit-head__title") {{move || title.get()}}
            {move || has_transport.get().then(|| AnyView::new(playback::transport(radio)))}
            {facts}
            {move || error.get().map(|error| AnyView::new(view! { text(class = "kit-alert") {{error}} }))}
            {footer(view! {
                {source_health(store, radio.set, |_| Vec::new())}
                {make_array}
                {titled_button(move || if closing.get() { "Closing…".to_owned() } else { "Forget recording".to_owned() }, Tone::Quiet, closing.into(), || CLOSE_TIP.to_owned(), move || forget(store, node.clone(), busy))}
            })}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording(lanes: u32) -> RecordingInfo {
        serde_json::from_value(serde_json::json!({
            "id": 1, "file": "airband", "device_id": "recording:airband", "device_label": "RTL-SDR",
            "center_hz": 118e6, "sample_rate": 2.4e6, "samples": 2_400_000, "bytes": 9_600_000,
            "duration_s": 1.0, "created_at": "2026-01-01T00:00:00Z", "lanes": lanes
        }))
        .expect("recording")
    }

    #[test]
    fn a_multi_lane_recording_names_its_lanes_first() {
        let labels = |lanes| -> Vec<&str> {
            recording_facts(&recording(lanes))
                .into_iter()
                .map(|(label, _, _)| label)
                .collect()
        };
        assert_eq!(labels(1), vec!["Centre", "Rate", "Length", "Size"]);
        assert_eq!(labels(4), vec!["Lanes", "Centre", "Rate", "Length", "Size"]);
        assert_eq!(recording_facts(&recording(4))[0].1, "4");
    }

    #[test]
    fn play_says_why_it_is_greyed_out() {
        assert!(play_title(true).contains("no longer"));
        assert!(play_title(false).starts_with("Start playing"));
    }
}
