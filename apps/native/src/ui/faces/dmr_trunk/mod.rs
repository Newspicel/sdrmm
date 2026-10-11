pub mod trunk;

use std::collections::HashSet;

use sdrmm_wire::{
    patch::{DmrTrunkNode, MAX_DMR_LOGICAL_CHANNEL, NodeBody},
    state::{TrunkChannel, TrunkSystemStatus},
};
use zgui::prelude::*;

use self::trunk::{
    DMR_TRUNK_PROTOCOLS, activity_rows, adoptable, awaiting_control_channel, channel_entry,
    channel_plan_rows, control_channel_stalled, control_hint, format_search_ranges,
    parse_control_hz, parse_search_ranges, plan_label, plan_summary, search_summary, source_hint,
    source_label, trunk_head, usable, with_channel, without_channel,
};
use crate::{
    head,
    store::Store,
    ui::{
        faces::channel::settings::NumberLimit,
        kit_audio::{self, Line, fault, lines, titled},
        kit_channel::{
            self, NumberSpec, button, format_hz, number_field, setting_row, text_field, tip,
            toggle_row,
        },
        widgets::pick,
    },
};

const SHEET: &str = css!(
    r#"
.dt-face { flex-direction: column; }
.dt-section { flex-direction: column; gap: 8px; padding: 8px; border-bottom: 1px solid var(--line); }
.dt-faults { flex-direction: column; gap: 4px; padding: 8px; border-bottom: 1px solid var(--line); }
.dt-plan { flex-direction: column; border-bottom: 1px solid var(--line); }
.dt-plan__head { align-items: center; justify-content: space-between; gap: 8px; padding: 8px 8px 0 8px; }
.dt-table { flex-direction: column; max-height: 192px; overflow: auto; margin-top: 4px; }
.dt-tr { align-items: center; font-family: var(--mono); font-size: 11px; }
.dt-tr.following { background-color: color-mix(in oklab, var(--accent) 10%, transparent); }
.dt-th { font-size: 10px; color: var(--ink-faint); }
.dt-lcn { width: 48px; padding: 3px 8px; text-align: right; color: var(--ink); }
.dt-mhz { width: 84px; padding: 3px 8px; text-align: right; color: var(--ink); }
.dt-mhz.guess { color: var(--ink-faint); }
.dt-by { flex: 1 1 auto; padding: 3px 8px; color: var(--ink-dim); }
.dt-by.guess { color: var(--ink-faint); }
.dt-act { width: 64px; padding: 3px 8px; justify-content: flex-end; display: flex; }
.dt-add { align-items: center; gap: 8px; padding: 8px 8px 0 8px; }
.dt-summary { padding: 8px; font-size: 11px; color: var(--ink-dim); }
"#
);

fn settings_of(store: Store, node: &str) -> Option<DmrTrunkNode> {
    match &store.graph.get().node(node)?.body {
        NodeBody::DmrTrunk(settings) => Some(settings.clone()),
        _ => None,
    }
}

fn edit(store: Store, node: &str, change: impl FnOnce(&mut DmrTrunkNode) + 'static) {
    store.edit_node(node, move |body| {
        if let NodeBody::DmrTrunk(settings) = body {
            change(settings);
        }
    });
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("dmr-trunk", SHEET);
    kit_channel::install();
    kit_audio::install();
    let settings = {
        let node = node.clone();
        Memo::new(move |_| settings_of(store, &node).unwrap_or_default())
    };
    let status = {
        let node = node.clone();
        Memo::new(move |_| {
            store
                .state
                .get()
                .trunk_systems
                .iter()
                .find(|system| system.node == node)
                .cloned()
        })
    };
    let on_iq = {
        let node = node.clone();
        Memo::new(move |_| {
            store
                .graph
                .get()
                .edges
                .iter()
                .any(|edge| edge.to.node == node && edge.to.port == "iq")
        })
    };
    head::show(store, node.clone(), move || {
        let held = settings.get();
        let status = status.get();
        trunk_head(
            on_iq.get(),
            awaiting_control_channel(on_iq.get(), held.control_hz),
            held.protocol,
            status.as_ref().and_then(|status| status.detected),
            status.as_ref().map_or(0, |status| status.followers.len()),
        )
    });
    view! {
        column(class = "dt-face") {
            {setup(store, node.clone(), settings, status, on_iq)}
            {activity(status)}
            {faults(settings, status, on_iq)}
            {plan(store, node, settings, status)}
        }
    }
}

