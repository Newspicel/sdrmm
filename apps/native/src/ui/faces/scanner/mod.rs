pub mod plan;
pub mod scan;

use std::{collections::HashMap, sync::Arc};

use sdrmm_wire::{
    bandplan::BandPlan,
    patch::NodeBody,
    scan::{ScanAction, ScanMode, ScanRange, ScanRequest, ScanSettings, ScanState, ScannerStatus},
    state::DeviceSet,
    ws::ServerEvent,
};
use zgui::prelude::*;

use self::{
    plan::{apply_preset, band_plan, channel_name, preset_in, scan_presets},
    scan::{
        MIN_STEP_KHZ, RangeValues, format_db, format_mhz, live_status, range_problem, range_title,
        range_value, scan_targets, sweep_kind, with_hz, without_hz,
    },
};
use crate::{
    store::Store,
    ui::{
        faces::channel::{binding::controlled_node_of, settings::NumberLimit},
        kit_audio::{self, Stat, titled},
        kit_channel::{
            self, CLOSE_ICON, NumberSpec, button, group, icon, number_field, readout_row,
            setting_row, toggle_row,
        },
        widgets::pick,
    },
};

type Plan = RwSignal<Option<Arc<BandPlan>>>;

const SHEET: &str = css!(
    r#"
.sc-face { flex-direction: column; }
.sc-body { flex-direction: column; gap: 8px; padding: 8px; }
.sc-readout { padding-bottom: 8px; border-bottom: 1px solid var(--line); }
.sc-foot { align-items: center; gap: 8px; padding: 6px 8px; border-top: 1px solid var(--line); }
.sc-list { flex-direction: column; max-height: 192px; overflow: auto; }
.sc-entry { align-items: center; gap: 8px; font-family: var(--mono); font-size: 11px; color: var(--ink); }
.sc-entry__name { flex: 1 1 auto; min-width: 0; color: var(--ink-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sc-count { font-family: var(--mono); font-size: 10px; color: var(--ink-faint); }
"#
);

#[derive(Clone, Debug, PartialEq)]
struct Decoder {
    set: DeviceSet,
    channel: u32,
    feeds: String,
}

fn decoder_of(store: Store, node: &str) -> Option<Decoder> {
    let driven = controlled_node_of(&store.graph.get(), node)?;
    let channel = store.channel_of(&driven)?;
    let set = store.set_of(store.device_set_of(&driven)?)?;
    Some(Decoder {
        feeds: format!(
            "{} at {}",
            channel.settings.params.type_id(),
            format_mhz(Some(channel.settings.frequency_hz))
        ),
        set,
        channel: channel.id,
    })
}

fn settings_of(store: Store, node: &str) -> ScanSettings {
    match store.graph.get().node(node).map(|found| &found.body) {
        Some(NodeBody::Scanner(scanner)) => scanner.settings.clone(),
        _ => ScanSettings::default(),
    }
}

fn edit_settings(store: Store, node: &str, edit: impl FnOnce(&mut ScanSettings) + 'static) {
    store.edit_node(node, move |body| {
        if let NodeBody::Scanner(scanner) = body {
            edit(&mut scanner.settings);
        }
    });
}

fn state_label(status: &ScannerStatus) -> &'static str {
    match (status.state, status.settings.mode) {
        (ScanState::Holding, ScanMode::All) => "visiting",
        (ScanState::Holding, _) => "holding",
        (ScanState::Done, _) => "done",
        (ScanState::Scanning, _) => "scanning",
    }
}

fn send(
    store: Store,
    decoder: (u32, u32),
    request: ScanRequest,
    then: impl FnOnce(ScannerStatus) + 'static,
) {
    zgui::task::spawn_local(async move {
        let path = format!(
            "/api/devicesets/{}/channels/{}/scanner",
            decoder.0, decoder.1
        );
        match store.api().post::<_, ScannerStatus>(&path, &request).await {
            Ok(finished) => then(finished),
            Err(error) => store.say(error.to_string()),
        }
        store.refresh_state();
    });
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("scanner-face", SHEET);
    kit_channel::install();
    kit_audio::install();
    let pushed = RwSignal::new(HashMap::<(u32, u32), ScannerStatus>::new());
    store.on_event(move |event| {
        if let ServerEvent::ScannerUpdate { device_set, status } = event {
            let key = (*device_set, status.settings.channel);
            let status = (**status).clone();
            pushed.update(|held| {
                held.insert(key, status);
            });
        }
    });
    let plan = band_plan(store);
    let decoder = {
        let node = node.clone();
        Memo::new(move |_| decoder_of(store, &node))
    };
    let settings = {
        let node = node.clone();
        Memo::new(move |_| settings_of(store, &node))
    };
    let status = Memo::new(move |_| {
        let decoder = decoder.get();
        let held = pushed.get();
        let pushed = decoder
            .as_ref()
            .and_then(|found| held.get(&(found.set.id, found.channel)));
        live_status(
            decoder.as_ref().map(|found| &found.set),
            decoder.as_ref().map(|found| found.channel),
            pushed,
        )
    });
    let running = Memo::new(move |_| status.get().is_some());
    let body = {
        let node = node.clone();
        move || {
            if running.get() {
                AnyView::new(running_view(status, decoder, plan))
            } else {
                AnyView::new(idle_view(store, node.clone(), settings, decoder, plan))
            }
        }
    };
    view! {
        column(class = "sc-face") {
            {body}
            {footer(store, node, settings, decoder, status, pushed)}
        }
    }
}

fn running_view(
    status: Memo<Option<ScannerStatus>>,
    decoder: Memo<Option<Decoder>>,
    plan: Plan,
) -> impl IntoView {
    let read = move |pick: fn(&ScannerStatus) -> String| {
        move || status.get().map(|status| pick(&status)).unwrap_or_default()
    };
    let resting = move || {
        status
            .get()
            .is_some_and(|status| status.state != ScanState::Scanning)
    };
    let frequency = move || {
        let Some(status) = status.get() else {
            return String::new();
        };
        let shown = format_mhz(Some(status.current_hz));
        match channel_name(plan.get().as_deref(), status.current_hz) {
            Some(name) => format!("{shown} {name}"),
            None => shown,
        }
    };
    let fault = move || status.get().and_then(|status| status.error);
    view! {
        column(class = "sc-body kc-readout") {
            {readout_row("State", view! {
                text(class:kc-accent = resting) {{move || status.get().map(|status| state_label(&status)).unwrap_or_default()}}
            })}
            {readout_row("Frequency", frequency)}
            {readout_row("Over noise", read(|status| format_db(status.current_snr_db)))}
            {readout_row("Find", read(|status| mode_label(status.settings.mode).to_owned()))}
            {readout_row("Sweep", move || {
                let status = status.get();
                sweep_kind(decoder.get().as_ref().map(|found| &found.set), status.as_ref()).to_owned()
            })}
            {readout_row("Span", read(|status| format!("{} to {}", format_mhz(Some(status.first_hz)), format_mhz(Some(status.last_hz)))))}
            {kit_audio::alert(fault)}
        }
    }
}

fn idle_view(
    store: Store,
    node: String,
    settings: Memo<ScanSettings>,
    decoder: Memo<Option<Decoder>>,
    plan: Plan,
) -> impl IntoView {
    let count = Memo::new(move |_| settings.get().ranges.len());
    let ranges = {
        let node = node.clone();
        move || {
            (0..count.get())
                .map(|index| range_group(store, node.clone(), settings, count, index))
                .collect::<Vec<_>>()
        }
    };
    let channels = {
        let node = node.clone();
        move || {
            let node = node.clone();
            (!settings.get().frequencies.is_empty()).then(|| {
                freq_list(
                    ("Channels", "Listed channels"),
                    Signal::derive(move || settings.get().frequencies),
                    plan,
                    false,
                    move |next| edit_settings(store, &node, move |held| held.frequencies = next),
                )
            })
        }
    };
    let problem = move || range_problem(&settings.get());
    view! {
        column(class = "sc-body") {
            column(class = "kc-readout sc-readout") {
                {move || decoder.get().map(|found| readout_row("Feeds", found.feeds))}
                {readout_row("Sweep", move || sweep_kind(decoder.get().as_ref().map(|found| &found.set), None).to_owned())}
            }
            column(class = "kc-grid") {
                {band_row(store, node.clone(), settings, plan)}
                {ranges}
                {channels}
                {sweep_group(store, node, settings, decoder, plan)}
            }
            {kit_audio::alert(problem)}
        }
    }
}

fn band_row(store: Store, node: String, settings: Memo<ScanSettings>, plan: Plan) -> AnyView {
    let presets = Memo::new(move |_| scan_presets(plan.get().as_deref()));
    let chosen = Signal::derive(move || {
        let presets = presets.get();
        Some(
            preset_in(&settings.get(), &presets)
                .map(|preset| preset.id.clone())
                .unwrap_or_default(),
        )
    });
    let picker = move || {
        let presets = presets.get();
        let mut options = vec![(String::new(), String::from("Custom"))];
        options.extend(
            presets
                .iter()
                .map(|preset| (preset.id.clone(), preset.name.clone())),
        );
        let node = node.clone();
        pick(options, chosen, move |id: String| {
            if let Some(preset) = presets.iter().find(|preset| preset.id == id).cloned() {
                edit_settings(store, &node, move |held| apply_preset(held, &preset));
            }
        })
    };
    titled(
        "Fill the scan from the band plan",
        setting_row("Band", None, picker),
    )
}

fn range_field(
    store: Store,
    node: String,
    settings: Memo<ScanSettings>,
    index: usize,
    spec: NumberSpec,
    read: fn(&RangeValues) -> f64,
    write: fn(&mut RangeValues, f64),
) -> impl IntoView {
    let value = Signal::derive(move || {
        settings
            .get()
            .ranges
            .get(index)
            .map(|range| read(&RangeValues::of(range)))
    });
    number_field(value, spec, move |next| {
        let Some(next) = next else { return };
        edit_settings(store, &node, move |held| {
            if let Some(range) = held.ranges.get_mut(index) {
                let mut values = RangeValues::of(range);
                write(&mut values, next);
                *range = values.wire();
            }
        });
    })
}

fn remove_button(label: String, press: impl Fn() + 'static) -> AnyView {
    AnyView::new(view! {
        control(
            class = "kc-icon",
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            a11y:label = label,
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
            on:click:stop = move |_| press()
        ) {
            {icon(CLOSE_ICON, "kc-glyph kc-glyph--sm")}
        }
    })
}

fn range_head(settings: Memo<ScanSettings>, count: Memo<usize>, index: usize) -> AnyView {
    let range = move || settings.get().ranges.get(index).copied();
    let reversed = move || range().is_some_and(|range| range.stop_hz < range.start_hz);
    let label = move || {
        let name = if count.get() > 1 {
            format!("Range {}", index + 1)
        } else {
            String::from("Range")
        };
        range().map_or(name.clone(), |range| {
            format!("{name} {}", range_value(&range))
        })
    };
    let title = move || range().map_or_else(String::new, |range| range_title(index, &range));
    AnyView::new(move || {
        titled(
            title(),
            AnyView::new(view! { text(class = "kc-name", class:kc-danger = reversed) {{label}} }),
        )
    })
}

fn range_group(
    store: Store,
    node: String,
    settings: Memo<ScanSettings>,
    count: Memo<usize>,
    index: usize,
) -> AnyView {
    let remove = {
        let node = node.clone();
        remove_button(format!("Remove range {}", index + 1), move || {
            edit_settings(store, &node, move |held| {
                if index < held.ranges.len() {
                    held.ranges.remove(index);
                }
            });
        })
    };
    let invalid = Signal::derive(move || {
        settings
            .get()
            .ranges
            .get(index)
            .is_some_and(|range| range.stop_hz < range.start_hz)
    });
    let mhz = |label| {
        NumberSpec::new(label)
            .limit(NumberLimit {
                min: Some(0.0),
                max: None,
                step: Some(0.1),
            })
            .unit("MHz")
    };
    AnyView::new(view! {
        column(class = "kc-group") {
            row(class = "kc-group__head") {
                {range_head(settings, count, index)}
                {remove}
            }
            {setting_row("From", None, range_field(store, node.clone(), settings, index, mhz("Range start"), |values| values.start_mhz, |values, next| values.start_mhz = next))}
            {setting_row("To", None, view! {
                box(class:kc-danger = invalid) {
                    {range_field(store, node.clone(), settings, index, mhz("Range stop"), |values| values.stop_mhz, |values, next| values.stop_mhz = next)}
                }
            })}
            {setting_row("Step", None, range_field(
                store, node.clone(), settings, index,
                NumberSpec::new("Range step").limit(NumberLimit { min: Some(MIN_STEP_KHZ), max: None, step: Some(MIN_STEP_KHZ) }).unit("kHz"),
                |values| values.step_khz,
                |values, next| values.step_khz = next,
            ))}
        }
    })
}

fn freq_list(
    (label, title): (&'static str, &'static str),
    values: Signal<Vec<f64>>,
    plan: Plan,
    addable: bool,
    on_change: impl Fn(Vec<f64>) + Clone + 'static,
) -> AnyView {
    let rows = {
        let on_change = on_change.clone();
        move || {
            let held = values.get();
            let named = plan.get();
            held.iter()
                .map(|hz| {
                    let hz = *hz;
                    let on_change = on_change.clone();
                    let name = channel_name(named.as_deref(), hz).unwrap_or_default();
                    view! {
                        row(class = "sc-entry") {
                            text {{format_mhz(Some(hz))}}
                            text(class = "sc-entry__name") {{name}}
                            {remove_button(format!("Remove {}", format_mhz(Some(hz))), move || {
                                on_change(without_hz(&values.get_untracked(), hz));
                            })}
                        }
                    }
                })
                .collect::<Vec<_>>()
        }
    };
    let add = addable.then(|| {
        setting_row(
            "Add",
            None,
            number_field(
                Signal::stored(None),
                NumberSpec::new("Add frequency")
                    .limit(NumberLimit {
                        min: Some(0.0),
                        max: None,
                        step: Some(0.0125),
                    })
                    .unit("MHz")
                    .optional("MHz"),
                move |added| {
                    if let Some(mhz) = added.filter(|mhz| *mhz > 0.0) {
                        on_change(with_hz(&values.get_untracked(), (mhz * 1e6).round()));
                    }
                },
            ),
        )
    });
    let count = move || {
        let count = values.get().len();
        if count == 0 {
            String::from("none")
        } else {
            count.to_string()
        }
    };
    let head = titled(
        title,
        AnyView::new(view! { text(class = "kc-name kc-name--tip") {{label}} }),
    );
    AnyView::new(view! {
        column(class = "kc-group") {
            row(class = "kc-group__head") {
                {head}
                text(class = "sc-count") {{count}}
            }
            column(class = "sc-list", on:wheel = |ev: &mut EventCx<'_, events::Wheel>| ev.stop_propagation()) {{rows}}
            {add}
        }
    })
}

const MODES: [(ScanMode, &str, &str); 3] = [
    (
        ScanMode::Targets,
        "Listed",
        "Stop on the first busy frequency",
    ),
    (
        ScanMode::CloseCall,
        "Strongest",
        "Stop on the strongest signal nearby",
    ),
    (
        ScanMode::All,
        "All",
        "Visit every busy frequency once, then finish",
    ),
];

fn mode_label(mode: ScanMode) -> &'static str {
    MODES
        .iter()
        .find(|(candidate, _, _)| *candidate == mode)
        .map_or("Listed", |(_, label, _)| label)
}

fn mode_hint(mode: ScanMode) -> &'static str {
    MODES
        .iter()
        .find(|(candidate, _, _)| *candidate == mode)
        .map_or("Scan mode", |(_, _, hint)| hint)
}

