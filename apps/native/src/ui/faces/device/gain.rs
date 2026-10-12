use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
    sync::Arc,
};

use sdrmm_wire::{
    device::{AgcSetting, DeviceSettings, GainStage, GainValue},
    state::DeviceSet,
    ws::ServerEvent,
};
use zgui::prelude::*;

use super::{
    caps::{format_gain, gain_label, gain_unit, setting_index, snap_to_stage, stage_settings},
    lanes::{agc_delta, agc_gain_db, agc_tip, lane_agc, stream_label},
    layout::{
        LaneLayout, Pressed, agc_drives, all_lanes_gain, flip_input, input_options, lane_gain,
        lane_gains, lane_input_name, lane_layout, meter_stage, picked_inputs, pressed_of,
        rx_stages, spread_of, stage_tip, tx_stages,
    },
    radio::Radio,
};
use crate::{
    store::Store,
    ui::{
        kit_sources::{
            debounced,
            meter::{GainMeter, MeterTone, gain_meter, meter_tone},
            tip, tip_some,
        },
        widgets::check,
    },
};

pub type Peaks = RwSignal<Arc<HashMap<u32, f32>>>;

#[derive(Clone)]
pub struct LaneExtras {
    pub advised: Signal<BTreeSet<u32>>,
    pub held_by: Signal<BTreeMap<u32, String>>,
    pub split: Signal<bool>,
    pub lane_lead: Option<Rc<dyn Fn(u32) -> AnyView>>,
}

impl LaneExtras {
    #[must_use]
    pub fn none() -> Self {
        Self {
            advised: Signal::stored(BTreeSet::new()),
            held_by: Signal::stored(BTreeMap::new()),
            split: Signal::stored(false),
            lane_lead: None,
        }
    }
}

pub fn lane_peaks(store: Store, radio: Radio) -> Peaks {
    let peaks: Peaks = RwSignal::new(Arc::default());
    store.on_event(move |event| {
        let ServerEvent::ChannelLevels {
            device_set, lanes, ..
        } = event
        else {
            return;
        };
        let mine = radio
            .set
            .with_untracked(|set| set.as_ref().map(|set| set.id))
            == Some(*device_set);
        if mine && !lanes.is_empty() {
            let next: HashMap<u32, f32> = lanes
                .iter()
                .map(|lane| (lane.stream, lane.peak_db))
                .collect();
            peaks.try_set(Arc::new(next));
        }
    });
    peaks
}

#[derive(Clone)]
struct Lanes {
    radio: Radio,
    extras: LaneExtras,
    peaks: Peaks,
    metered: Option<GainStage>,
    layout: LaneLayout,
}

impl Lanes {
    fn held(&self, stream: u32) -> Signal<Option<String>> {
        let held_by = self.extras.held_by;
        Signal::derive(move || held_by.with(|held| held.get(&stream).cloned()))
    }

    fn peak(&self, stream: u32) -> Signal<Option<f32>> {
        let peaks = self.peaks;
        Signal::derive(move || peaks.with(|peaks| peaks.get(&stream).copied()))
    }

    fn clipping(&self, stream: u32) -> Signal<bool> {
        let radio = self.radio;
        Signal::derive(move || {
            radio
                .read(|set| set.clipping.contains(&stream))
                .unwrap_or(false)
        })
    }
}

pub fn gain_lanes(radio: Radio, set: &DeviceSet, extras: LaneExtras, peaks: Peaks) -> Vec<AnyView> {
    let caps = &set.capabilities;
    let layout = lane_layout(caps);
    let stages = rx_stages(caps);
    let transmit = tx_stages(caps);
    let leads = extras.split.get_untracked() && extras.lane_lead.is_some();
    if stages.is_empty() && transmit.is_empty() && !layout.inputs && !leads {
        return Vec::new();
    }
    let lanes = Lanes {
        radio,
        extras,
        peaks,
        metered: meter_stage(caps),
        layout,
    };
    let mut rows = Vec::new();
    if layout.inputs {
        rows.push(inputs_row(radio, set));
    }
    if let Some(metered) = lanes.metered.clone().filter(|_| layout.master) {
        rows.push(master_row(&lanes, metered));
    }
    if layout.lanes > 1 || !layout.master || leads {
        for stream in 0..layout.lanes {
            rows.extend(lane_rows(&lanes, set, stream, &stages, leads));
        }
    }
    rows.extend(transmit.into_iter().map(|stage| tx_row(radio, stage)));
    rows
}

