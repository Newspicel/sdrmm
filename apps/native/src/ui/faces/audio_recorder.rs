use sdrmm_wire::{patch::NodeBody, state::AudioRecordingStatus};
use zgui::prelude::*;

use crate::{
    audio::SAMPLE_RATE,
    binding::{self, Input},
    store::Store,
    ui::{
        kit_audio::{
            self, Line, Stat, edit_body, idle_file_line, lines, recording_for, take_fault,
            take_line, time_stat, written_stat,
        },
        kit_channel::tip,
        widgets::{check, row_field},
    },
};

const TITLE: &str = "Record every wired input to its own WAV file";
const UNWIRED: &str = "Wire a channel's audio in";

#[derive(Clone, Debug, PartialEq)]
struct Take {
    name: String,
    status: Option<AudioRecordingStatus>,
}

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

fn take_of(store: Store, input: &Input) -> Take {
    let kind = input.channel.settings.params.type_id().to_owned();
    Take {
        name: kit_audio::node_label(store, &input.node, &kind),
        status: recording_for(&input.channel, &input.fx).cloned(),
    }
}

fn take_lines(takes: &[Take], recording: bool) -> Vec<Line> {
    if takes.is_empty() {
        return vec![idle_file_line()];
    }
    takes
        .iter()
        .map(|take| {
            let file = take.status.as_ref().map(|status| status.file.as_str());
            take_line(take.name.clone(), file, recording)
        })
        .collect()
}

fn take_stats(takes: &[Take]) -> Vec<Stat> {
    let held: Vec<&AudioRecordingStatus> = takes
        .iter()
        .filter_map(|take| take.status.as_ref())
        .collect();
    let longest = held.iter().map(|status| status.frames).max();
    let bytes = (!held.is_empty()).then(|| held.iter().map(|status| status.bytes).sum());
    vec![
        time_stat(longest.map(|frames| frames as f64 / f64::from(SAMPLE_RATE))),
        written_stat(bytes),
    ]
}

fn take_faults(takes: &[Take]) -> Vec<String> {
    takes
        .iter()
        .filter_map(|take| {
            let error = take
                .status
                .as_ref()
                .and_then(|status| status.error.as_deref());
            take_fault(&take.name, error)
        })
        .collect()
}

pub fn face(store: Store, node: String) -> impl IntoView {
    kit_audio::install();
    let takes = {
        let node = node.clone();
        Memo::new(move |_| {
            let state = store.state.get();
            binding::inputs_of(
                &store.graph.get(),
                &node,
                "audio",
                &state.device_sets,
                &state.trunk_systems,
            )
            .iter()
            .map(|input| take_of(store, input))
            .collect::<Vec<_>>()
        })
    };
    let recording = kit_audio::is_recording(store, node.clone());
    let unwired = Signal::derive(move || takes.with(Vec::is_empty).then_some(UNWIRED));
    view! {
        column(class = "face") {
            {skip_silence(store, node.clone())}
            {lines(move || takes.with(|takes| take_lines(takes, recording.get())))}
            {kit_audio::faults(move || takes.with(|takes| take_faults(takes)))}
            row(class = "face__foot rec-foot") {
                {kit_audio::stats(move || takes.with(|takes| take_stats(takes)))}
                {kit_audio::recorder_switch(store, node.clone(), TITLE, unwired)}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(name: &str, frames: u64, bytes: u64, error: Option<&str>) -> Take {
        Take {
            name: name.to_owned(),
            status: Some(AudioRecordingStatus {
                fx: Vec::new(),
                file: format!("/rec/{name}.wav"),
                started_at: String::from("2026-09-23T12:00:00Z"),
                channels: 1,
                frames,
                bytes,
                error: error.map(str::to_owned),
            }),
        }
    }

    fn idle(name: &str) -> Take {
        Take {
            name: name.to_owned(),
            status: None,
        }
    }

    #[test]
    fn the_footer_reads_the_longest_take_and_the_sum_written() {
        let stats = take_stats(&[take("a", 96_000, 1_000, None), take("b", 48_000, 536, None)]);
        assert_eq!(stats[0].value.as_deref(), Some("2.0 s"));
        assert_eq!(stats[1].value.as_deref(), Some("1.5 kB"));
        let none = take_stats(&[idle("a")]);
        assert_eq!(none[0].value.as_deref(), Some("-"));
        assert_eq!(none[1].value.as_deref(), Some("-"));
    }

    #[test]
    fn each_input_gets_a_file_row_and_idle_reads_a_dash() {
        let shown = take_lines(&[], false);
        assert_eq!(
            (shown[0].label.as_str(), shown[0].value.as_str()),
            ("File", "-")
        );
        let shown = take_lines(&[take("NFM", 1, 1, None), idle("AM")], true);
        assert_eq!(shown[0].value, "/rec/NFM.wav");
        assert_eq!(shown[0].title.as_deref(), Some("/rec/NFM.wav"));
        assert_eq!(
            (shown[1].label.as_str(), shown[1].value.as_str()),
            ("AM", "Waiting")
        );
        assert_eq!(take_lines(&[idle("AM")], false)[0].value, "-");
    }

    #[test]
    fn a_fault_names_the_input_it_came_from() {
        let faults = take_faults(&[take("NFM", 1, 1, Some("disk full")), take("AM", 1, 1, None)]);
        assert_eq!(faults, vec![String::from("NFM: disk full")]);
    }
}