fn margin_row(store: Store, node: String, settings: Memo<ScanSettings>) -> AnyView {
    setting_row(
        "Over noise",
        Some("How far over the noise floor counts as busy"),
        number_field(
            Signal::derive(move || Some(f64::from(settings.get().margin_db))),
            NumberSpec::new("Scan margin")
                .limit(NumberLimit::new(1.0, 60.0, 1.0))
                .unit("dB"),
            move |next| {
                if let Some(next) = next {
                    edit_settings(store, &node, move |held| held.margin_db = next as f32);
                }
            },
        ),
    )
}

fn hold_row(store: Store, node: String, settings: Memo<ScanSettings>) -> AnyView {
    setting_row(
        "Hold",
        Some("Time on each busy frequency, long enough for the decoder to read it"),
        number_field(
            Signal::derive(move || Some(f64::from(settings.get().hold_ms) / 1_000.0)),
            NumberSpec::new("Hold time")
                .limit(NumberLimit::new(0.5, 120.0, 0.5))
                .unit("s"),
            move |next| {
                if let Some(seconds) = next {
                    edit_settings(store, &node, move |held| {
                        held.hold_ms = (seconds * 1_000.0).round() as u32;
                    });
                }
            },
        ),
    )
}

fn hold_or_priority(
    store: Store,
    node: String,
    settings: Memo<ScanSettings>,
    plan: Plan,
) -> AnyView {
    let all = Memo::new(move |_| settings.get().mode == ScanMode::All);
    AnyView::new(move || {
        let node = node.clone();
        if all.get() {
            return hold_row(store, node, settings);
        }
        freq_list(
            ("Priority", "Checked every 2 s and taken over anything else"),
            Signal::derive(move || settings.get().priority),
            plan,
            true,
            move |next| edit_settings(store, &node, move |held| held.priority = next),
        )
    })
}

