use std::collections::HashMap;

use sdrmm_wire::{
    channel::{
        BandMiss, ChannelSettings, MAX_SQUELCH_AUTO_MARGIN_DB, MIN_SQUELCH_AUTO_MARGIN_DB, Squelch,
    },
    decode::BroadcastStatus,
    patch::NodeBody,
    ws::ServerEvent,
};
use zgui::prelude::*;

use super::{
    ANY_FREQUENCY, FaceStatus,
    band_miss::band_miss_of,
    binding::{
        ChannelBinding, iq_lanes_of, radio_is_attached, radio_refs_of, tuning_controller_of,
    },
    controls::Ctx,
    face_status,
    health::{DROPS_HINT, broadcast_stat, broadcast_title, channel_queue_summary},
    heard_hz, hotkey_of, in_tuning_range, lane_center_hz, lane_rate_hz, modes, picker, run_hotkey,
    settings::{RadioWindow, channel_has_audio, channel_width_hz, radio_window_hz, reaches_hz},
    swap, tuning_range,
};
use crate::{
    head::{self, NodeHead, Tone},
    store::Store,
    ui::{
        faces::processor::count,
        kit_channel::{
            self, NumberSpec, SWAP_ICON, StatTone, blanker_control, button, format_hz, format_mhz,
            icon, number_field, setting_row, signal, stat, tip, tune_to, tuning_lock,
        },
        kit_sources::{DialSpec, dial::Reach, frequency_dial},
    },
};

use super::settings::NumberLimit;

const SHEET: &str = css!(
    r#"
.ch-face { flex-direction: column; }
.ch-top { flex-direction: column; gap: 6px; padding: 8px; border-bottom: 1px solid var(--line); }
.ch-meta { align-items: center; gap: 8px; min-height: 18px; }
.ch-dialrow { align-items: center; gap: 4px; min-width: 0; }
.ch-tools { margin-left: auto; align-items: center; gap: 4px; flex: 0 0 auto; }
.ch-body { flex-direction: column; gap: 8px; padding: 12px; }
.ch-foot { align-items: center; justify-content: flex-end; gap: 8px; padding: 6px 12px; border-top: 1px solid var(--line); }
"#
);

const SATELLITE: &str = "satellite";
const SCANNING: &str = "scanning";

#[derive(Clone, Debug, PartialEq)]
struct View {
    settings: Option<ChannelSettings>,
    live: Option<(u32, u32)>,
    binding: ChannelBinding,
    status: Option<FaceStatus>,
    width_hz: Option<f64>,
    window: Option<RadioWindow>,
    range: (f64, f64),
    locked: bool,
    controller: Option<String>,
}

fn carrier_of(store: Store, node: &str, stream: u32, set_label: Option<String>) -> Option<String> {
    let graph = store.graph.get();
    let lanes = iq_lanes_of(&graph, node);
    if lanes.len() < 2 {
        return None;
    }
    let lane = lanes
        .iter()
        .find(|lane| lane.stream == stream)
        .or(lanes.first())?;
    graph
        .node(&lane.source)
        .and_then(|found| found.label.clone())
        .or(set_label)
}

fn view_of(store: Store, node: &str, tracked: Option<String>) -> View {
    let graph = store.graph.get();
    let descriptor = store.channel_descriptor(node);
    let settings = store.channel_settings(node);
    let live = store.live_channel(node);
    let lanes = iq_lanes_of(&graph, node);
    let set = store.device_set_of(node).and_then(|id| store.set_of(id));
    let references = radio_refs_of(&graph, node);
    let binding = ChannelBinding::of(
        !lanes.is_empty(),
        set.is_some(),
        !references.is_empty(),
        radio_is_attached(&references, &store.devices.get()),
    );
    let stream = live.as_ref().map_or_else(
        || lanes.first().map_or(0, |lane| lane.stream),
        |live| live.channel.stream,
    );
    let center_hz = set.as_ref().and_then(|set| lane_center_hz(set, stream));
    let span_hz = set.as_ref().and_then(|set| lane_rate_hz(set, stream));
    let window = radio_window_hz(center_hz, span_hz, descriptor.as_ref());
    let reaches = set.is_none()
        || settings
            .as_ref()
            .is_none_or(|settings| reaches_hz(settings.frequency_hz, window));
    let miss: Option<BandMiss> =
        band_miss_of(live.as_ref().map(|live| live.channel.out_of_band), reaches);
    let scanned = live.as_ref().is_some_and(|live| {
        set.as_ref().is_some_and(|set| {
            set.scanners.iter().any(|scanner| {
                scanner.error.is_none() && scanner.settings.channel == live.channel.id
            })
        })
    });
    let driver = if scanned {
        Some(String::from(SCANNING))
    } else {
        tracked.clone()
    };
    let carrier = carrier_of(
        store,
        node,
        stream,
        set.as_ref().map(|set| set.device.label.clone()),
    );
    let locked = matches!(graph.node(node).map(|found| &found.body), Some(NodeBody::Channel(channel)) if channel.tuning_locked);
    let controller = tuning_controller_of(&graph, node).map(|kind| tracked.unwrap_or(kind));
    View {
        width_hz: channel_width_hz(
            settings.as_ref().map(|settings| &settings.params),
            descriptor.as_ref(),
        ),
        settings,
        live: live.as_ref().map(|live| (live.device_set, live.channel.id)),
        binding,
        status: face_status(live.is_some(), binding, miss, driver, carrier),
        window,
        range: set
            .as_ref()
            .map_or(ANY_FREQUENCY, |set| tuning_range(&set.capabilities)),
        locked,
        controller,
    }
}