fn setup(
    store: Store,
    node: String,
    settings: Memo<DmrTrunkNode>,
    status: Memo<Option<TrunkSystemStatus>>,
    on_iq: Memo<bool>,
) -> impl IntoView {
    let protocol = Signal::derive(move || Some(settings.get().protocol));
    let options = DMR_TRUNK_PROTOCOLS
        .iter()
        .map(|(value, label)| (*value, (*label).to_owned()))
        .collect::<Vec<_>>();
    let pick_protocol = {
        let node = node.clone();
        move |next| edit(store, &node, move |held| held.protocol = next)
    };
    let control = Signal::derive(move || {
        settings
            .get()
            .control_hz
            .map(|hz| crate::ui::kit_channel::format_number(hz as f64 / 1e6, None))
            .unwrap_or_default()
    });
    let set_control = {
        let node = node.clone();
        move |text: String| {
            let hz = parse_control_hz(&text);
            edit(store, &node, move |held| held.control_hz = hz);
            true
        }
    };
    let awaiting =
        Memo::new(move |_| awaiting_control_channel(on_iq.get(), settings.get().control_hz));
    let control_row = move || {
        let set_control = set_control.clone();
        titled(
            control_hint(awaiting.get()),
            setting_row(
                "Control",
                None,
                view! {
                    box(class = "kc-num") {
                        text(class = "kc-num__unit") {"MHz"}
                        {text_field(control, "Control channel", "451.0125", "kc-fill", set_control)}
                    }
                },
            ),
        )
    };
    view! {
        column(class = "dt-section kc-grid") {
            {titled("Trunking protocol", setting_row("Protocol", None, pick(options, protocol, pick_protocol)))}
            {control_row}
            {search(store, node, settings, status)}
        }
    }
}

fn faults(
    settings: Memo<DmrTrunkNode>,
    status: Memo<Option<TrunkSystemStatus>>,
    on_iq: Memo<bool>,
) -> impl IntoView {
    let shown = move || {
        let mut shown = Vec::new();
        let status = status.get();
        if control_channel_stalled(
            on_iq.get(),
            settings.get().control_hz,
            status.as_ref().map(|status| status.carriers),
        ) {
            shown.push(fault(
                String::from("Control channel not running"),
                Some(String::from("Check it sits inside the radio's passband.")),
            ));
        }
        for problem in status.map(|status| status.problems).unwrap_or_default() {
            shown.push(fault(
                format!(
                    "Cannot follow {} TS {}",
                    format_hz(problem.freq_hz as f64),
                    problem.slot
                ),
                Some(problem.reason),
            ));
        }
        (!shown.is_empty()).then(|| view! { column(class = "dt-faults") {{shown}} })
    };
    shown
}

fn activity(status: Memo<Option<TrunkSystemStatus>>) -> impl IntoView {
    let rows = Memo::new(move |_| status.with(|status| activity_rows(status.as_ref())));
    move || {
        (!rows.with(Vec::is_empty)).then(|| {
            let shown = move || {
                rows.get()
                    .into_iter()
                    .map(|row| Line {
                        label: row.label,
                        value: row.value,
                        title: row.title.map(str::to_owned),
                    })
                    .collect()
            };
            view! { column(class = "dt-section") {{lines(shown)}} }
        })
    }
}

fn plan(
    store: Store,
    node: String,
    settings: Memo<DmrTrunkNode>,
    status: Memo<Option<TrunkSystemStatus>>,
) -> impl IntoView {
    let learned = Memo::new(move |_| {
        status
            .get()
            .map(|status| status.channel_map)
            .unwrap_or_default()
    });
    let rows = Memo::new(move |_| channel_plan_rows(&learned.get(), &settings.get().channel_map));
    let found = Memo::new(move |_| adoptable(&learned.get(), &settings.get().channel_map));
    let following = Memo::new(move |_| {
        status
            .get()
            .map(|status| {
                status
                    .followers
                    .iter()
                    .filter_map(|follower| follower.logical_channel)
                    .collect::<HashSet<u16>>()
            })
            .unwrap_or_default()
    });
    let label = move || {
        let held = settings.get();
        plan_label(
            held.protocol,
            status.get().and_then(|status| status.detected),
        )
    };
    let keep = {
        let node = node.clone();
        move || {
            let found = found.get();
            let node = node.clone();
            (!found.is_empty()).then(|| {
                let count = found.len();
                AnyView::new(button(
                    format!("Keep {count} found"),
                    "kc-btn kc-btn--sm",
                    false,
                    move || {
                        let found = found.clone();
                        edit(store, &node, move |held| held.channel_map.extend(found));
                    },
                ))
            })
        }
    };
    let table = {
        let node = node.clone();
        move || {
            let following = following.get();
            rows.get()
                .into_iter()
                .map(|row| {
                    plan_row(
                        store,
                        node.clone(),
                        row,
                        following.contains(&row.logical_channel),
                    )
                })
                .collect::<Vec<_>>()
        }
    };
    view! {
        column(class = "dt-plan") {
            row(class = "dt-plan__head") {
                text(class = "kc-legend") {{label}}
                {keep}
            }
            column(class = "dt-table", on:wheel = |ev: &mut EventCx<'_, events::Wheel>| ev.stop_propagation()) {
                row(class = "dt-tr dt-th") {
                    text(class = "dt-lcn") {"LCN"}
                    text(class = "dt-mhz") {"MHz"}
                    text(class = "dt-by") {"Known by"}
                    box(class = "dt-act") {}
                }
                {table}
            }
            {add_row(store, node)}
            text(class = "dt-summary") {{move || plan_summary(&rows.get())}}
        }
    }
}