fn lockouts(store: Store, node: String, settings: Memo<ScanSettings>, plan: Plan) -> AnyView {
    let any = Memo::new(move |_| !settings.get().lockouts.is_empty());
    AnyView::new(move || {
        let node = node.clone();
        any.get().then(|| {
            freq_list(
                ("Lockouts", "Never held. Skip adds to this list."),
                Signal::derive(move || settings.get().lockouts),
                plan,
                false,
                move |next| edit_settings(store, &node, move |held| held.lockouts = next),
            )
        })
    })
}

fn sweep_group(
    store: Store,
    node: String,
    settings: Memo<ScanSettings>,
    decoder: Memo<Option<Decoder>>,
    plan: Plan,
) -> AnyView {
    let mode = Signal::derive(move || Some(settings.get().mode));
    let pick_mode = {
        let node = node.clone();
        move |next: ScanMode| edit_settings(store, &node, move |held| held.mode = next)
    };
    let firmware = {
        let node = node.clone();
        move || {
            let node = node.clone();
            decoder
                .get()
                .is_some_and(|found| found.set.capabilities.hardware_sweep)
                .then(|| {
                    toggle_row(
                        "Firmware sweep",
                        Some("Let the radio sweep itself"),
                        Signal::derive(move || settings.get().hardware_sweep),
                        move |on| edit_settings(store, &node, move |held| held.hardware_sweep = on),
                    )
                })
        }
    };
    let modes: Vec<(ScanMode, String)> = MODES
        .iter()
        .map(|(mode, label, _)| (*mode, (*label).to_owned()))
        .collect();
    let hint = Memo::new(move |_| mode_hint(settings.get().mode));
    let find = move || {
        titled(
            hint.get(),
            setting_row("Find", None, pick(modes.clone(), mode, pick_mode.clone())),
        )
    };
    group(
        "Sweep",
        AnyView::new(()),
        view! {
            {find}
            {margin_row(store, node.clone(), settings)}
            {hold_or_priority(store, node.clone(), settings, plan)}
            {lockouts(store, node, settings, plan)}
            {firmware}
        },
    )
}