fn inputs_row(radio: Radio, set: &DeviceSet) -> AnyView {
    let options = input_options(&set.capabilities);
    let picked = Signal::derive(move || {
        radio
            .read(|set| picked_inputs(&set.settings))
            .unwrap_or_default()
    });
    let items: Vec<AnyView> = options
        .into_iter()
        .map(|(input, name)| {
            let on = Signal::derive(move || picked.with(|picked| picked.contains(&input)));
            let last =
                Signal::derive(move || picked.with(|picked| picked.as_slice() == [input]));
            let item = move || {
                let name = name.clone();
                AnyView::new(view! {
                    control(
                        class = "seg__item",
                        tabindex = Focus::Sequential,
                        a11y:role = Role::Button,
                        class:on = on,
                        state:disabled = last,
                        on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                        on:click:stop = move |_| {
                            if last.get_untracked() {
                                return;
                            }
                            let next = flip_input(&picked.get_untracked(), input);
                            radio.patch(DeviceSettings { rx_inputs: Some(next), ..DeviceSettings::default() });
                        }
                    ) {
                        {name}
                    }
                })
            };
            AnyView::new(tip_some(
                Signal::derive(move || last.get().then(|| "Last one stays on".to_owned())),
                item,
            ))
        })
        .collect();
    tip(
        || "Receivers streamed. One gets the whole link".to_owned(),
        AnyView::new(view! { row(class = "seg kit-inputs") {{items}} }),
    )
}

fn agc_now(radio: Radio, stream: u32) -> Option<AgcSetting> {
    radio.read_untracked(|set| lane_agc(set, stream))
}

fn turn_agc(radio: Radio, stream: u32, on: bool) {
    let delta = radio.read_untracked(|set| {
        let mut agc = lane_agc(set, stream);
        agc.on = on;
        agc_delta(&set.capabilities, stream, agc)
    });
    if let Some(delta) = delta {
        radio.patch(delta);
    }
}

fn lane_rows(
    lanes: &Lanes,
    set: &DeviceSet,
    stream: u32,
    stages: &[GainStage],
    leads: bool,
) -> Vec<AnyView> {
    let radio = lanes.radio;
    let caps = &set.capabilities;
    let lane_name = lanes.layout.per_lane.then(|| {
        lane_input_name(set, stream)
            .unwrap_or_else(|| stream_label("iq", stream, lanes.layout.lanes))
    });
    let agc_here = caps.agc.offered() && (caps.per_stream.agc || stream == 0);
    let lead = leads
        .then(|| lanes.extras.lane_lead.as_ref().map(|lead| lead(stream)))
        .flatten();
    let has_lead = lead.is_some();
    let mut rows: Vec<AnyView> = lead.into_iter().collect();
    for (index, stage) in stages.iter().enumerate() {
        let label = match (&lane_name, index) {
            (Some(_), 0) if has_lead => RowLabel::Plain(String::new()),
            (Some(name), 0) => RowLabel::Port(name.clone()),
            _ => RowLabel::Plain(gain_label(stage)),
        };
        let metered = lanes.metered.as_ref() == Some(stage);
        let driven = {
            let stage = stage.clone();
            Signal::derive(move || {
                radio
                    .read(|set| agc_drives(&stage, &lane_agc(set, stream)))
                    .unwrap_or(false)
            })
        };
        let value = {
            let stage = stage.clone();
            Signal::derive(move || {
                radio
                    .read(|set| {
                        let value = lane_gains(set, &stage)
                            .get(stream as usize)
                            .copied()
                            .unwrap_or(stage.range.min);
                        if agc_drives(&stage, &lane_agc(set, stream)) {
                            agc_gain_db(set, stream).unwrap_or(value)
                        } else {
                            value
                        }
                    })
                    .unwrap_or(stage.range.min)
            })
        };
        let peak = lanes.peak(stream);
        let clipping = lanes.clipping(stream);
        let trailing = (metered && agc_here).then(|| {
            agc_auto(
                radio,
                stream,
                lane_name.clone(),
                lanes.extras.advised,
                lanes.held(stream),
            )
        });
        let commit_stage = stage.clone();
        rows.push(stage_row(StageRow {
            label,
            lane: lane_name.clone(),
            stage: stage.clone(),
            value,
            readout: Signal::stored(None),
            auto: driven,
            peak: metered.then_some(peak),
            tone: Signal::derive(move || meter_tone(peak.get(), clipping.get())),
            held_by: lanes.held(stream),
            trailing,
            commit: Rc::new(move |value_db| {
                if let Some(agc) = agc_now(radio, stream)
                    && agc_drives(&commit_stage, &agc)
                {
                    turn_agc(radio, stream, false);
                }
                let delta = radio.read_untracked(|set| {
                    lane_gain(&set.capabilities, stream, &commit_stage, value_db)
                });
                if let Some(delta) = delta {
                    radio.patch(delta);
                }
            }),
        }));
    }
    rows
}

