use sdrmm_wire::state::RecordingStatus;
use zgui::prelude::*;

use crate::{
    binding::{self, Input},
    store::Store,
    ui::kit_audio::{
        self, Line, Stat, drops_stat, idle_file_line, lines, samples_stat, take_fault, take_line,
        written_stat,
    },
};

const TITLE: &str = "Record every wired channel's baseband to its own SigMF pair";
const UNWIRED: &str = "Wire a channel's baseband in";

#[derive(Clone, Debug, PartialEq)]
struct Take {
    name: String,
    status: Option<RecordingStatus>,
}

fn take_of(store: Store, input: &Input) -> Take {
    let kind = input.channel.settings.params.type_id().to_owned();
    Take {
        name: kit_audio::node_label(store, &input.node, &kind),
        status: input.channel.baseband_recording.clone(),
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
    let held: Vec<&RecordingStatus> = takes
        .iter()
        .filter_map(|take| take.status.as_ref())
        .collect();
    let sum = |pick: fn(&RecordingStatus) -> u64| {
        (!held.is_empty()).then(|| held.iter().map(|status| pick(status)).sum::<u64>())
    };
    let mut shown = vec![
        written_stat(sum(|status| status.bytes)),
        samples_stat(sum(|status| status.samples)),
    ];
    shown.extend(drops_stat(sum(|status| status.overruns).unwrap_or(0)));
    shown
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
                "baseband",
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

    fn take(name: &str, samples: u64, bytes: u64, overruns: u64) -> Take {
        Take {
            name: name.to_owned(),
            status: Some(RecordingStatus {
                file: format!("/rec/{name}.sigmf-data"),
                stream: 0,
                started_at: String::from("2026-09-23T12:00:00Z"),
                samples,
                bytes,
                overruns,
                error: Some(String::from("disk full")),
            }),
        }
    }

    #[test]
    fn the_footer_sums_every_take() {
        let shown = take_stats(&[take("a", 1_000_000, 1_000, 0), take("b", 200_000, 500, 2)]);
        let values: Vec<_> = shown.iter().map(|stat| stat.value.as_deref()).collect();
        assert_eq!(values, vec![Some("1.5 kB"), Some("1.2M"), Some("2")]);
        let idle = take_stats(&[Take {
            name: String::from("a"),
            status: None,
        }]);
        assert_eq!(idle.len(), 2);
    }

    #[test]
    fn rows_and_faults_name_their_input() {
        let takes = [take("DMR", 1, 1, 0)];
        assert_eq!(take_lines(&takes, false)[0].label, "DMR");
        assert_eq!(take_faults(&takes), vec![String::from("DMR: disk full")]);
        assert_eq!(take_lines(&[], true)[0].label, "File");
    }
}