fn plan_row(store: Store, node: String, row: TrunkChannel, following: bool) -> AnyView {
    let guess = !usable(row.source);
    let known = match row.source {
        sdrmm_wire::state::TrunkChannelSource::Learned => {
            format!("{} {}%", source_label(row.source), row.confidence)
        }
        source => source_label(source).to_owned(),
    };
    let forget = (row.source == sdrmm_wire::state::TrunkChannelSource::Manual).then(|| {
        let lcn = row.logical_channel;
        AnyView::new(button("Forget", "kc-btn kc-btn--sm", false, move || {
            edit(store, &node, move |held| {
                held.channel_map = without_channel(&held.channel_map, lcn)
            });
        }))
    });
    AnyView::new(view! {
        row(class = "dt-tr", class:following = following) {
            text(class = "dt-lcn") {{row.logical_channel.to_string()}}
            text(class = "dt-mhz", class:guess = guess) {{format!("{:.4}", row.freq_hz as f64 / 1e6)}}
            {tip(source_hint(row.source), AnyView::new(view! { text(class = "dt-by", class:guess = guess) {{known}} }))}
            box(class = "dt-act") {{forget}}
        }
    })
}

fn add_row(store: Store, node: String) -> impl IntoView {
    let lcn = RwSignal::new(None::<f64>);
    let mhz = RwSignal::new(None::<f64>);
    let pending = Signal::derive(move || channel_entry(lcn.get(), mhz.get()));
    let add = move || {
        let Some(entry) = pending.get_untracked() else {
            return;
        };
        edit(store, &node, move |held| {
            held.channel_map = with_channel(&held.channel_map, entry)
        });
        lcn.set(None);
        mhz.set(None);
    };
    view! {
        row(class = "dt-add") {
            {number_field(
                lcn.into(),
                NumberSpec::new("Logical channel to add")
                    .limit(NumberLimit::new(0.0, f64::from(MAX_DMR_LOGICAL_CHANNEL), 1.0))
                    .optional("LCN")
                    .size("kc-num kc-num--narrow"),
                move |next| lcn.set(next),
            )}
            {number_field(
                mhz.into(),
                NumberSpec::new("Frequency to add")
                    .limit(NumberLimit { min: Some(0.0), max: None, step: Some(0.0125) })
                    .unit("MHz")
                    .optional("451.0125"),
                move |next| mhz.set(next),
            )}
            {button("Add", "kc-btn kc-btn--sm", Signal::derive(move || pending.get().is_none()), add)}
        }
    }
}

fn search(
    store: Store,
    node: String,
    settings: Memo<DmrTrunkNode>,
    status: Memo<Option<TrunkSystemStatus>>,
) -> impl IntoView {
    let enabled = Signal::derive(move || settings.get().discovery.enabled);
    let toggle = {
        let node = node.clone();
        move |on: bool| edit(store, &node, move |held| held.discovery.enabled = on)
    };
    let ranges = Signal::derive(move || format_search_ranges(&settings.get().discovery.ranges));
    let summary = move || {
        let held = settings.get();
        let status = status.get();
        search_summary(
            &held.discovery.ranges,
            status.as_ref().map_or(0, |status| status.candidates),
            status.as_ref().map_or(0, |status| status.searching),
            status.as_ref().map_or(0, |status| status.probes.len()),
        )
    };
    let range = move || {
        let node = node.clone();
        enabled.get().then(|| {
            titled(
                format!(
                    "Narrow the search to start-end in MHz / step in kHz. {}",
                    summary()
                ),
                setting_row(
                    "Range",
                    None,
                    text_field(
                        ranges,
                        "Search range",
                        "whole band",
                        "kc-num kc-num--wide",
                        move |text| {
                            let parsed = parse_search_ranges(&text);
                            edit(store, &node, move |held| held.discovery.ranges = parsed);
                            true
                        },
                    ),
                ),
            )
        })
    };
    view! {
        {titled("Find the rest of the site's channels", toggle_row("Search", None, enabled, toggle))}
        {range}
    }
}
