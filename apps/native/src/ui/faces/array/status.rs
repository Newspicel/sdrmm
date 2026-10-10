use std::{future::Future, time::Instant};

use sdrmm_wire::array::{
    ArrayRecordingRequest, ArrayRecordingStarted, ArrayStatus, ArrayTuneRequest, CalPhase,
};
use zgui::{prelude::*, reactive::RenderEffect, vocab::SharedString};

use super::{
    Array,
    geometry::aliasing,
    labels::{
        cal_label, cal_title, failure_title, heading_label, lane_gaps, recording_label, sync_label,
        tier_label,
    },
    tune::{PendingTune, confirm, retune, settle, shown_center},
};
use crate::{
    store::Store,
    ui::{
        faces::processor::count,
        kit_sources::{
            DialSpec, Tone, button,
            dial::{ANY_FREQUENCY, Reach, in_tuning_range, tuning_range},
            footer as foot, frequency_dial, tune_to, units,
        },
    },
};

pub const ALIASING: &str = "Aliasing";
pub const ALIASING_TITLE: &str = "Elements more than half a wavelength apart";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub label: &'static str,
    pub value: Option<String>,
    pub title: String,
    pub tone: Option<&'static str>,
}

fn warn(label: &'static str, value: u64, title: &str) -> Option<Stat> {
    (value > 0).then(|| Stat {
        label,
        value: Some(count(value)),
        title: title.to_owned(),
        tone: Some("warn"),
    })
}

#[must_use]
pub fn health(status: Option<&ArrayStatus>, aliased: bool) -> Vec<Stat> {
    let mut stats = Vec::new();
    if let Some(status) = status {
        stats.extend(status.recording.as_ref().map(|recording| {
            Stat {
                label: "Rec",
                value: Some(recording_label(recording, status.sample_rate)),
                title: recording
                    .error
                    .clone()
                    .unwrap_or_else(|| recording.stem.clone()),
                tone: (recording.dropped > 0 || recording.error.is_some()).then_some("danger"),
            }
        }));
        stats.extend(
            [
                warn("Gaps", lane_gaps(status), "Samples lost and resynced"),
                warn("Realigns", status.realigns, "Lanes lined up again"),
                warn("Drops", status.dropped_samples, "Samples dropped"),
                warn("Lost", status.events_lost, "Lane events lost"),
            ]
            .into_iter()
            .flatten(),
        );
    }
    if aliased {
        stats.push(Stat {
            label: ALIASING,
            value: None,
            title: ALIASING_TITLE.to_owned(),
            tone: Some("danger"),
        });
    }
    stats
}

#[must_use]
pub fn uncalibrated(status: Option<&ArrayStatus>) -> bool {
    status.is_none_or(|status| matches!(status.cal, CalPhase::None | CalPhase::Failed))
}

#[derive(Clone)]
struct Tuner {
    store: Store,
    node: String,
    reported: Memo<Option<f64>>,
    pending: RwSignal<Option<PendingTune>>,
    waiting: RwSignal<Option<f64>>,
    busy: RwSignal<bool>,
}

impl Tuner {
    fn tune(&self, hz: f64) {
        let reported = self.reported.get_untracked();
        self.pending
            .update(|pending| *pending = Some(retune(pending.take(), reported, hz)));
        self.waiting.set(Some(hz));
        if !self.busy.get_untracked() {
            self.busy.set(true);
            zgui::task::spawn_local(self.clone().drain());
        }
    }

    async fn drain(self) {
        while let Some(hz) = self.waiting.try_update(Option::take).flatten() {
            let path = format!("/api/arrays/{}/tune", self.node);
            let body = ArrayTuneRequest {
                center_hz: Some(hz),
                gain: None,
            };
            let sent = self
                .store
                .api()
                .patch::<_, serde::de::IgnoredAny>(&path, &body)
                .await;
            if let Err(error) = &sent {
                self.store.say(format!("Cannot tune the array: {error:#}"));
            }
            if self.waiting.try_with_untracked(Option::is_none) != Some(false) {
                let reported = self.reported.try_get_untracked().flatten();
                let tuned = sent.is_ok().then_some(hz);
                self.pending.try_update(|pending| {
                    *pending = settle(pending.take(), reported, tuned, Instant::now());
                });
            }
        }
        self.busy.try_set(false);
    }

    fn confirm(&self) -> RenderEffect<()> {
        let (reported, pending) = (self.reported, self.pending);
        RenderEffect::new(move |_| {
            let Some(center) = reported.get() else {
                return;
            };
            let held = pending.get_untracked();
            let next = confirm(held.clone(), center, Instant::now());
            if next != held {
                pending.set(next);
            }
        })
    }
}

fn lead_reach(cx: &Array) -> Signal<Reach> {
    let (members, sets) = (cx.members, cx.sets);
    Signal::derive(move || {
        let lead = members.with(|members| members.first().cloned());
        lead.and_then(|lead| {
            sets.with(|sets| sets.get(&lead).map(|set| tuning_range(&set.capabilities)))
        })
        .unwrap_or(ANY_FREQUENCY)
    })
}

