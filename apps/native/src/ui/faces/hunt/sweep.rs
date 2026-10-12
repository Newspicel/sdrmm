use sdrmm_wire::{
    HuntAction,
    hunt::{HuntSweep, HuntSweepParams},
    limits::HUNT_LIMITS,
};
use zgui::prelude::*;

use super::{
    model::{covered_label, degrees_label, sweep_on, sweep_text},
    rose::rose,
};
use crate::ui::{
    faces::channel::settings::NumberLimit,
    kit_audio::{Line, lines, titled, titled_block},
    kit_channel::{NumberSpec, button, number_field, setting_row},
};

pub fn sweep_panel(
    sweep: Memo<Option<HuntSweep>>,
    busy: Signal<bool>,
    act: impl Fn(HuntAction) + Clone + 'static,
) -> impl IntoView {
    let on = Memo::new(move |_| sweep.with(|sweep| sweep_on(sweep.as_ref())));
    let dim = Signal::derive(move || !on.get());
    let heading = move || {
        let held = sweep.get().and_then(|sweep| sweep.heading_deg);
        match held {
            Some(deg) => AnyView::new(degrees_label(Some(deg))),
            None => titled(
                "Wire a phone GPS",
                AnyView::new(view! { text(class = "kc-danger") {"None"} }),
            ),
        }
    };
    let rows = move || {
        let held = sweep.get();
        let state = sweep_text(held.as_ref().map(|sweep| sweep.state).unwrap_or_default());
        vec![
            Line {
                label: String::from("Peak"),
                value: degrees_label(held.as_ref().and_then(|sweep| sweep.peak_deg)),
                title: None,
            },
            Line {
                label: String::from("Covered"),
                value: covered_label(held.as_ref()),
                title: None,
            },
            Line {
                label: String::from("Sweep"),
                value: state.label.to_owned(),
                title: Some(state.title.to_owned()),
            },
        ]
    };
    let turn = {
        let act = act.clone();
        move || {
            let act = act.clone();
            if on.get() {
                titled(
                    "Back to warmer and colder",
                    AnyView::new(button("End sweep", "kc-btn", busy, move || {
                        act(HuntAction::Start)
                    })),
                )
            } else {
                titled(
                    "Turn slowly all the way round",
                    AnyView::new(button("Sweep", "kc-btn kc-btn--primary", busy, move || {
                        act(HuntAction::Sweep)
                    })),
                )
            }
        }
    };
    let mark = titled(
        "Send your heading as a bearing",
        AnyView::new(button("Mark", "kc-btn", busy, move || {
            act(HuntAction::Mark)
        })),
    );
    view! {
        column(class = "hunt-sweep") {
            {rose(sweep, dim)}
            row(class = "readout__row") {
                text(class = "readout__label") {"Heading"}
                box(class = "readout__value") {{heading}}
            }
            {lines(rows)}
            row(class = "hunt-sweep__actions") {
                {turn}
                {mark}
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Param {
    label: &'static str,
    title: &'static str,
    unit: &'static str,
    limit: NumberLimit,
    read: fn(&HuntSweepParams) -> f64,
    write: fn(&mut HuntSweepParams, f64),
}

fn limit(min: f64, max: f64, step: f64) -> NumberLimit {
    NumberLimit {
        min: Some(min),
        max: Some(max),
        step: Some(step),
    }
}

fn params() -> [Param; 5] {
    let limits = HUNT_LIMITS;
    [
        Param {
            label: "Beam",
            title: "Main lobe of the handheld antenna",
            unit: "\u{b0}",
            limit: limit(limits.beamwidth_deg.min, limits.beamwidth_deg.max, 1.0),
            read: |held| held.beamwidth_deg,
            write: |held, value| held.beamwidth_deg = value,
        },
        Param {
            label: "F/B",
            title: "Front/back ratio of the antenna",
            unit: "dB",
            limit: limit(limits.front_back_db.min, limits.front_back_db.max, 1.0),
            read: |held| held.front_back_db,
            write: |held, value| held.front_back_db = value,
        },
        Param {
            label: "Span",
            title: "How far to turn before a bearing",
            unit: "\u{b0}",
            limit: limit(limits.min_span_deg.min, limits.min_span_deg.max, 10.0),
            read: |held| held.min_span_deg,
            write: |held, value| held.min_span_deg = value,
        },
        Param {
            label: "Contrast",
            title: "Peak over the weakest direction",
            unit: "dB",
            limit: limit(
                f64::from(limits.min_contrast_db.min),
                f64::from(limits.min_contrast_db.max),
                0.5,
            ),
            read: |held| f64::from(held.min_contrast_db),
            write: |held, value| held.min_contrast_db = value as f32,
        },
        Param {
            label: "Mount",
            title: "Antenna forward relative to the phone",
            unit: "\u{b0}",
            limit: limit(
                limits.mount_offset_deg.min,
                limits.mount_offset_deg.max,
                1.0,
            ),
            read: |held| held.mount_offset_deg,
            write: |held, value| held.mount_offset_deg = value,
        },
    ]
}

pub fn sweep_params(
    sweep: Memo<HuntSweepParams>,
    edit: impl Fn(Box<dyn FnOnce(&mut HuntSweepParams)>) + Clone + 'static,
) -> impl IntoView {
    let rows: Vec<AnyView> = params()
        .into_iter()
        .map(|param| {
            let edit = edit.clone();
            let value = Signal::derive(move || Some((param.read)(&sweep.get())));
            let spec = NumberSpec::new(param.label)
                .limit(param.limit)
                .unit(param.unit);
            let field = number_field(value, spec, move |next| {
                if let Some(next) = next {
                    edit(Box::new(move |held| (param.write)(held, next)));
                }
            });
            titled_block(param.title, setting_row(param.label, None, field))
        })
        .collect();
    view! { column(class = "kc-grid") {{rows}} }
}

pub const SHEET: &str = css!(
    r#"
.hunt-sweep { flex-direction: column; gap: 6px; padding-top: 8px; border-top: 1px solid var(--line); }
.hunt-sweep__actions { justify-content: center; gap: 8px; }
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sweep_setting_writes_back_what_it_reads() {
        for param in params() {
            let mut held = HuntSweepParams::default();
            let target = param.limit.min.unwrap_or_default();
            (param.write)(&mut held, target);
            assert_eq!((param.read)(&held), target, "{}", param.label);
        }
        assert!(HuntSweepParams::default().valid());
    }
}
