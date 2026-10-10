use sdrmm_wire::{
    array::{ArrayGain, ArrayStatus, ArrayTuneRequest},
    limits::ARRAY_LIMITS,
};
use zgui::{prelude::*, vocab::SharedString};

use super::{
    Array,
    labels::lane_quality_percent,
    rows::{LaneRow, lane_title},
};
use crate::{
    store::Store,
    ui::{
        faces::processor::hint,
        kit_sources::debounced,
        widgets::{check, row_field, slide},
    },
};

pub const NO_LANES_HINT: &str = "Wire radio lanes in";

#[must_use]
pub fn gain_bounds(status: Option<&ArrayStatus>) -> (f64, f64) {
    let limits = ARRAY_LIMITS.gain_db;
    let range = status.and_then(|status| status.gain_range_db.as_ref());
    (
        range.map_or(limits.min, |range| range.min.max(limits.min)),
        range.map_or(limits.max, |range| range.max.min(limits.max)),
    )
}

#[must_use]
pub fn held_gain(status: Option<&ArrayStatus>) -> f64 {
    match status.map(|status| (status.gain, status.gain_db)) {
        Some((ArrayGain::Manual { db }, _)) => db,
        Some((ArrayGain::Auto, Some(db))) => db,
        _ => ARRAY_LIMITS.gain_db.min,
    }
}

#[must_use]
pub fn lane_cells(row: &LaneRow) -> (u8, String, String, bool) {
    row.status.as_ref().map_or_else(
        || (0, "-".to_owned(), "-".to_owned(), false),
        |lane| {
            (
                lane_quality_percent(lane.coherence),
                format!("{:.1} dB", lane.gain_db),
                format!("{:.0}°", lane.phase_deg),
                lane.clipping,
            )
        },
    )
}

fn set_gain(store: Store, node: String, busy: RwSignal<bool>, gain: ArrayGain) {
    busy.set(true);
    zgui::task::spawn_local(async move {
        let body = ArrayTuneRequest {
            center_hz: None,
            gain: Some(gain),
        };
        let sent = store
            .api()
            .patch::<_, serde::de::IgnoredAny>(&format!("/api/arrays/{node}/tune"), &body)
            .await;
        if let Err(error) = sent {
            store.say(format!("Cannot set the array gain: {error:#}"));
        }
        busy.try_set(false);
    });
}

fn gain_row(cx: &Array) -> impl IntoView {
    let (store, status) = (cx.store, cx.status);
    let busy = RwSignal::new(false);
    let auto = Memo::new(move |_| {
        status.with(|status| {
            status
                .as_ref()
                .is_some_and(|found| found.gain == ArrayGain::Auto)
        })
    });
    let held = Memo::new(move |_| status.with(|status| held_gain(status.as_ref())));
    let bounds = Memo::new(move |_| status.with(|status| gain_bounds(status.as_ref())));
    let off = Signal::derive(move || busy.get() || status.with(Option::is_none));
    let frozen = Signal::derive(move || off.get() || auto.get());
    let commit = {
        let node = cx.node.clone();
        move |db: f64| set_gain(store, node.clone(), busy, ArrayGain::Manual { db })
    };
    let (pending, change) = debounced(commit);
    let shown = Signal::derive(move || pending.get().unwrap_or_else(|| held.get()));
    let slider = move || {
        let (min, max) = bounds.get();
        let change = change.clone();
        let read = |db: f64| format!("{db:.1} dB");
        AnyView::new(slide(shown, min, max, read, move |db| {
            if !frozen.get_untracked() {
                change(db);
            }
        }))
    };
    let toggle = {
        let node = cx.node.clone();
        move |on: bool| {
            if off.get_untracked() {
                return;
            }
            let gain = if on {
                ArrayGain::Auto
            } else {
                ArrayGain::Manual {
                    db: held.get_untracked(),
                }
            };
            set_gain(store, node.clone(), busy, gain);
        }
    };
    let body = view! {
        row(class = "field__body") {
            row(class = "kit-auto", a11y:description = SharedString::from("Array picks one gain for every lane. Needs a cal source.")) {
                {check(auto.into(), toggle)}
                text(class = "kit-legend") {"Auto"}
            }
            box(class = "kit-slot", class:kit-muted = frozen) {{slider}}
        }
    };
    view! {
        box(a11y:description = SharedString::from("One gain for every lane")) {
            {row_field("Gain", body)}
        }
    }
}

