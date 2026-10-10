use sdrmm_wire::{
    array::{ArrayCalSource, ArrayOrientation, ArrayTuningMode},
    device::Coherence,
    limits::ARRAY_LIMITS,
};
use zgui::{prelude::*, vocab::SharedString};

use super::{
    Array, geometry_view,
    labels::{
        cal_source_of, cal_sources, check_options, heading_label, switched_orientation,
        tier_options, tier_text,
    },
    rows::span_hz,
};
use crate::ui::{
    kit_raster::{Bounds, number},
    kit_sources::{collapsible, units},
    widgets::{check, pick, row_field, segments},
};

pub const NO_HEADING: &str = "No heading";
pub const NO_HEADING_TITLE: &str = "Wire a GPS with heading, like a phone";
pub const TIER_TITLE: &str = "What the radios share";

fn titled(title: &'static str, body: impl IntoView + 'static) -> AnyView {
    AnyView::new(view! { box(a11y:description = SharedString::from(title)) {{body}} })
}

fn tuning(cx: &Array) -> impl IntoView {
    let chosen = cx.read(|settings| settings.tuning);
    let edit = cx.clone();
    let options = vec![
        (ArrayTuningMode::Together, "Together"),
        (ArrayTuningMode::Spread, "Spread"),
    ];
    titled(
        "Together: every lane on one frequency. Spread: lanes side by side.",
        row_field(
            "Tuning",
            segments(options, chosen, move |mode| {
                edit.edit(|settings| settings.tuning = mode)
            }),
        ),
    )
}

fn span(cx: &Array) -> impl IntoView {
    let (store, node, sets, status) = (cx.store, cx.node.clone(), cx.sets, cx.status);
    let spread = cx.read(|settings| settings.tuning == ArrayTuningMode::Spread);
    let value = Signal::derive(move || {
        let hz = store.graph.with(|graph| {
            sets.with(|sets| status.with(|status| span_hz(graph, sets, &node, status.as_ref())))
        });
        hz.map_or_else(|| "-".to_owned(), units::mhz)
    });
    move || {
        spread.get().then(|| {
            titled(
                "Band the lanes cover together",
                row_field(
                    "Span",
                    view! { text(class = "kit-mono") {{move || value.get()}} },
                ),
            )
        })
    }
}

fn heading_rows(cx: &Array) -> AnyView {
    let mount = cx.read(|settings| match settings.orientation {
        ArrayOrientation::Heading { mount_offset_deg } => mount_offset_deg,
        ArrayOrientation::Fixed { .. } => 0.0,
    });
    let (store, node, status, settings) = (cx.store, cx.node.clone(), cx.status, cx.settings);
    let wired = Signal::derive(move || {
        store
            .graph
            .with(|graph| graph.position_source(&node).is_some())
    });
    let label = Signal::derive(move || {
        if !wired.get() {
            return NO_HEADING.to_owned();
        }
        let orientation = settings.with(|settings| settings.orientation);
        status.with(|status| heading_label(orientation, status.as_ref()))
    });
    let title =
        Signal::derive(move || SharedString::from(if wired.get() { "" } else { NO_HEADING_TITLE }));
    let edit = cx.clone();
    AnyView::new(view! {
        column {
            {titled("Array forward relative to the heading source", row_field("Mount", number("Mount", mount, Bounds::new(-180.0, 180.0), move |mount_offset_deg| {
                edit.edit(|settings| settings.orientation = ArrayOrientation::Heading { mount_offset_deg });
            })))}
            box(a11y:description = title) {
                {row_field("Heading", view! { text(class = "kit-mono", class:danger = move || !wired.get()) {{move || label.get()}} })}
            }
        }
    })
}

fn fixed_rows(cx: &Array) -> AnyView {
    let azimuth = cx.read(|settings| match settings.orientation {
        ArrayOrientation::Fixed { azimuth_deg } => azimuth_deg,
        ArrayOrientation::Heading { .. } => 0.0,
    });
    let edit = cx.clone();
    titled(
        "Array forward, from true north",
        row_field(
            "Azimuth",
            number(
                "Azimuth",
                azimuth,
                Bounds::new(0.0, 359.9),
                move |azimuth_deg| {
                    edit.edit(|settings| {
                        settings.orientation = ArrayOrientation::Fixed { azimuth_deg }
                    });
                },
            ),
        ),
    )
}