fn master_row(lanes: &Lanes, stage: GainStage) -> AnyView {
    let radio = lanes.radio;
    let count = lanes.layout.lanes;
    let driven = {
        let stage = stage.clone();
        Signal::derive(move || {
            radio
                .read(|set| {
                    (0..count)
                        .map(|stream| agc_drives(&stage, &lane_agc(set, stream)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    };
    let gains = {
        let stage = stage.clone();
        Signal::derive(move || {
            radio
                .read(|set| {
                    lane_gains(set, &stage)
                        .into_iter()
                        .enumerate()
                        .map(|(stream, value)| {
                            let stream = stream as u32;
                            if agc_drives(&stage, &lane_agc(set, stream)) {
                                agc_gain_db(set, stream).unwrap_or(value)
                            } else {
                                value
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    };
    let spread = Signal::derive(move || gains.with(|gains| spread_of(gains)));
    let pressed = Signal::derive(move || driven.with(|driven| pressed_of(driven)));
    let peaks = lanes.peaks;
    let peak = Signal::derive(move || {
        peaks.with(|peaks| {
            (0..count)
                .filter_map(|stream| peaks.get(&stream).copied())
                .filter(|db| db.is_finite())
                .reduce(f32::max)
        })
    });
    let clipping = Signal::derive(move || {
        radio
            .read(|set| set.clipping.iter().any(|lane| *lane < count))
            .unwrap_or(false)
    });
    let held_by = lanes.extras.held_by;
    let held = Signal::derive(move || held_by.with(|held| held.values().next().cloned()));
    let set_all = move |on: bool| {
        let deltas = radio.read_untracked(|set| {
            (0..count)
                .filter_map(|stream| {
                    let mut agc = lane_agc(set, stream);
                    (agc.on != on).then(|| {
                        agc.on = on;
                        agc_delta(&set.capabilities, stream, agc)
                    })
                })
                .collect::<Vec<_>>()
        });
        for delta in deltas.into_iter().flatten() {
            radio.patch(delta);
        }
    };
    let offered = radio
        .read_untracked(|set| set.capabilities.agc.offered())
        .unwrap_or(false);
    let trailing = offered.then(|| {
        let title = Signal::derive(move || match held.get() {
            Some(array) => format!("Set on {array}"),
            None => radio.read(|set| agc_tip(set, 0, false)).unwrap_or_default(),
        });
        auto_toggle(AutoSpec {
            label: "Automatic gain on every lane".to_owned(),
            pressed,
            title,
            warn: Signal::stored(false),
            disabled: Signal::derive(move || held.with(Option::is_some)),
            on_change: Rc::new(set_all),
        })
    });
    let commit_stage = stage.clone();
    stage_row(StageRow {
        label: RowLabel::Legend("All".to_owned()),
        lane: None,
        stage,
        value: Signal::derive(move || spread.get().mean),
        readout: Signal::derive(move || (!spread.get().uniform).then(|| "mixed".to_owned())),
        auto: Signal::derive(move || pressed.get() == Pressed::On),
        peak: Some(peak),
        tone: Signal::derive(move || meter_tone(peak.get(), clipping.get())),
        held_by: held,
        trailing,
        commit: Rc::new(move |value_db| {
            set_all(false);
            let delta = radio
                .read_untracked(|set| all_lanes_gain(&set.capabilities, &commit_stage, value_db));
            if let Some(delta) = delta {
                radio.patch(delta);
            }
        }),
    })
}

fn tx_row(radio: Radio, stage: GainStage) -> AnyView {
    let value = {
        let stage = stage.clone();
        Signal::derive(move || {
            radio
                .read(|set| set.settings.gain(&stage.name))
                .flatten()
                .unwrap_or(stage.range.min)
        })
    };
    let name = stage.name.clone();
    stage_row(StageRow {
        label: RowLabel::Legend("TX".to_owned()),
        lane: None,
        stage,
        value,
        readout: Signal::stored(None),
        auto: Signal::stored(false),
        peak: None,
        tone: Signal::stored(MeterTone::Ok),
        held_by: Signal::stored(None),
        trailing: None,
        commit: Rc::new(move |value_db| {
            radio.patch(DeviceSettings {
                gains: vec![GainValue {
                    stage: name.clone(),
                    value_db,
                }],
                ..DeviceSettings::default()
            });
        }),
    })
}

struct AutoSpec {
    label: String,
    pressed: Signal<Pressed>,
    title: Signal<String>,
    warn: Signal<bool>,
    disabled: Signal<bool>,
    on_change: Rc<dyn Fn(bool)>,
}

fn auto_toggle(spec: AutoSpec) -> AnyView {
    let AutoSpec {
        label,
        pressed,
        title,
        warn,
        disabled,
        on_change,
    } = spec;
    tip(
        move || title.get(),
        AnyView::new(view! {
            control(
                class = "kit-auto-btn",
                class:on = move || pressed.get() == Pressed::On,
                class:mixed = move || pressed.get() == Pressed::Mixed,
                class:warn = warn,
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                a11y:label = label,
                state:disabled = disabled,
                on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                on:click:stop = move |_| {
                    if !disabled.get_untracked() {
                        on_change(pressed.get_untracked() != Pressed::On);
                    }
                }
            ) {
                "Auto"
            }
        }),
    )
}

fn agc_auto(
    radio: Radio,
    stream: u32,
    port: Option<String>,
    advised: Signal<BTreeSet<u32>>,
    held: Signal<Option<String>>,
) -> AnyView {
    let lane_advised = Signal::derive(move || advised.with(|lanes| lanes.contains(&stream)));
    let on = Signal::derive(move || radio.read(|set| lane_agc(set, stream).on).unwrap_or(false));
    let title = Signal::derive(move || match held.get() {
        Some(array) => format!("Set on {array}"),
        None => radio
            .read(|set| agc_tip(set, stream, lane_advised.get()))
            .unwrap_or_default(),
    });
    auto_toggle(AutoSpec {
        label: port.map_or_else(
            || "automatic gain".to_owned(),
            |port| format!("{port} automatic gain"),
        ),
        pressed: Signal::derive(move || if on.get() { Pressed::On } else { Pressed::Off }),
        title,
        warn: Signal::derive(move || on.get() && lane_advised.get()),
        disabled: Signal::derive(move || held.with(Option::is_some)),
        on_change: Rc::new(move |next| turn_agc(radio, stream, next)),
    })
}

#[derive(Clone)]
enum RowLabel {
    Plain(String),
    Port(String),
    Legend(String),
}

struct StageRow {
    label: RowLabel,
    lane: Option<String>,
    stage: GainStage,
    value: Signal<f64>,
    readout: Signal<Option<String>>,
    auto: Signal<bool>,
    peak: Option<Signal<Option<f32>>>,
    tone: Signal<MeterTone>,
    held_by: Signal<Option<String>>,
    trailing: Option<AnyView>,
    commit: Rc<dyn Fn(f64)>,
}

fn row_name(label: RowLabel, unit_tip: Signal<Option<String>>) -> impl IntoView {
    let (class, text) = match label {
        RowLabel::Plain(text) => ("kit-gain-row__name", text),
        RowLabel::Port(text) => ("kit-gain-row__name kit-gain-row__port", text),
        RowLabel::Legend(text) => ("kit-gain-row__name kit-legend", text),
    };
    tip_some(unit_tip, move || {
        let text = text.clone();
        AnyView::new(view! { text(class = class) {{text}} })
    })
}

fn stage_row(row: StageRow) -> AnyView {
    let StageRow {
        label,
        lane,
        stage,
        value,
        readout,
        auto,
        peak,
        tone,
        held_by,
        trailing,
        commit,
    } = row;
    let unit = stage.unit;
    let unit_tip = Signal::derive(move || stage_tip(unit, held_by.get().as_deref()));
    let disabled = Signal::derive(move || held_by.with(Option::is_some));
    let control = format!(
        "{}{} gain",
        lane.map(|lane| format!("{lane} ")).unwrap_or_default(),
        gain_label(&stage)
    );
    let (pending, change) = debounced({
        let commit = commit.clone();
        move |db| commit(db)
    });
    let shown = Signal::derive(move || pending.get().unwrap_or_else(|| value.get()));
    let symbol = gain_unit(unit);
    let read = move || match readout.get() {
        Some(said) => said,
        None if symbol.is_empty() => format_gain(unit, shown.get()),
        None => format!("{} {symbol}", format_gain(unit, shown.get())),
    };
    let body = if stage.is_switch() {
        switch_body(&stage, shown, disabled, commit)
    } else {
        let spec = GainMeter {
            label: control,
            value: shown,
            min: 0.0,
            max: 0.0,
            auto,
            disabled,
            peak: peak.unwrap_or_else(|| Signal::stored(None)),
            tone,
        };
        stepped_body(&stage, spec, change)
    };
    let switch = stage.is_switch();
    let (min, max) = (stage.range.min, stage.range.max);
    let switch_read = move || {
        if shown.get() > min {
            format!("+{max:.0} dB")
        } else {
            "0 dB".to_owned()
        }
    };
    AnyView::new(view! {
        row(class = "kit-gain-row") {
            {row_name(label, unit_tip)}
            box(class = "kit-gain-row__meter") {{body}}
            text(class = "kit-gain-row__read", class:quiet = move || readout.with(Option::is_some)) {{move || if switch { switch_read() } else { read() }}}
            box(class = "kit-gain-row__tail") {{trailing}}
        }
    })
}

fn switch_body(
    stage: &GainStage,
    shown: Signal<f64>,
    disabled: Signal<bool>,
    commit: Rc<dyn Fn(f64)>,
) -> AnyView {
    let (min, max) = (stage.range.min, stage.range.max);
    let on = Signal::derive(move || shown.get() > min);
    AnyView::new(view! {
        row(class = "kit-slot", class:kit-muted = disabled) {
            {check(on, move |next| {
                if !disabled.get_untracked() {
                    commit(if next { max } else { min });
                }
            })}
        }
    })
}

fn stepped_body(
    stage: &GainStage,
    spec: GainMeter,
    change: impl Fn(f64) + Clone + 'static,
) -> AnyView {
    let settings = stage_settings(stage);
    if settings.is_empty() {
        let snap = stage.clone();
        let spec = GainMeter {
            min: stage.range.min,
            max: stage.range.max,
            ..spec
        };
        return AnyView::new(gain_meter(spec, move |db| change(snap_to_stage(&snap, db))));
    }
    let top = settings.len().saturating_sub(1) as f64;
    let table = settings.clone();
    let shown = spec.value;
    let index = Signal::derive(move || setting_index(&table, shown.get()) as f64);
    let spec = GainMeter {
        value: index,
        min: 0.0,
        max: top,
        ..spec
    };
    AnyView::new(gain_meter(spec, move |at| {
        let at = at.round().clamp(0.0, top) as usize;
        if let Some(db) = settings.get(at) {
            change(*db);
        }
    }))
}