pub fn dial(cx: &Array) -> impl IntoView {
    let status = cx.status;
    let tuner = Tuner {
        store: cx.store,
        node: cx.node.clone(),
        reported: Memo::new(move |_| {
            status.with(|status| status.as_ref().map(|found| found.center_hz))
        }),
        pending: RwSignal::new(None),
        waiting: RwSignal::new(None),
        busy: RwSignal::new(false),
    };
    let effect = tuner.confirm();
    on_cleanup_local(move || drop(effect));
    let (pending, reported) = (tuner.pending, tuner.reported);
    let hz = Signal::derive(move || {
        pending
            .with(|pending| shown_center(pending.as_ref(), reported.get()))
            .unwrap_or(0.0)
    });
    let reach = lead_reach(cx);
    let held = Signal::derive(move || status.with(Option::is_none));
    let (store, node) = (cx.store, cx.node.clone());
    let wheel = Signal::derive(move || store.selected.get().as_deref() == Some(node.as_str()));
    let hint = Signal::derive(move || {
        let reach = reach.get();
        format!(
            "Reaches {} to {}",
            units::mhz(reach.min),
            units::mhz(reach.max)
        )
    });
    let tune = move |value: f64| tuner.tune(value);
    let spec = DialSpec {
        hz,
        reach,
        disabled: held,
        wheel,
    };
    view! {
        row(class = "kit-dial-row") {
            {frequency_dial(spec, tune.clone())}
            spacer() {}
            row(class = "kit-tools") {
                {tune_to("Type a frequency", hz, hint, move |entered| in_tuning_range(entered, reach.get_untracked()), held, tune)}
            }
        }
    }
}

fn read_cell(label: &'static str, title: String, value: String, danger: bool) -> AnyView {
    AnyView::new(view! {
        row(class = "af__read", a11y:description = SharedString::from(title)) {
            text(class = "af__read_name") {{label}}
            text(class = "af__read_value", class:danger = danger) {{value}}
        }
    })
}

pub fn readouts(cx: &Array) -> impl IntoView {
    let (status, settings, now) = (cx.status, cx.settings, cx.now);
    move || {
        let status = status.get()?;
        let orientation = settings.with(|settings| settings.orientation);
        let cells = vec![
            read_cell(
                "Sync",
                "Lanes lined up in time".to_owned(),
                sync_label(&status),
                false,
            ),
            read_cell(
                "Tier",
                "Capped: measured drift allows less than declared".to_owned(),
                tier_label(&status),
                false,
            ),
            read_cell(
                "Cal",
                cal_title(&status).unwrap_or_default(),
                cal_label(&status, now.get()),
                status.cal == CalPhase::Failed,
            ),
            read_cell(
                "Heading",
                "Array forward, true north".to_owned(),
                heading_label(orientation, Some(&status)),
                false,
            ),
        ];
        Some(view! { row(class = "af__reads") {{cells}} })
    }
}

pub fn failure(cx: &Array) -> impl IntoView {
    let status = cx.status;
    move || {
        let failure =
            status.with(|status| status.as_ref().and_then(|found| found.failure.clone()))?;
        let title = SharedString::from(failure_title(&failure));
        Some(view! { text(class = "kit-alert", a11y:description = title) {{failure.to_string()}} })
    }
}

fn stat_view(stat: Stat) -> AnyView {
    AnyView::new(view! {
        row(
            class = "pf__stat",
            class:danger = stat.tone == Some("danger"),
            class:af__plain = stat.tone.is_none(),
            a11y:description = SharedString::from(stat.title)
        ) {
            text(class = "pf__stat_label") {{stat.label}}
            {stat.value.map(|value| view! { text(class = "pf__stat_value") {{value}} })}
        }
    })
}

fn run(
    store: Store,
    busy: RwSignal<bool>,
    failed: &'static str,
    work: impl Future<Output = anyhow::Result<()>> + 'static,
) {
    busy.set(true);
    zgui::task::spawn_local(async move {
        if let Err(error) = work.await {
            store.say(format!("{failed}: {error:#}"));
        }
        busy.try_set(false);
    });
}

fn record(store: Store, node: String, busy: RwSignal<bool>) {
    run(store, busy, "Cannot record the array", async move {
        let path = format!("/api/arrays/{node}/recording");
        store
            .api()
            .post::<_, ArrayRecordingStarted>(&path, &ArrayRecordingRequest::default())
            .await
            .map(drop)
    });
}

fn stop(store: Store, node: String, busy: RwSignal<bool>) {
    run(store, busy, "Cannot stop the recording", async move {
        store
            .api()
            .delete(&format!("/api/arrays/{node}/recording"))
            .await
    });
}

fn calibrate(store: Store, node: String, busy: RwSignal<bool>) {
    run(store, busy, "Cannot calibrate", async move {
        store
            .api()
            .post_empty::<serde::de::IgnoredAny>(&format!("/api/arrays/{node}/calibrate"))
            .await
            .map(drop)
    });
}