fn orientation(cx: &Array) -> impl IntoView {
    let heading = Memo::new({
        let settings = cx.settings;
        move |_| {
            settings
                .with(|settings| matches!(settings.orientation, ArrayOrientation::Heading { .. }))
        }
    });
    let edit = cx.clone();
    let pick_mode = move |to_heading: bool| {
        let azimuth = edit
            .status
            .with_untracked(|status| status.as_ref().and_then(|found| found.azimuth_deg));
        let current = edit
            .settings
            .with_untracked(|settings| settings.orientation);
        let next = switched_orientation(to_heading, current, azimuth);
        if next != current {
            edit.edit(|settings| settings.orientation = next);
        }
    };
    let options = vec![(false, "Fixed"), (true, "Heading")];
    let rows = {
        let cx = cx.clone();
        move || {
            if heading.get() {
                heading_rows(&cx)
            } else {
                fixed_rows(&cx)
            }
        }
    };
    view! {
        column {
            {titled("Where the array points", row_field("Forward", segments(options, heading.into(), pick_mode)))}
            {rows}
        }
    }
}

fn cal_source(cx: &Array) -> impl IntoView {
    let chosen = cx.read(|settings| Some(settings.cal.source.kind()));
    let edit = cx.clone();
    titled(
        "Calibration source",
        row_field(
            "Cal source",
            pick(cal_sources(), chosen, move |kind| {
                edit.edit(|settings| {
                    settings.cal.source = cal_source_of(kind, &settings.cal.source)
                });
            }),
        ),
    )
}

fn tier(cx: &Array) -> impl IntoView {
    let (members, status, sets) = (cx.members, cx.status, cx.sets);
    let shared = Memo::new(move |_| members.with(Vec::len) >= 2);
    let unset = Memo::new({
        let settings = cx.settings;
        move |_| settings.with(|settings| settings.declared == Coherence::None)
    });
    let declared = cx.read(|settings| Some(settings.declared));
    let lead = Signal::derive(move || {
        let lead = members.with(|members| members.first().cloned());
        let coherence = lead.and_then(|lead| {
            sets.with(|sets| sets.get(&lead).map(|set| set.capabilities.coherence))
        });
        tier_text(coherence.unwrap_or_default())
    });
    let cx = cx.clone();
    move || {
        if !shared.get() {
            return status.with(Option::is_none).then(|| {
                titled(
                    "One radio sets the tier",
                    row_field(
                        "Tier",
                        view! { text(class = "kit-mono") {{move || lead.get()}} },
                    ),
                )
            });
        }
        let edit = cx.clone();
        let options = tier_options(if unset.get() {
            Coherence::None
        } else {
            Coherence::TimeSync
        });
        Some(AnyView::new(view! {
            box(class:af__danger = unset, a11y:description = SharedString::from(TIER_TITLE)) {
                {row_field("Tier", pick(options, declared, move |tier: Coherence| {
                    if tier != Coherence::None {
                        edit.edit(|settings| settings.declared = tier);
                    }
                }))}
            }
        }))
    }
}

pub fn chips(cx: &Array) -> impl IntoView {
    view! {
        column(class = "params") {
            {tuning(cx)}
            {span(cx)}
            {orientation(cx)}
            {cal_source(cx)}
            {tier(cx)}
        }
    }
}

fn set_source(cx: &Array, change: impl FnOnce(&mut ArrayCalSource)) {
    cx.edit(|settings| change(&mut settings.cal.source));
}

fn emitter_rows(cx: &Array) -> AnyView {
    let bearing = cx.read(|settings| match settings.cal.source {
        ArrayCalSource::Emitter { bearing_deg, .. } => bearing_deg,
        _ => 0.0,
    });
    let edit = cx.clone();
    titled(
        "True bearing to the transmitter",
        row_field(
            "Bearing",
            number("Bearing", bearing, Bounds::new(0.0, 359.9), move |value| {
                set_source(&edit, |source| {
                    if let ArrayCalSource::Emitter { bearing_deg, .. } = source {
                        *bearing_deg = value;
                    }
                });
            }),
        ),
    )
}