#[must_use]
fn running_stats(status: &ScannerStatus) -> Vec<Stat> {
    let mut shown = vec![
        Stat::new(
            "Targets",
            status.targets.to_string(),
            "Frequencies per sweep",
        ),
        Stat::new("Sweeps", status.sweeps.to_string(), "Sweeps finished"),
        Stat::new("Hits", status.hits.to_string(), "Signals held"),
    ];
    let locked = status.settings.lockouts.len();
    if locked > 0 {
        shown.push(Stat::new(
            "Locked",
            locked.to_string(),
            "Frequencies locked out",
        ));
    }
    shown
}

#[must_use]
fn idle_stats(settings: &ScanSettings) -> Vec<Stat> {
    if range_problem(settings).is_some() {
        return Vec::new();
    }
    vec![Stat::new(
        "Targets",
        scan_targets(settings).to_string(),
        "Frequencies per sweep",
    )]
}

fn next_range(settings: &ScanSettings) -> ScanRange {
    settings.ranges.last().copied().unwrap_or(ScanRange {
        start_hz: 145_600_000.0,
        stop_hz: 145_800_000.0,
        step_hz: 12_500.0,
    })
}

fn footer(
    store: Store,
    node: String,
    settings: Memo<ScanSettings>,
    decoder: Memo<Option<Decoder>>,
    status: Memo<Option<ScannerStatus>>,
    pushed: RwSignal<HashMap<(u32, u32), ScannerStatus>>,
) -> impl IntoView {
    let target = move || {
        decoder
            .get_untracked()
            .map(|found| (found.set.id, found.channel))
    };
    let holding = Signal::derive(move || {
        status
            .get()
            .is_some_and(|status| status.state == ScanState::Holding)
    });
    let done = Memo::new(move |_| {
        status
            .get()
            .is_some_and(|status| status.state == ScanState::Done)
    });
    let blocked =
        Signal::derive(move || decoder.get().is_none() || range_problem(&settings.get()).is_some());
    let keep_lockouts = {
        let node = node.clone();
        move |finished: ScannerStatus| {
            let lockouts = finished.settings.lockouts;
            edit_settings(store, &node, move |held| held.lockouts = lockouts);
        }
    };
    let add = {
        let node = node.clone();
        move || {
            edit_settings(store, &node, |held| {
                let next = next_range(held);
                held.ranges.push(next);
            });
        }
    };
    let start = move || {
        let Some(at) = target() else { return };
        let request = ScanRequest {
            action: ScanAction::Start,
            settings: Some(ScanSettings {
                channel: at.1,
                ..settings.get_untracked()
            }),
        };
        send(store, at, request, |_| {});
    };
    let skip = {
        let keep = keep_lockouts.clone();
        move || {
            if let Some(at) = target() {
                let request = ScanRequest {
                    action: ScanAction::Skip,
                    settings: None,
                };
                send(store, at, request, keep.clone());
            }
        }
    };
    let stop = move || {
        if let Some(at) = target() {
            let keep = keep_lockouts.clone();
            let request = ScanRequest {
                action: ScanAction::Stop,
                settings: None,
            };
            send(store, at, request, move |finished| {
                keep(finished);
                pushed.update(|held| {
                    held.remove(&at);
                });
            });
        }
    };
    move || {
        let add = add.clone();
        if status.get().is_some() {
            let stop = stop.clone();
            AnyView::new(view! {
                row(class = "sc-foot") {
                    {kit_audio::stats(move || status.get().map(|status| running_stats(&status)).unwrap_or_default())}
                    {titled("Leave this frequency and lock it out", AnyView::new(button("Skip", "kc-btn", Signal::derive(move || !holding.get()), skip.clone())))}
                    {move || {
                        let stop = stop.clone();
                        if done.get() {
                            AnyView::new(button("Close", "kc-btn", false, stop))
                        } else {
                            AnyView::new(button("Stop scan", "kc-btn kc-btn--danger", false, stop))
                        }
                    }}
                }
            })
        } else {
            let start_button = AnyView::new(button(
                "Start scan",
                "kc-btn kc-btn--primary",
                blocked,
                start,
            ));
            let start_button = if decoder.get().is_none() {
                titled("Wire this node's control out to a decoder", start_button)
            } else {
                start_button
            };
            AnyView::new(view! {
                row(class = "sc-foot") {
                    {kit_audio::stats(move || idle_stats(&settings.get()))}
                    {button("Add range", "kc-btn", false, add)}
                    {start_button}
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::scan::ScanState;

    use super::*;

    fn status(lockouts: Vec<f64>) -> ScannerStatus {
        ScannerStatus {
            state: ScanState::Scanning,
            settings: ScanSettings {
                lockouts,
                ..ScanSettings::for_channel(1)
            },
            targets: 10,
            first_hz: 0.0,
            last_hz: 0.0,
            current_hz: 0.0,
            current_snr_db: None,
            sweeps: 3,
            hits: 2,
            hardware_sweep: false,
            error: None,
        }
    }

    #[test]
    fn a_running_scan_counts_lockouts_only_once_there_are_some() {
        let labels =
            |shown: Vec<Stat>| shown.into_iter().map(|stat| stat.label).collect::<Vec<_>>();
        assert_eq!(
            labels(running_stats(&status(vec![]))),
            vec!["Targets", "Sweeps", "Hits"]
        );
        assert_eq!(
            labels(running_stats(&status(vec![1.0]))),
            vec!["Targets", "Sweeps", "Hits", "Locked"]
        );
    }

    #[test]
    fn an_idle_scan_counts_targets_only_when_it_could_start() {
        assert!(idle_stats(&ScanSettings::for_channel(1)).is_empty());
        let listed = ScanSettings {
            frequencies: vec![446e6, 446.1e6],
            ..ScanSettings::for_channel(1)
        };
        assert_eq!(idle_stats(&listed)[0].value.as_deref(), Some("2"));
    }

    #[test]
    fn a_new_range_repeats_the_last_one() {
        let mut held = ScanSettings::for_channel(1);
        assert_eq!(next_range(&held).step_hz, 12_500.0);
        held.ranges.push(ScanRange {
            start_hz: 1.0,
            stop_hz: 2.0,
            step_hz: 3.0,
        });
        assert_eq!(next_range(&held).step_hz, 3.0);
    }

    #[test]
    fn every_mode_has_a_label_and_a_hint() {
        for (mode, label, hint) in MODES {
            assert_eq!(mode_label(mode), label);
            assert_eq!(mode_hint(mode), hint);
        }
    }
}