#[must_use]
fn head_of(view: &View) -> NodeHead {
    let said = view
        .status
        .as_ref()
        .map_or_else(NodeHead::default, |status| {
            let tone = if status.warn { Tone::Warn } else { Tone::Plain };
            let said = NodeHead::said(status.text.clone()).toned(tone);
            if status.title.is_empty() {
                said
            } else {
                said.titled(status.title.clone())
            }
        });
    match view.width_hz {
        Some(width) => said.badged(format_hz(width)),
        None => said,
    }
}

fn watch_satellites(store: Store, node: String) -> Signal<Option<String>> {
    let tracked = RwSignal::new(HashMap::<String, Option<String>>::new());
    let wanted = node.clone();
    store.on_event(move |event| {
        if let ServerEvent::SatelliteUpdate { status } = event {
            let driving = status.driving.contains(&wanted);
            tracked.update(|held| {
                if driving {
                    held.insert(status.node.clone(), status.name.clone());
                } else {
                    held.remove(&status.node);
                }
            });
        }
    });
    Signal::derive(move || {
        tracked
            .get()
            .values()
            .next()
            .map(|name| name.clone().unwrap_or_else(|| String::from(SATELLITE)))
    })
}

fn watch_broadcast(
    store: Store,
    live: Signal<Option<(u32, u32)>>,
) -> Signal<Option<BroadcastStatus>> {
    Signal::derive(move || {
        let at = live.get()?;
        store.broadcasts.with(|held| held.get(&at).cloned())
    })
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("channel-face", SHEET);
    kit_channel::install();
    let tracked = watch_satellites(store, node.clone());
    let shown = {
        let node = node.clone();
        Memo::new(move |_| view_of(store, &node, tracked.get()))
    };
    head::show(store, node.clone(), move || shown.with(head_of));
    let live = Signal::derive(move || shown.get().live);
    let broadcast = watch_broadcast(store, live);
    let replacing = RwSignal::new(false);
    let has_settings = Memo::new(move |_| shown.get().settings.is_some());
    let top = {
        let node = node.clone();
        move || {
            has_settings
                .get()
                .then(|| top(store, node.clone(), shown, replacing))
        }
    };
    let body = {
        let node = node.clone();
        move || {
            has_settings
                .get()
                .then(|| controls(store, node.clone(), shown, broadcast))
        }
    };
    let replace = {
        let node = node.clone();
        move || {
            replacing.get().then(|| {
                AnyView::new(picker::replace_decoder(store, node.clone(), move || {
                    replacing.set(false)
                }))
            })
        }
    };
    let keys = {
        let node = node.clone();
        move |ev: &mut EventCx<'_, events::KeyDown>| {
            if ev.modifiers.control() || ev.modifiers.meta() || ev.modifiers.alt() {
                return;
            }
            if let Some(key) = ev.key.as_str().and_then(hotkey_of) {
                run_hotkey(store, &node, key);
                ev.prevent_default();
            }
        }
    };
    view! {
        column(class = "ch-face", on:key_down = keys) {
            {top}
            {body}
            {footer(store, shown, broadcast)}
            {replace}
        }
    }
}

