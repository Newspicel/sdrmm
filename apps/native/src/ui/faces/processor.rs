use std::time::{Duration, SystemTime};

use sdrmm_wire::{
    array::{ArrayStatus, ProcessorGate, ProcessorStatus},
    frame::FrameKind,
    patch::{NodeBody, PatchGraph},
    processor::ProcessorReading,
    ws::{ClientCommand, SurfaceFit},
};
use zgui::prelude::*;

use crate::{
    arrays::{Heard, processor_of},
    bus::Source,
    head::{self, NodeHead},
    store::Store,
    ui::{kit_audio::titled, kit_raster::edit_body},
};

pub const NO_ARRAY: &str = "no array";
pub const STALE: &str = "stale";
const STALE_FLOOR: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_secs(1);

#[must_use]
pub fn is_stale(age: Duration, period_ms: u32) -> bool {
    age > (Duration::from_millis(u64::from(period_ms)) * 3).max(STALE_FLOOR)
}

#[must_use]
pub fn age_label(age: Option<Duration>) -> String {
    let Some(age) = age else {
        return "-".to_owned();
    };
    let seconds = age.as_secs_f64();
    if seconds < 10.0 {
        format!("{seconds:.1} s")
    } else if seconds < 60.0 {
        format!("{} s", seconds.floor())
    } else {
        format!("{} min", (seconds / 60.0).floor())
    }
}

#[must_use]
pub fn count(value: u64) -> String {
    let scaled = [(1e9, "B"), (1e6, "M"), (1e3, "K")]
        .into_iter()
        .find(|(step, _)| value as f64 >= *step);
    match scaled {
        Some((step, suffix)) => {
            let text = format!("{:.1}", (value as f64 / step * 10.0).floor() / 10.0);
            format!("{}{suffix}", text.trim_end_matches(".0"))
        }
        None => value.to_string(),
    }
}

#[must_use]
pub fn lanes_of(graph: &PatchGraph, array: &str, status: Option<&ArrayStatus>) -> usize {
    status.map_or_else(
        || graph.array_wired_lanes(array) as usize,
        |status| status.lanes.len(),
    )
}

#[must_use]
pub fn subtitle(
    array: Option<&str>,
    gate: Option<ProcessorGate>,
    stale: bool,
    lanes: usize,
) -> String {
    if array.is_none() {
        return NO_ARRAY.to_owned();
    }
    if let Some(gate) = gate {
        return gate.label().to_lowercase();
    }
    if stale {
        return STALE.to_owned();
    }
    format!("{lanes} {}", if lanes == 1 { "lane" } else { "lanes" })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub label: &'static str,
    pub count: u64,
    pub title: &'static str,
}

#[must_use]
pub fn faults(status: Option<&ProcessorStatus>) -> Vec<Fault> {
    let Some(status) = status else {
        return Vec::new();
    };
    [
        ("Drops", status.dropped_samples, "Samples dropped"),
        ("Lost", status.dropped_reports, "Reports lost"),
        (
            "Cut",
            status.truncated.saturating_add(status.lane_overflows),
            "Results cut at their limit",
        ),
        ("Fails", status.solver_failures, "Solver failures"),
        (
            "Mismatch",
            status.lane_mismatch,
            "Blocks with the wrong lane count",
        ),
    ]
    .into_iter()
    .filter(|(_, count, _)| *count > 0)
    .map(|(label, count, title)| Fault {
        label,
        count,
        title,
    })
    .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub label: String,
    pub title: String,
    pub danger: bool,
}

impl Chip {
    #[must_use]
    pub fn warn(label: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            title: title.into(),
            danger: false,
        }
    }

    #[must_use]
    pub fn danger(label: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            title: title.into(),
            danger: true,
        }
    }
}

#[derive(Clone)]
pub struct Processor {
    pub store: Store,
    pub node: String,
    pub array: Memo<Option<String>>,
    pub status: Memo<Option<ArrayStatus>>,
    pub health: Memo<Option<ProcessorStatus>>,
    pub heard: Memo<Option<Heard>>,
    pub now: RwSignal<SystemTime>,
}