fn lane_row(row: LaneRow) -> AnyView {
    let (quality, gain, phase, clipping) = lane_cells(&row);
    let title = SharedString::from(lane_title(&row));
    let name = format!("L{}", row.lane);
    let meter = format!("Lane {} quality", row.lane);
    AnyView::new(view! {
        row(class = "af__lane", a11y:description = title) {
            text(class = "af__lane_name") {{name}}
            text(class = "af__lane_source") {{row.source_label}}
            box(class = "af__bar", a11y:label = meter) {
                box(class = "af__bar_fill", style:width = Some(format!("{quality}%")))
            }
            text(class = "af__lane_gain", class:danger = clipping) {{gain}}
            text(class = "af__lane_phase") {{phase}}
        }
    })
}

pub fn lanes(cx: &Array) -> impl IntoView {
    let rows = cx.rows;
    let empty = Memo::new(move |_| rows.with(Vec::is_empty));
    let cx = cx.clone();
    move || {
        if empty.get() {
            return AnyView::new(hint(NO_LANES_HINT));
        }
        let list = move || rows.get().into_iter().map(lane_row).collect::<Vec<_>>();
        AnyView::new(view! {
            column(class = "af__lanes") {
                {gain_row(&cx)}
                column(class = "af__lane_list", a11y:label = "Lanes") {{list}}
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{array::ArrayLaneStatus, device::Range};

    use super::*;

    fn row(status: Option<ArrayLaneStatus>) -> LaneRow {
        LaneRow {
            lane: 1,
            source_label: "KrakenSDR iq1".to_owned(),
            status,
        }
    }

    #[test]
    fn the_gain_slider_stays_inside_both_the_radio_and_the_array_limits() {
        assert_eq!(gain_bounds(None), (-20.0, 80.0));
        let narrow = ArrayStatus {
            gain_range_db: Some(Range {
                min: 0.0,
                max: 49.6,
                step: None,
            }),
            ..ArrayStatus::default()
        };
        assert_eq!(gain_bounds(Some(&narrow)), (0.0, 49.6));
        let wide = ArrayStatus {
            gain_range_db: Some(Range {
                min: -40.0,
                max: 120.0,
                step: None,
            }),
            ..ArrayStatus::default()
        };
        assert_eq!(gain_bounds(Some(&wide)), (-20.0, 80.0));
    }

    #[test]
    fn the_held_gain_is_the_manual_value_or_what_auto_chose() {
        assert_eq!(held_gain(None), -20.0);
        let manual = ArrayStatus {
            gain: ArrayGain::Manual { db: 12.5 },
            gain_db: Some(30.0),
            ..ArrayStatus::default()
        };
        assert_eq!(held_gain(Some(&manual)), 12.5);
        let auto = ArrayStatus {
            gain: ArrayGain::Auto,
            gain_db: Some(30.0),
            ..ArrayStatus::default()
        };
        assert_eq!(held_gain(Some(&auto)), 30.0);
    }

    #[test]
    fn a_lane_reads_quality_gain_and_phase_or_dashes() {
        assert_eq!(
            lane_cells(&row(None)),
            (0, "-".to_owned(), "-".to_owned(), false)
        );
        let measured = ArrayLaneStatus {
            coherence: 0.826,
            gain_db: -1.24,
            phase_deg: 42.4,
            clipping: true,
            ..ArrayLaneStatus::default()
        };
        assert_eq!(
            lane_cells(&row(Some(measured))),
            (83, "-1.2 dB".to_owned(), "42°".to_owned(), true)
        );
    }
}