fn dial_row(store: Store, node: String, shown: Memo<View>, hz: Signal<f64>) -> AnyView {
    let locked = Signal::derive(move || {
        let view = shown.get();
        view.locked || view.controller.is_some()
    });
    let tune = {
        let node = node.clone();
        move |value: f64| {
            let view = shown.get_untracked();
            if view.locked || view.controller.is_some() {
                return;
            }
            let value = value.clamp(view.range.0, view.range.1);
            store.edit_channel(&node, move |settings| settings.frequency_hz = value);
        }
    };
    let hint = Signal::derive(move || {
        let view = shown.get();
        match view.window {
            Some(window) => format!(
                "The radio hears {} to {}",
                format_mhz(window.low_hz),
                format_mhz(window.high_hz)
            ),
            None => format!(
                "Reaches {} to {}",
                format_mhz(view.range.0),
                format_mhz(view.range.1)
            ),
        }
    });
    let resolve = move |entered: f64| in_tuning_range(entered, shown.get_untracked().range);
    let hold = Signal::derive(move || {
        shown
            .get()
            .controller
            .map(|by| format!("Tuned by {by}. Unwire its control to tune by hand."))
    });
    let lock = {
        let node = node.clone();
        move |on: bool| {
            store.edit_node(&node, move |body| {
                if let NodeBody::Channel(channel) = body {
                    channel.tuning_locked = on;
                }
            });
        }
    };
    let spec = DialSpec {
        hz,
        reach: Signal::derive(move || {
            let (min, max) = shown.get().range;
            Reach { min, max }
        }),
        disabled: locked,
        wheel: Signal::derive(move || store.selected.get().as_deref() == Some(node.as_str())),
    };
    let dial_tune = tune.clone();
    AnyView::new(view! {
        row(class = "ch-dialrow") {
            {frequency_dial(spec, dial_tune)}
            row(class = "ch-tools") {
                {tune_to("Type a frequency to listen on", hz, hint, resolve, locked, tune)}
                {tuning_lock(locked, "Frequency locked", "Lock frequency", hold, lock)}
            }
        }
    })
}

fn top(store: Store, node: String, shown: Memo<View>, replacing: RwSignal<bool>) -> AnyView {
    let level = Signal::derive(move || {
        let at = shown.get().live?;
        store.levels.with(|levels| levels.get(&at).copied())
    });
    let hz = Signal::derive(move || {
        let tuned = shown
            .get()
            .settings
            .map_or(0.0, |settings| settings.frequency_hz);
        heard_hz(tuned, level.get().and_then(|level| level.shift_hz))
    });
    let audio = channel_has_audio(store.channel_descriptor(&node).as_ref());
    let is_live = Memo::new(move |_| shown.get().live.is_some());
    let squelch = Signal::derive(move || {
        shown
            .get()
            .settings
            .map(|settings| settings.squelch)
            .unwrap_or_default()
    });
    let meter = {
        let node = node.clone();
        move || {
            if !is_live.get() {
                return None;
            }
            if !audio {
                return Some(signal::signal_row(level));
            }
            let node = node.clone();
            Some(signal::squelch_row(level, squelch, move |next: Squelch| {
                store.edit_channel(&node, move |settings| settings.squelch = next);
            }))
        }
    };
    AnyView::new(view! {
        column(class = "ch-top") {
            {meta(replacing)}
            {dial_row(store, node, shown, hz)}
            {meter}
        }
    })
}

fn meta(replacing: RwSignal<bool>) -> impl IntoView {
    view! {
        row(class = "ch-meta") {
            spacer() {}
            {tip("Replace the decoder", AnyView::new(view! {
                control(
                    class = "kc-icon",
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:label = "Replace the decoder",
                    on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                    on:click:stop = move |_| replacing.set(true)
                ) {
                    {icon(SWAP_ICON, "kc-glyph kc-glyph--sm")}
                }
            }))}
        }
    }
}

fn controls(
    store: Store,
    node: String,
    shown: Memo<View>,
    broadcast: Signal<Option<BroadcastStatus>>,
) -> AnyView {
    let descriptor = store.channel_descriptor(&node);
    let kind = swap::current_type(&store.graph.get_untracked(), &node).unwrap_or_default();
    let audio = channel_has_audio(descriptor.as_ref());
    let ctx = Ctx {
        store,
        node: StoredValue::new(node.clone()),
        params: Signal::derive(move || shown.get().settings.map(|settings| settings.params)),
        limits: StoredValue::new(
            descriptor
                .as_ref()
                .map(|descriptor| descriptor.limits.clone())
                .unwrap_or_default(),
        ),
        broadcast,
    };
    let margin = audio.then(|| margin_row(store, node.clone(), shown));
    let blanker = audio.then(|| {
        let node = node.clone();
        blanker_control(
            Signal::derive(move || {
                shown
                    .get()
                    .settings
                    .map(|settings| settings.blanker)
                    .unwrap_or_default()
            }),
            move |blanker| store.edit_channel(&node, move |settings| settings.blanker = blanker),
        )
    });
    AnyView::new(view! {
        column(class = "ch-body kc-grid") {
            {margin}
            {modes::mode_view(ctx, &kind)}
            {blanker}
        }
    })
}

