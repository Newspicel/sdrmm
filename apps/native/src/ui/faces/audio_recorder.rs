use sdrmm_wire::patch::NodeBody;
use zgui::prelude::*;

use crate::{
    audio::SAMPLE_RATE,
    binding::{self, Input},
    store::Store,
    ui::{
        kit_audio::{self, edit_body, format_bytes, format_duration, readout, recording_for},
        kit_channel::tip,
        widgets::{check, row_field},
    },
};

fn skip_silence(store: Store, node: String) -> impl IntoView {
    let on = {
        let node = node.clone();
        Signal::derive(move || {
            store.graph.with(|graph| {
                matches!(
                    graph.node(&node).map(|found| &found.body),
                    Some(NodeBody::AudioRecorder(body)) if body.skip_silence
                )
            })
        })
    };
    let toggle = move |next: bool| {
        edit_body(store, &node, |body| {
            if let NodeBody::AudioRecorder(recorder) = body {
                recorder.skip_silence = next;
            }
        });
    };
    tip(
        "Pause while the squelch is closed",
        AnyView::new(row_field("Skip silence", check(on, toggle))),
    )
}

pub fn face(store: Store, node: String) -> impl IntoView {
    kit_audio::install();
    let inputs = {
        let node = node.clone();
        Signal::derive(move || {
            let state = store.state.get();
            binding::inputs_of(
                &store.graph.get(),
                &node,
                "audio",
                &state.device_sets,
                &state.trunk_systems,
            )
        })
    };
    let recording = kit_audio::is_recording(store, node.clone());
    let skip = skip_silence(store, node.clone());
    view! {
        column(class = "face") {
            if move || inputs.get().is_empty() {
                text(class = "hint") {"Wire a channel's audio in"}
            } else {
                row(class = "field__body") {
                    {kit_audio::recorder_switch(store, node.clone())}
                }
            }
            {skip}
            for input in move || inputs.get(), key = |input: &Input| input.route() {
                {lane(store, input, recording)}
            }
        }
    }
}

fn lane(store: Store, input: Input, recording: Signal<bool>) -> impl IntoView {
    let route = input.route();
    let name = {
        let node = input.node.clone();
        let kind = input.channel.settings.params.type_id().to_owned();
        move || kit_audio::node_label(store, &node, &kind)
    };
    let status = Signal::derive(move || {
        let state = store.state.get();
        state
            .device_sets
            .iter()
            .find(|set| set.id == route.device_set)
            .and_then(|set| {
                set.channels
                    .iter()
                    .find(|channel| channel.id == route.channel)
            })
            .and_then(|channel| recording_for(channel, &route.fx).cloned())
    });
    let elapsed = move || {
        status.get().map_or_else(String::new, |status| {
            format_duration(status.frames as f64 / f64::from(SAMPLE_RATE))
        })
    };
    let written = move || {
        status
            .get()
            .map_or_else(String::new, |status| format_bytes(status.bytes))
    };
    let file = move || status.get().map(|status| status.file).unwrap_or_default();
    view! {
        column(class = "lane") {
            row(class = "lane__head") {
                text(class = "lane__name") {{name}}
                if move || recording.get() && status.get().is_none() {
                    text(class = "hint") {"Waiting"}
                }
            }
            if move || status.get().is_some() {
                {readout(vec![
                    ("Elapsed", AnyView::new(elapsed)),
                    ("Written", AnyView::new(written)),
                    ("File", AnyView::new(file)),
                ])}
            }
            {kit_audio::alert(move || status.get().and_then(|status| status.error))}
        }
    }
}