fn carrier_rows(cx: &Array) -> AnyView {
    let offset = cx.read(|settings| match settings.cal.source {
        ArrayCalSource::Pilot { offset_hz, .. } | ArrayCalSource::Emitter { offset_hz, .. } => {
            offset_hz
        }
        _ => 0.0,
    });
    let width = cx.read(|settings| match settings.cal.source {
        ArrayCalSource::Pilot { bandwidth_hz, .. }
        | ArrayCalSource::Emitter { bandwidth_hz, .. } => bandwidth_hz,
        _ => ARRAY_LIMITS.cal_bandwidth_seed_hz,
    });
    let (offsets, widths) = (ARRAY_LIMITS.cal_offset_hz, ARRAY_LIMITS.cal_bandwidth_hz);
    let (on_offset, on_width) = (cx.clone(), cx.clone());
    AnyView::new(view! {
        column {
            {titled("From the array centre, Hz", row_field("Offset", number("Offset", offset, Bounds::whole(offsets.min, offsets.max), move |value| {
                set_source(&on_offset, |source| {
                    if let ArrayCalSource::Pilot { offset_hz, .. } | ArrayCalSource::Emitter { offset_hz, .. } = source {
                        *offset_hz = value;
                    }
                });
            })))}
            {titled("Cal signal width, Hz", row_field("Width", number("Width", width, Bounds::whole(widths.min, widths.max), move |value| {
                set_source(&on_width, |source| {
                    if let ArrayCalSource::Pilot { bandwidth_hz, .. } | ArrayCalSource::Emitter { bandwidth_hz, .. } = source {
                        *bandwidth_hz = value;
                    }
                });
            })))}
        }
    })
}

fn source_rows(cx: &Array) -> impl IntoView {
    let kind = Memo::new({
        let settings = cx.settings;
        move |_| settings.with(|settings| settings.cal.source.kind())
    });
    let cx = cx.clone();
    move || {
        let emitter = matches!(kind.get(), Some(sdrmm_wire::array::CalSourceKind::Emitter));
        let carrier = matches!(
            kind.get(),
            Some(
                sdrmm_wire::array::CalSourceKind::Pilot | sdrmm_wire::array::CalSourceKind::Emitter
            )
        );
        view! {
            column {
                {emitter.then(|| emitter_rows(&cx))}
                {carrier.then(|| carrier_rows(&cx))}
            }
        }
    }
}

fn check_row(cx: &Array) -> impl IntoView {
    let current = Memo::new({
        let settings = cx.settings;
        move |_| settings.with(|settings| settings.cal.check_s)
    });
    let cx = cx.clone();
    move || {
        let edit = cx.clone();
        let chosen = Signal::derive(move || Some(current.get()));
        titled(
            "Recheck phase this often",
            row_field(
                "Check",
                pick(check_options(current.get()), chosen, move |check_s| {
                    edit.edit(|settings| settings.cal.check_s = check_s);
                }),
            ),
        )
    }
}

fn calibration(cx: &Array) -> AnyView {
    let equaliser = cx.read(|settings| settings.cal.equaliser);
    let warm = cx.read(|settings| settings.cal.warm_start);
    let (on_eq, on_warm) = (cx.clone(), cx.clone());
    AnyView::new(view! {
        column(class = "params") {
            {source_rows(cx)}
            {check_row(cx)}
            {titled("Flatten each lane across the band", row_field("EQ", check(equaliser, move |on| on_eq.edit(|settings| settings.cal.equaliser = on))))}
            {titled("Start from the last stored calibration", row_field("Warm start", check(warm, move |on| on_warm.edit(|settings| settings.cal.warm_start = on))))}
        }
    })
}

pub fn folds(cx: &Array) -> impl IntoView {
    let (geometry, cal) = (cx.clone(), cx.clone());
    view! {
        column {
            {collapsible(|| "Geometry".to_owned(), "kit-fold__link", move || geometry_view::editor(&geometry))}
            {collapsible(|| "Calibration".to_owned(), "kit-fold__link", move || calibration(&cal))}
        }
    }
}