fn margin_row(store: Store, node: String, shown: Memo<View>) -> AnyView {
    let margin = Memo::new(move |_| {
        shown
            .get()
            .settings
            .and_then(|settings| settings.squelch.auto_margin_db())
    });
    let row = move || {
        margin.get()?;
        let node = node.clone();
        let spec = NumberSpec::new("Squelch margin")
            .limit(NumberLimit::new(
                f64::from(MIN_SQUELCH_AUTO_MARGIN_DB),
                f64::from(MAX_SQUELCH_AUTO_MARGIN_DB),
                1.0,
            ))
            .unit("dB");
        Some(setting_row(
            "Margin",
            Some("How far above the measured noise floor the squelch opens"),
            number_field(
                Signal::derive(move || margin.get().map(f64::from)),
                spec,
                move |next| {
                    if let Some(next) = next {
                        store.edit_channel(&node, move |settings| {
                            settings.squelch = Squelch::Auto {
                                margin_db: next as f32,
                            };
                        });
                    }
                },
            ),
        ))
    };
    AnyView::new(row)
}

fn health(store: Store, at: (u32, u32), broadcast: Signal<Option<BroadcastStatus>>) -> AnyView {
    let summary = Memo::new(move |_| {
        store
            .pipeline
            .with(|queues| channel_queue_summary(queues, at.0, at.1))
    });
    let lock = move || {
        let status = broadcast.get()?;
        let shown = broadcast_stat(&status);
        let tone = if shown.locked {
            StatTone::Ok
        } else {
            StatTone::Warn
        };
        Some(stat(
            shown.label,
            shown.value,
            move || broadcast_title(&status),
            tone,
        ))
    };
    let queue = move || {
        let held = summary.get()?;
        let detail = held.detail.clone();
        let drops = (held.dropped > 0).then(|| {
            stat(
                "Drops",
                Some(count(held.dropped)),
                || DROPS_HINT.to_owned(),
                StatTone::Warn,
            )
        });
        Some(view! {
            {stat("Queue", Some(format!("{:.0} ms", held.oldest_ms)), move || detail.clone(), StatTone::Plain)}
            {drops}
        })
    };
    AnyView::new(view! {
        row(class = "kc-stats") {
            {lock}
            {queue}
        }
    })
}

fn footer(
    store: Store,
    shown: Memo<View>,
    broadcast: Signal<Option<BroadcastStatus>>,
) -> impl IntoView {
    let live = Memo::new(move |_| shown.get().live);
    let action = Memo::new(move |_| {
        let view = shown.get();
        view.live
            .is_none()
            .then_some(view.binding)
            .and_then(|binding| binding.action().map(|label| (label, binding.hint())))
    });
    move || {
        let stats = live.get().map(|at| health(store, at, broadcast));
        let button = action.get().map(|(label, hint)| {
            tip(
                hint,
                AnyView::new(button(
                    label,
                    "kc-btn kc-btn--primary",
                    Signal::stored(false),
                    move || {
                        zgui::task::spawn_local(async move { store.apply().await });
                    },
                )),
            )
        });
        (stats.is_some() || button.is_some()).then(|| {
            view! {
                row(class = "ch-foot") {
                    {stats}
                    {button}
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(status: Option<FaceStatus>, width_hz: Option<f64>) -> View {
        View {
            settings: None,
            live: None,
            binding: ChannelBinding::NotStarted,
            status,
            width_hz,
            window: None,
            range: ANY_FREQUENCY,
            locked: false,
            controller: None,
        }
    }

    #[test]
    fn the_head_carries_the_status_and_the_width_badge() {
        assert_eq!(head_of(&view(None, None)), NodeHead::default());
        let missed = FaceStatus {
            text: String::from("crowded out"),
            title: String::new(),
            warn: true,
        };
        assert_eq!(
            head_of(&view(Some(missed), Some(12_500.0))),
            NodeHead::said("crowded out")
                .toned(Tone::Warn)
                .badged("12.5 kHz")
        );
    }
}
