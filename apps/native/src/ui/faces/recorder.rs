use std::time::Duration;

use sdrmm_wire::state::RecordingStatus;
use zgui::prelude::*;

use super::set_signal;
use crate::{
    store::Store,
    ui::kit_audio::{
        self, Stat, drops_stat, lines, recording_elapsed_s, take_line, time_stat, written_stat,
    },
};

const TICK: Duration = Duration::from_secs(1);
const TITLE: &str = "Record IQ to a SigMF pair";
const UNWIRED: &str = "Wire a device's IQ in";

fn iq_stats(status: Option<&RecordingStatus>, now_ms: i64, sample_rate: f64) -> Vec<Stat> {
    let mut shown = vec![
        time_stat(status.map(|status| recording_elapsed_s(status, now_ms, sample_rate))),
        written_stat(status.map(|status| status.bytes)),
    ];
    shown.extend(drops_stat(status.map_or(0, |status| status.overruns)));
    shown
}

pub fn face(store: Store, node: String) -> impl IntoView {
    kit_audio::install();
    let set = set_signal(store, node.clone());
    let recording = kit_audio::is_recording(store, node.clone());
    let status = Signal::derive(move || set.get().and_then(|set| set.recording));
    let rate = Signal::derive(move || {
        set.get()
            .and_then(|set| set.settings.sample_rate)
            .unwrap_or(0.0)
    });
    let now = kit_audio::ticker(TICK);
    let unwired = Signal::derive(move || set.get().is_none().then_some(UNWIRED));
    let file = move || {
        let waiting = recording.get() && set.get().is_some();
        let status = status.get();
        vec![take_line(
            String::from("File"),
            status.as_ref().map(|status| status.file.as_str()),
            waiting,
        )]
    };
    view! {
        column(class = "face") {
            {lines(file)}
            {kit_audio::alert(move || status.get().and_then(|status| status.error))}
            row(class = "face__foot rec-foot") {
                {kit_audio::stats(move || iq_stats(status.get().as_ref(), now.get(), rate.get()))}
                {kit_audio::recorder_switch(store, node.clone(), TITLE, unwired)}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_recorder_reads_dashes_and_hides_drops() {
        let shown = iq_stats(None, 0, 48_000.0);
        let values: Vec<_> = shown.iter().map(|stat| stat.value.as_deref()).collect();
        assert_eq!(values, vec![Some("-"), Some("-")]);
    }

    #[test]
    fn drops_join_the_footer_once_samples_are_lost() {
        let status = RecordingStatus {
            file: String::from("a.sigmf"),
            stream: 0,
            started_at: String::from("garbage"),
            samples: 0,
            bytes: 2_048,
            overruns: 3,
            error: None,
        };
        let shown = iq_stats(Some(&status), 0, 48_000.0);
        assert_eq!(shown.len(), 3);
        assert_eq!(shown[1].value.as_deref(), Some("2 kB"));
        assert_eq!(shown[2].label, "Drops");
    }
}
