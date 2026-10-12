use std::time::Duration;

use zgui_ui::prelude::*;

use sdrmm_wire::{
    channel::ChannelInfo,
    patch::NodeBody,
    state::{AudioRecordingStatus, RecordingStatus},
};
use zgui::prelude::*;

use crate::store::Store;

pub const DROPS_HINT: &str = "Samples lost between the radio and here";
pub const IDLE: &str = "-";
const TIP_DELAY: Duration = Duration::from_millis(250);
const BYTE_DECIMALS: usize = 1;
const SI_DECIMALS: usize = 9;
const COMPACT: [(f64, &str); 4] = [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")];
const PREFIXES: [(f64, &str); 4] = [(1e9, "G"), (1e6, "M"), (1e3, "k"), (1.0, "")];

const SHEET: &str = css!(
    r#"
.btn.danger { border-color: var(--danger); color: var(--danger); }
.btn.primary { border-color: var(--accent); color: var(--accent); }
.btn:disabled { opacity: 0.45; }
.btn__dot { color: var(--danger); }
.stats { flex: 1 1 auto; min-width: 0; flex-direction: row; flex-wrap: wrap; align-items: center; column-gap: 12px; row-gap: 2px; }
.stat { flex-direction: row; gap: 4px; align-items: baseline; font-family: var(--mono); font-size: 11px; color: var(--ink-faint); white-space: nowrap; }
.stat__value { color: var(--ink); font-weight: 500; }
.stat.warn, .stat.warn .stat__value { color: oklch(0.8 0.14 85); }
.stat.danger, .stat.danger .stat__value { color: var(--danger); }
.rec-foot { gap: 8px; }
.zui-tooltip__trigger.tip-block { display: flex; flex-direction: column; align-items: stretch; align-self: stretch; min-width: 0; }
.zui-tooltip__trigger.tip-fill { display: flex; flex-direction: row; flex: 1 1 0; min-width: 0; }
.zui-tooltip__trigger.tip-fill > * { flex: 1 1 auto; min-width: 0; }
.readout { flex-direction: column; gap: 3px; }
.readout__row { flex-direction: row; gap: 8px; align-items: baseline; min-width: 0; }
.readout__label { flex: 0 0 76px; color: var(--ink-faint); font-size: 10px; text-transform: uppercase; }
.readout__value { flex: 1 1 auto; min-width: 0; font-family: var(--mono); font-size: 11px; color: var(--ink); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.alert { color: var(--danger); font-size: 11px; }
.chips { flex-direction: row; flex-wrap: wrap; gap: 4px; }
.chip { flex-direction: row; gap: 4px; padding: 1px 6px; border: 1px solid var(--line); border-radius: 4px; font-family: var(--mono); font-size: 10px; color: var(--ink-dim); }
.lane { flex-direction: column; gap: 6px; padding-bottom: 8px; border-bottom: 1px solid var(--line); }
.lane:last-child { border-bottom-width: 0; padding-bottom: 0; }
.lane__head { flex-direction: row; gap: 8px; align-items: center; min-width: 0; }
.lane__name { flex: 1 1 auto; min-width: 0; font-size: 10px; text-transform: uppercase; color: var(--ink-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.transport { flex-direction: row; gap: 6px; align-items: center; }
"#
);

pub fn install() {
    install_stylesheet("kit_audio", SHEET);
}

#[must_use]
pub fn si(value: f64, unit: &str) -> String {
    si_to(value, unit, SI_DECIMALS)
}

#[must_use]
pub fn si_to(value: f64, unit: &str, decimals: usize) -> String {
    if !value.is_finite() {
        return format!("? {unit}");
    }
    let (scale, prefix) = PREFIXES
        .iter()
        .copied()
        .find(|(step, _)| value.abs() >= *step)
        .unwrap_or((1.0, ""));
    let fixed = format!("{:.decimals$}", value / scale);
    let trimmed = if fixed.contains('.') {
        fixed.trim_end_matches('0').trim_end_matches('.')
    } else {
        &fixed
    };
    format!("{trimmed} {prefix}{unit}")
}

#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    si_to(bytes as f64, "B", BYTE_DECIMALS)
}

#[must_use]
pub fn format_count(count: u64) -> String {
    let value = count as f64;
    let Some(at) = COMPACT.iter().position(|(step, _)| value >= *step) else {
        return count.to_string();
    };
    let mut at = at;
    let mut shown = (value / COMPACT[at].0 * 10.0).round() / 10.0;
    if shown >= 1_000.0 && at > 0 {
        at -= 1;
        shown = (value / COMPACT[at].0 * 10.0).round() / 10.0;
    }
    let text = format!("{shown:.1}");
    format!("{}{}", text.trim_end_matches(".0"), COMPACT[at].1)
}

#[must_use]
pub fn format_duration(seconds: f64) -> String {
    let tenths = (seconds * 10.0).round() / 10.0;
    if tenths < 60.0 {
        return format!("{tenths:.1} s");
    }
    format_clock(tenths.round())
}

#[must_use]
pub fn format_clock(seconds: f64) -> String {
    let whole = seconds.max(0.0).floor() as u64;
    let (hours, minutes, secs) = (whole / 3_600, (whole % 3_600) / 60, whole % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

#[must_use]
pub fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

#[must_use]
pub fn recording_elapsed_s(status: &RecordingStatus, now_ms: i64, sample_rate: f64) -> f64 {
    if status.error.is_some() {
        return if sample_rate > 0.0 {
            status.samples as f64 / sample_rate
        } else {
            0.0
        };
    }
    status
        .started_at
        .parse::<jiff::Timestamp>()
        .map_or(0.0, |started| {
            ((now_ms - started.as_millisecond()) as f64 / 1_000.0).max(0.0)
        })
}

#[must_use]
pub fn recording_for<'a>(
    channel: &'a ChannelInfo,
    fx: &[String],
) -> Option<&'a AudioRecordingStatus> {
    channel
        .audio_recordings
        .iter()
        .find(|status| status.fx == fx)
}

pub fn ticker(every: Duration) -> RwSignal<i64> {
    let now = RwSignal::new(now_ms());
    let ticking = set_interval(every, move || now.set(now_ms()));
    on_cleanup_local(move || drop(ticking));
    now
}

pub fn edit_body(store: Store, node: &str, change: impl FnOnce(&mut NodeBody)) {
    let node = node.to_owned();
    store.edit_graph(move |graph| {
        if let Some(found) = graph.nodes.iter_mut().find(|found| found.id == node) {
            change(&mut found.body);
        }
    });
}

pub fn node_label(store: Store, node: &str, fallback: &str) -> String {
    store
        .graph
        .get()
        .node(node)
        .and_then(|found| found.label.clone())
        .unwrap_or_else(|| fallback.to_uppercase())
}

pub fn button(
    label: impl Fn() -> String + 'static,
    danger: Signal<bool>,
    disabled: Signal<bool>,
    on_press: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        control(
            class = "btn",
            class:danger = danger,
            state:disabled = disabled,
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            on:pointer_down:stop = move |_| {},
            on:click:stop = move |_| on_press()
        ) {
            {label}
        }
    }
}

pub fn readout(rows: Vec<(&'static str, AnyView)>) -> impl IntoView {
    let rows: Vec<_> = rows
        .into_iter()
        .map(|(label, value)| {
            view! {
                row(class = "readout__row") {
                    text(class = "readout__label") {{label}}
                    box(class = "readout__value") {{value}}
                }
            }
        })
        .collect();
    view! { column(class = "readout") {{rows}} }
}

pub fn alert(message: impl Fn() -> Option<String> + 'static) -> impl IntoView {
    move || {
        message().map(|said| {
            view! {
                text(class = "alert") {{said}}
            }
        })
    }
}

pub fn fault(message: String, detail: Option<String>) -> AnyView {
    let shown = AnyView::new(view! {
        text(class = "alert", a11y:role = Role::Alert) {{message}}
    });
    match detail {
        Some(detail) => titled(detail, shown),
        None => shown,
    }
}

fn tipped(class: &'static str, title: String, child: AnyView) -> AnyView {
    let title = Signal::stored(title);
    AnyView::new(view! {
        Tooltip(delay = TIP_DELAY) {
            TooltipTrigger(class = class) {{child}}
            TooltipContent {{move || title.get()}}
        }
    })
}

pub fn titled(title: impl Into<String>, child: AnyView) -> AnyView {
    tipped("", title.into(), child)
}

pub fn titled_block(title: impl Into<String>, child: AnyView) -> AnyView {
    tipped("tip-block", title.into(), child)
}

pub fn titled_fill(title: impl Into<String>, child: AnyView) -> AnyView {
    tipped("tip-fill", title.into(), child)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Plain,
    Warn,
    Danger,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub label: String,
    pub value: Option<String>,
    pub title: String,
    pub tone: Tone,
}

impl Stat {
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        value: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            value: Some(value.into()),
            title: title.into(),
            tone: Tone::Plain,
        }
    }

    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

#[must_use]
pub fn drops_stat(drops: u64) -> Option<Stat> {
    (drops > 0).then(|| Stat::new("Drops", format_count(drops), DROPS_HINT).tone(Tone::Warn))
}

fn stat_view(stat: Stat) -> AnyView {
    let Stat {
        label,
        value,
        title,
        tone,
    } = stat;
    let shown = AnyView::new(view! {
        row(
            class = "stat",
            class:warn = tone == Tone::Warn,
            class:danger = tone == Tone::Danger
        ) {
            text {{label}}
            {value.map(|value| view! { text(class = "stat__value") {{value}} })}
        }
    });
    titled(title, shown)
}

pub fn stats(items: impl Fn() -> Vec<Stat> + 'static) -> impl IntoView {
    view! {
        row(class = "stats") {
            {move || items().into_iter().map(stat_view).collect::<Vec<_>>()}
        }
    }
}

pub struct Line {
    pub label: String,
    pub value: String,
    pub title: Option<String>,
}

pub fn lines(rows: impl Fn() -> Vec<Line> + 'static) -> impl IntoView {
    let shown = move || {
        rows()
            .into_iter()
            .map(|line| {
                let value = AnyView::new(view! { text {{line.value}} });
                let value = match line.title {
                    Some(title) => titled(title, value),
                    None => value,
                };
                view! {
                    row(class = "readout__row") {
                        text(class = "readout__label") {{line.label}}
                        box(class = "readout__value") {{value}}
                    }
                }
            })
            .collect::<Vec<_>>()
    };
    view! { column(class = "readout") {{shown}} }
}

#[must_use]
pub fn take_line(name: String, file: Option<&str>, recording: bool) -> Line {
    match file {
        Some(file) => Line {
            label: name,
            value: file.to_owned(),
            title: Some(file.to_owned()),
        },
        None => Line {
            label: name,
            value: String::from(if recording { "Waiting" } else { IDLE }),
            title: None,
        },
    }
}

#[must_use]
pub fn idle_file_line() -> Line {
    Line {
        label: String::from("File"),
        value: String::from(IDLE),
        title: None,
    }
}

#[must_use]
pub fn take_fault(name: &str, error: Option<&str>) -> Option<String> {
    error.map(|error| format!("{name}: {error}"))
}

#[must_use]
pub fn time_stat(seconds: Option<f64>) -> Stat {
    Stat::new(
        "Time",
        seconds.map_or_else(|| IDLE.to_owned(), format_duration),
        "Recording length",
    )
}

#[must_use]
pub fn written_stat(bytes: Option<u64>) -> Stat {
    Stat::new(
        "Written",
        bytes.map_or_else(|| IDLE.to_owned(), format_bytes),
        "Bytes on disk",
    )
}

#[must_use]
pub fn samples_stat(samples: Option<u64>) -> Stat {
    Stat::new(
        "Samples",
        samples.map_or_else(|| IDLE.to_owned(), format_count),
        "Samples written",
    )
}

pub fn faults(messages: impl Fn() -> Vec<String> + 'static) -> impl IntoView {
    move || {
        messages()
            .into_iter()
            .map(|said| view! { text(class = "alert") {{said}} })
            .collect::<Vec<_>>()
    }
}

pub fn is_recording(store: Store, node: String) -> Signal<bool> {
    Signal::derive(move || {
        store
            .graph
            .get()
            .node(&node)
            .is_some_and(|found| match &found.body {
                NodeBody::Recorder(body) | NodeBody::BasebandRecorder(body) => body.recording,
                NodeBody::AudioRecorder(body) => body.recording,
                _ => false,
            })
    })
}

pub fn recorder_switch(
    store: Store,
    node: String,
    title: &'static str,
    unwired: Signal<Option<&'static str>>,
) -> impl IntoView {
    let recording = is_recording(store, node.clone());
    let disabled = Signal::derive(move || unwired.get().is_some() && !recording.get());
    let press = move || {
        let next = !recording.get_untracked();
        edit_body(store, &node, |body| match body {
            NodeBody::Recorder(recorder) | NodeBody::BasebandRecorder(recorder) => {
                recorder.recording = next;
            }
            NodeBody::AudioRecorder(recorder) => recorder.recording = next,
            _ => {}
        });
    };
    let press = std::rc::Rc::new(press);
    move || {
        let press = press.clone();
        let switch = AnyView::new(view! {
            control(
                class = "btn",
                class:danger = recording,
                state:disabled = disabled,
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                on:pointer_down:stop = move |_| {},
                on:click:stop = move |_| {
                    if !disabled.get_untracked() {
                        press();
                    }
                }
            ) {
                {move || if recording.get() {
                    AnyView::new("Stop")
                } else {
                    AnyView::new(view! {
                        row(class = "transport") {
                            text(class = "btn__dot") {"\u{25cf}"}
                            text {"Record"}
                        }
                    })
                }}
            }
        });
        if recording.get() {
            switch
        } else {
            titled(unwired.get().unwrap_or(title), switch)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording(started_at: &str, samples: u64, error: Option<&str>) -> RecordingStatus {
        RecordingStatus {
            file: String::from("a.sigmf"),
            stream: 0,
            started_at: started_at.to_owned(),
            samples,
            bytes: 0,
            overruns: 0,
            error: error.map(str::to_owned),
        }
    }

    #[test]
    fn sizes_read_in_the_unit_they_belong_in() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1_536), "1.5 kB");
        assert_eq!(format_bytes(12_000_000), "12 MB");
        assert_eq!(si(f64::NAN, "B"), "? B");
        assert_eq!(si(1_536.0, "Hz"), "1.536 kHz");
    }

    #[test]
    fn counts_read_compact_with_one_decimal() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_234), "1.2K");
        assert_eq!(format_count(1_250_000), "1.3M");
        assert_eq!(format_count(2_000_000_000), "2B");
        assert_eq!(format_count(999_960), "1M");
    }

    #[test]
    fn drops_show_only_when_some_were_lost() {
        assert_eq!(drops_stat(0), None);
        let shown = drops_stat(1_500).expect("a drops stat");
        assert_eq!(shown.value.as_deref(), Some("1.5K"));
        assert_eq!(shown.tone, Tone::Warn);
        assert_eq!(shown.title, DROPS_HINT);
    }

    #[test]
    fn durations_read_seconds_then_a_clock() {
        assert_eq!(format_duration(9.94), "9.9 s");
        assert_eq!(format_duration(75.0), "1:15");
        assert_eq!(format_duration(3_725.0), "1:02:05");
    }

    #[test]
    fn a_clock_is_fixed_width_grows_hours_only_when_needed_and_never_reads_negative() {
        assert_eq!(format_clock(0.0), "0:00");
        assert_eq!(format_clock(9.9), "0:09");
        assert_eq!(format_clock(64.0), "1:04");
        assert_eq!(format_clock(599.0), "9:59");
        assert_eq!(format_clock(3_600.0), "1:00:00");
        assert_eq!(format_clock(3_725.0), "1:02:05");
        assert_eq!(format_clock(-5.0), "0:00");
    }

    #[test]
    fn a_recording_counts_the_wall_clock_until_it_faults() {
        let now = "2026-09-23T12:00:10Z"
            .parse::<jiff::Timestamp>()
            .map(|at| at.as_millisecond())
            .unwrap_or_default();
        let running = recording("2026-09-23T12:00:00Z", 0, None);
        assert!((recording_elapsed_s(&running, now, 48_000.0) - 10.0).abs() < 1e-9);
        let faulted = recording("2026-09-23T12:00:00Z", 96_000, Some("disk full"));
        assert!((recording_elapsed_s(&faulted, now, 48_000.0) - 2.0).abs() < 1e-9);
        assert!(recording_elapsed_s(&recording("garbage", 0, None), now, 48_000.0).abs() < 1e-9);
    }

    #[test]
    fn an_audio_recording_is_found_by_its_exact_route() {
        let status = |file: &str, fx: &[&str]| AudioRecordingStatus {
            fx: fx.iter().map(|node| (*node).to_owned()).collect(),
            file: file.to_owned(),
            started_at: String::from("2026-09-23T12:00:00Z"),
            channels: 1,
            frames: 0,
            bytes: 0,
            error: None,
        };
        let mut channel: ChannelInfo = serde_json::from_value(serde_json::json!({
            "id": 1,
            "settings": { "frequency_hz": 1.0, "params": { "type": "nfm", "settings": {} } }
        }))
        .expect("a channel");
        channel.audio_recordings = vec![status("raw.wav", &[]), status("clean.wav", &["a", "b"])];
        let route = |fx: &[&str]| fx.iter().map(|node| (*node).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            recording_for(&channel, &route(&[])).map(|s| s.file.as_str()),
            Some("raw.wav")
        );
        assert_eq!(
            recording_for(&channel, &route(&["a", "b"])).map(|s| s.file.as_str()),
            Some("clean.wav")
        );
        assert!(recording_for(&channel, &route(&["b", "a"])).is_none());
        channel.audio_recordings.clear();
        assert!(recording_for(&channel, &route(&[])).is_none());
    }
}