impl Processor {
    pub fn new(store: Store, node: String) -> Self {
        install_stylesheet("processor-face", SHEET);
        let array = {
            let node = node.clone();
            Memo::new(move |_| {
                store
                    .graph
                    .with(|graph| graph.array_of_processor(&node).map(str::to_owned))
            })
        };
        let status = Memo::new(move |_| {
            let array = array.get()?;
            let seeded = store.state.get();
            store
                .arrays
                .with(|book| book.array(&array, &seeded.arrays).cloned())
        });
        let health = {
            let node = node.clone();
            Memo::new(move |_| status.with(|status| processor_of(status.as_ref(), &node).cloned()))
        };
        let heard = {
            let node = node.clone();
            Memo::new(move |_| store.arrays.with(|book| book.readings.get(&node).cloned()))
        };
        let now = RwSignal::new(SystemTime::now());
        let tick = set_interval(TICK, move || now.set(SystemTime::now()));
        on_cleanup_local(move || drop(tick));
        Self {
            store,
            node,
            array,
            status,
            health,
            heard,
            now,
        }
    }

    pub fn reading<T>(&self, pick: fn(&ProcessorReading) -> Option<&T>) -> Memo<Option<T>>
    where
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        let heard = self.heard;
        Memo::new(move |_| {
            heard.with(|heard| heard.as_ref().and_then(|held| pick(&held.reading).cloned()))
        })
    }

    #[must_use]
    pub fn age(&self) -> Signal<Option<Duration>> {
        let (heard, now) = (self.heard, self.now);
        Signal::derive(move || {
            let at = heard.with(|heard| heard.as_ref().map(|held| held.at))?;
            Some(now.get().duration_since(at).unwrap_or_default())
        })
    }

    #[must_use]
    pub fn stale(&self, period_ms: Signal<u32>) -> Signal<bool> {
        let age = self.age();
        Signal::derive(move || age.get().is_some_and(|age| is_stale(age, period_ms.get())))
    }

    #[must_use]
    pub fn gate(&self) -> Signal<Option<ProcessorGate>> {
        let health = self.health;
        Signal::derive(move || health.with(|health| health.as_ref().and_then(|found| found.gated)))
    }

    #[must_use]
    pub fn lanes(&self) -> Signal<usize> {
        let (store, array, status) = (self.store, self.array, self.status);
        Signal::derive(move || {
            array.get().map_or(0, |array| {
                store
                    .graph
                    .with(|graph| status.with(|status| lanes_of(graph, &array, status.as_ref())))
            })
        })
    }

    #[must_use]
    pub fn subtitle(&self, period_ms: Signal<u32>) -> Signal<String> {
        let (array, gate, stale, lanes) =
            (self.array, self.gate(), self.stale(period_ms), self.lanes());
        Signal::derive(move || {
            subtitle(array.get().as_deref(), gate.get(), stale.get(), lanes.get())
        })
    }
}

pub fn settings<T>(store: Store, node: String, read: fn(&NodeBody) -> Option<&T>) -> Memo<Option<T>>
where
    T: Clone + PartialEq + Send + Sync + 'static,
{
    Memo::new(move |_| {
        store.graph.with(|graph| {
            graph
                .node(&node)
                .and_then(|found| read(&found.body).cloned())
        })
    })
}

pub fn edit<T>(
    store: Store,
    node: &str,
    write: fn(&mut NodeBody) -> Option<&mut T>,
    change: impl FnOnce(&mut T),
) {
    edit_body(store, node, |body| {
        if let Some(settings) = write(body) {
            change(settings);
        }
    });
}

pub fn surface(
    store: Store,
    node: String,
    kind: FrameKind,
    fit: Option<SurfaceFit>,
    show: impl Fn(&[u8]) + 'static,
) {
    store.hold(ClientCommand::SubscribeSurface {
        node: node.clone(),
        fit,
    });
    store.on_frame(move |frame| {
        if frame.kind != kind {
            return;
        }
        let ours = matches!(
            store.source_of(frame.stream_id),
            Some(Source::Surface { node: ref from, .. }) if *from == node
        );
        if ours {
            show(&frame.bytes);
        }
    });
}

pub fn head(store: Store, node: String, subtitle: Signal<String>) {
    head::show(store, node, move || NodeHead::said(subtitle.get()));
}

pub fn error(health: Memo<Option<ProcessorStatus>>) -> impl IntoView {
    move || {
        health
            .get()
            .and_then(|found| found.error)
            .map(|message| view! { text(class = "kit-alert") {{message}} })
    }
}

pub fn hint(text: &'static str) -> impl IntoView {
    view! { text(class = "kit-note pf__hint") {{text}} }
}

fn stat(label: String, value: Option<String>, title: String, danger: bool) -> AnyView {
    titled(
        title,
        AnyView::new(view! {
            row(class = "pf__stat", class:danger = danger) {
                text(class = "pf__stat_label") {{label}}
                {value.map(|value| view! { text(class = "pf__stat_value") {{value}} })}
            }
        }),
    )
}