fn record_button(cx: &Array) -> impl IntoView {
    let (store, status) = (cx.store, cx.status);
    let busy = RwSignal::new(false);
    let stem = Memo::new(move |_| {
        status.with(|status| {
            status.as_ref().and_then(|found| {
                found
                    .recording
                    .as_ref()
                    .map(|recording| recording.stem.clone())
            })
        })
    });
    let recording = Memo::new(move |_| stem.with(Option::is_some));
    let node = cx.node.clone();
    move || {
        let node = node.clone();
        if recording.get() {
            let title = SharedString::from(stem.get().unwrap_or_default());
            AnyView::new(view! {
                box(a11y:description = title) {
                    {button(|| "Stop".to_owned(), Tone::Plain, busy.into(), move || stop(store, node.clone(), busy))}
                }
            })
        } else {
            let off = Signal::derive(move || busy.get() || status.with(Option::is_none));
            AnyView::new(view! {
                box(a11y:description = SharedString::from("Record every lane to one collection")) {
                    {button(|| "Rec".to_owned(), Tone::Plain, off, move || record(store, node.clone(), busy))}
                }
            })
        }
    }
}

fn calibrate_button(cx: &Array) -> impl IntoView {
    let (store, status, rows) = (cx.store, cx.status, cx.rows);
    let busy = RwSignal::new(false);
    let primary = Memo::new(move |_| status.with(|status| uncalibrated(status.as_ref())));
    let off = Signal::derive(move || busy.get() || rows.with(Vec::is_empty));
    let node = cx.node.clone();
    move || {
        let tone = if primary.get() {
            Tone::Primary
        } else {
            Tone::Plain
        };
        let node = node.clone();
        view! {
            box(a11y:description = SharedString::from("Line up lane phase and gain")) {
                {button(|| "Calibrate".to_owned(), tone, off, move || calibrate(store, node.clone(), busy))}
            }
        }
    }
}

pub fn footer(cx: &Array) -> impl IntoView {
    let (status, settings, wired) = (cx.status, cx.settings, cx.wired);
    let stats = move || {
        let aliased = settings.with(|settings| {
            let center = status.with(|status| status.as_ref().map(|found| found.center_hz));
            aliasing(&settings.geometry, wired.get(), center)
        });
        let shown: Vec<AnyView> = status
            .with(|status| health(status.as_ref(), aliased))
            .into_iter()
            .map(stat_view)
            .collect();
        view! { row(class = "af__stats") {{shown}} }
    };
    foot(view! {
        {stats}
        spacer() {}
        {record_button(cx)}
        {calibrate_button(cx)}
    })
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::array::{ArrayLaneStatus, ArrayRecordingStatus};

    use super::*;

    fn labels(stats: &[Stat]) -> Vec<(&'static str, Option<&str>)> {
        stats
            .iter()
            .map(|stat| (stat.label, stat.value.as_deref()))
            .collect()
    }

    #[test]
    fn only_counters_above_zero_reach_the_footer() {
        assert!(health(None, false).is_empty());
        let status = ArrayStatus {
            lanes: vec![ArrayLaneStatus {
                gaps: 3,
                ..ArrayLaneStatus::default()
            }],
            dropped_samples: 1_500,
            ..ArrayStatus::default()
        };
        assert_eq!(
            labels(&health(Some(&status), false)),
            vec![("Gaps", Some("3")), ("Drops", Some("1.5K"))]
        );
    }

    #[test]
    fn a_recording_turns_danger_once_it_drops() {
        let running = ArrayStatus {
            sample_rate: 1_000.0,
            recording: Some(ArrayRecordingStatus {
                stem: "north-1".to_owned(),
                samples: 5_000,
                ..ArrayRecordingStatus::default()
            }),
            ..ArrayStatus::default()
        };
        let calm = health(Some(&running), false);
        assert_eq!(labels(&calm), vec![("Rec", Some("5 s"))]);
        assert_eq!(calm[0].tone, None);
        assert_eq!(calm[0].title, "north-1");
        let mut dropping = running;
        if let Some(recording) = dropping.recording.as_mut() {
            recording.dropped = 4;
        }
        assert_eq!(health(Some(&dropping), false)[0].tone, Some("danger"));
    }

    #[test]
    fn aliasing_shows_even_without_a_status() {
        let stats = health(None, true);
        assert_eq!(labels(&stats), vec![(ALIASING, None)]);
        assert_eq!(stats[0].tone, Some("danger"));
    }

    #[test]
    fn calibrate_stands_out_until_a_solve_holds() {
        assert!(uncalibrated(None));
        let failed = ArrayStatus {
            cal: CalPhase::Failed,
            ..ArrayStatus::default()
        };
        assert!(uncalibrated(Some(&failed)));
        let solved = ArrayStatus {
            cal: CalPhase::Solved,
            ..ArrayStatus::default()
        };
        assert!(!uncalibrated(Some(&solved)));
    }
}