fn shown_stats(health: Option<&ProcessorStatus>, chips: Vec<Chip>) -> Vec<AnyView> {
    let mut shown: Vec<AnyView> = chips
        .into_iter()
        .map(|chip| stat(chip.label, None, chip.title, chip.danger))
        .collect();
    shown.extend(faults(health).into_iter().map(|fault| {
        stat(
            fault.label.to_owned(),
            Some(count(fault.count)),
            fault.title.to_owned(),
            true,
        )
    }));
    shown
}

pub fn footer(
    health: Memo<Option<ProcessorStatus>>,
    chips: impl Fn() -> Vec<Chip> + 'static,
) -> impl IntoView {
    footer_with(health, chips, || None)
}

pub fn footer_with(
    health: Memo<Option<ProcessorStatus>>,
    chips: impl Fn() -> Vec<Chip> + 'static,
    actions: impl Fn() -> Option<AnyView> + 'static,
) -> impl IntoView {
    move || {
        let shown = health.with(|health| shown_stats(health.as_ref(), chips()));
        let actions = actions();
        (!shown.is_empty() || actions.is_some()).then(|| {
            view! {
                row(class = "pf__stats") {
                    row(class = "pf__chips") {{shown}}
                    {actions}
                }
            }
        })
    }
}

pub(crate) const SHEET: &str = css!(
    r#"
.pf__hint { align-self: center; padding: 8px 0; }
.pf__stats { align-items: center; gap: 6px; padding-top: 6px; border-top: 1px solid var(--line); }
.pf__chips { flex: 1 1 auto; min-width: 0; flex-wrap: wrap; gap: 6px; }
.pf__stat {
    align-items: baseline;
    gap: 4px;
    padding: 1px 6px;
    border-radius: 3px;
    border: 1px solid color-mix(in oklab, oklch(0.8 0.14 85) 45%, transparent);
    color: oklch(0.8 0.14 85);
    font-family: var(--mono);
    font-size: 10px;
}
.pf__stat.danger { border-color: color-mix(in oklab, var(--danger) 45%, transparent); color: var(--danger); }
.pf__stat_label { letter-spacing: 0.06em; text-transform: uppercase; }
.pf__stat_value { color: var(--ink); }
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_goes_stale_after_three_periods_but_never_under_two_seconds() {
        assert!(!is_stale(Duration::from_millis(1_500), 100));
        assert!(is_stale(Duration::from_millis(2_100), 100));
        assert!(!is_stale(Duration::from_millis(2_900), 1_000));
        assert!(is_stale(Duration::from_millis(3_100), 1_000));
    }

    #[test]
    fn the_age_reads_in_tenths_then_seconds_then_minutes() {
        assert_eq!(age_label(None), "-");
        assert_eq!(age_label(Some(Duration::from_millis(1_250))), "1.2 s");
        assert_eq!(age_label(Some(Duration::from_secs(42))), "42 s");
        assert_eq!(age_label(Some(Duration::from_secs(150))), "2 min");
    }

    #[test]
    fn the_subtitle_says_the_first_thing_that_holds_the_processor_back() {
        assert_eq!(subtitle(None, None, false, 4), NO_ARRAY);
        assert_eq!(
            subtitle(Some("bench"), Some(ProcessorGate::Phase), true, 4),
            "needs cal"
        );
        assert_eq!(subtitle(Some("bench"), None, true, 4), STALE);
        assert_eq!(subtitle(Some("bench"), None, false, 1), "1 lane");
        assert_eq!(subtitle(Some("bench"), None, false, 4), "4 lanes");
    }

    #[test]
    fn only_counters_above_zero_become_faults() {
        assert!(faults(None).is_empty());
        let status = ProcessorStatus {
            dropped_samples: 3,
            truncated: 1,
            lane_overflows: 2,
            ..ProcessorStatus::default()
        };
        let found = faults(Some(&status));
        assert_eq!(
            found
                .iter()
                .map(|fault| (fault.label, fault.count))
                .collect::<Vec<_>>(),
            vec![("Drops", 3), ("Cut", 3)]
        );
    }

    #[test]
    fn counts_read_short() {
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1K");
        assert_eq!(count(12_345), "12.3K");
        assert_eq!(count(2_500_000), "2.5M");
        assert_eq!(count(3_000_000_000), "3B");
    }
}
