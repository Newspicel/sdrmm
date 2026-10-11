use std::{collections::BTreeMap, rc::Rc};

use sdrmm_wire::{
    device::{DeviceSettings, Tuning},
    patch::{DeviceNode, NodeBody},
};
use zgui::prelude::*;

use super::{
    actions::edit_body,
    lanes::{auto_tuning, lanes_merged, lock_stream, tune_delta, tuner_dials, tuning_delta},
    layout::{
        LaneHold, all_auto_tuning, all_locked, dial_hold, lane_holds, lock_all, tune_all_delta,
        tuning_all_delta,
    },
    radio::Radio,
};
use crate::{
    store::Store,
    ui::{
        kit_sources::{
            DialSpec,
            dial::{ANY_FREQUENCY, Reach, in_tuning_range, is_tunable, tuning_range},
            frequency_dial, icons, tip, titled_icon_button, tune_to, units,
        },
        shell::Shell,
    },
};

#[derive(Clone)]
pub struct Tuned {
    pub store: Store,
    pub node: String,
    pub radio: Radio,
    pub shell: Option<Shell>,
}

impl Tuned {
    pub fn new(store: Store, node: String, radio: Radio) -> Self {
        Self {
            store,
            node,
            radio,
            shell: Shell::current(),
        }
    }

    fn read_node<T>(&self, read: impl Fn(&DeviceNode) -> T + Send + Sync + 'static) -> Signal<T>
    where
        T: Default + Send + Sync + Clone + 'static,
    {
        let (store, node) = (self.store, self.node.clone());
        Signal::derive(move || {
            store.graph.with(|graph| {
                graph
                    .node(&node)
                    .and_then(|found| match &found.body {
                        NodeBody::Device(device) => Some(read(device)),
                        _ => None,
                    })
                    .unwrap_or_default()
            })
        })
    }

    pub fn locked_streams(&self) -> Signal<Vec<u32>> {
        self.read_node(|device| device.locked_streams.clone())
    }

    pub fn split(&self) -> Signal<bool> {
        self.read_node(|device| device.split_tuning)
    }

    pub fn holds(&self) -> Signal<BTreeMap<u32, LaneHold>> {
        let (store, node) = (self.store, self.node.clone());
        Signal::derive(move || store.graph.with(|graph| lane_holds(graph, &node)))
    }

    fn edit(&self, change: impl FnOnce(&mut DeviceNode) + 'static) {
        edit_body(self.store, self.node.clone(), move |body| {
            if let NodeBody::Device(device) = body {
                change(device);
            }
        });
    }

    fn leave_auto(&self, auto: bool, proceed: impl FnOnce() + 'static) {
        match self.shell {
            Some(shell) => shell.leave_auto(auto, proceed),
            None => proceed(),
        }
    }

    fn send(&self, auto: bool, delta: Option<DeviceSettings>) {
        let radio = self.radio;
        if let Some(delta) = delta {
            self.leave_auto(auto, move || radio.patch(delta));
        }
    }

    pub fn tune_radio(&self, stream: u32, hz: f64) {
        let read = self.radio.read_untracked(|set| {
            (
                auto_tuning(set, stream),
                tune_delta(&set.capabilities, stream, hz),
            )
        });
        if let Some((auto, delta)) = read {
            self.send(auto, Some(delta));
        }
    }

    pub fn tune_all(&self, hz: f64) {
        let read = self
            .radio
            .read_untracked(|set| (all_auto_tuning(set), tune_all_delta(&set.capabilities, hz)));
        if let Some((auto, delta)) = read {
            self.send(auto, Some(delta));
        }
    }

    pub fn set_tuning(&self, stream: Option<u32>, tuning: Tuning) {
        let read = self.radio.read_untracked(|set| match stream {
            Some(stream) => (
                auto_tuning(set, stream),
                tuning_delta(&set.capabilities, stream, tuning),
            ),
            None => (
                all_auto_tuning(set),
                tuning_all_delta(&set.capabilities, tuning),
            ),
        });
        if let Some((auto, delta)) = read {
            self.send(tuning == Tuning::Manual && auto, Some(delta));
        }
    }
}

pub fn reach_of(radio: Radio) -> Signal<Reach> {
    Signal::derive(move || {
        radio
            .read(|set| tuning_range(&set.capabilities))
            .unwrap_or(ANY_FREQUENCY)
    })
}

fn reach_hint(reach: Signal<Reach>) -> Signal<String> {
    Signal::derive(move || {
        let reach = reach.get();
        format!(
            "Reaches {} to {}",
            units::mhz(reach.min),
            units::mhz(reach.max)
        )
    })
}

fn dial_hz(radio: Radio, stream: u32) -> Signal<f64> {
    Signal::derive(move || {
        radio
            .read(|set| {
                tuner_dials(set)
                    .into_iter()
                    .find(|dial| dial.stream == stream)
                    .map_or(0.0, |dial| dial.hz)
            })
            .unwrap_or(0.0)
    })
}

pub fn held_badge(store: Store, hold: LaneHold) -> AnyView {
    let LaneHold { array, label } = hold;
    tip(
        move || format!("Tuned by {label}"),
        AnyView::new(view! {
            control(
                class = "kit-btn kit-btn--sm kit-held",
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                on:click:stop = move |_| store.selected.set(Some(array.clone()))
            ) {
                {icons::icon(icons::LINK)}
                text {"Array"}
            }
        }),
    )
}

fn auto_tuning_button(tuned: &Tuned, stream: Option<u32>) -> AnyView {
    let radio = tuned.radio;
    let auto = Signal::derive(move || {
        radio
            .read(|set| match stream {
                Some(stream) => auto_tuning(set, stream),
                None => all_auto_tuning(set),
            })
            .unwrap_or(true)
    });
    let tuned = tuned.clone();
    titled_icon_button(
        icons::RADAR,
        "Follow the decoders",
        move || {
            if auto.get() {
                "Auto mode: following decoders"
            } else {
                "Auto mode: follow decoders"
            }
            .to_owned()
        },
        auto,
        Signal::stored(false),
        move || {
            let next = if auto.get_untracked() {
                Tuning::Manual
            } else {
                Tuning::Auto
            };
            tuned.set_tuning(stream, next);
        },
    )
}

fn link_button(tuned: &Tuned) -> AnyView {
    let split = tuned.split();
    let linked = Signal::derive(move || !split.get());
    let tuned = tuned.clone();
    titled_icon_button(
        icons::LINK,
        "Tune all lanes together",
        move || {
            if linked.get() {
                "Lanes tune together. Click to tune each lane"
            } else {
                "Tune all lanes together"
            }
            .to_owned()
        },
        linked,
        Signal::stored(false),
        move || {
            let link = !linked.get_untracked();
            tuned.edit(move |device| device.split_tuning = !link);
            if link {
                let hz = tuned
                    .radio
                    .read_untracked(|set| tuner_dials(set).first().map(|dial| dial.hz))
                    .flatten();
                if let Some(hz) = hz {
                    tuned.tune_all(hz);
                }
            }
        },
    )
}

fn lock_button(locked: Signal<bool>, on_lock: Rc<dyn Fn(bool)>) -> AnyView {
    AnyView::new(move || {
        let on_lock = on_lock.clone();
        let (svg, label, title) = if locked.get() {
            (icons::LOCK, "Unlock tuning", "Tuning locked")
        } else {
            (icons::LOCK_OPEN, "Lock tuning", "Lock tuning")
        };
        titled_icon_button(
            svg,
            label,
            move || title.to_owned(),
            locked,
            Signal::stored(false),
            move || on_lock(!locked.get_untracked()),
        )
    })
}

#[derive(Clone, PartialEq)]
struct Shape {
    merged: bool,
    split: bool,
    tunable: bool,
}

pub fn tuner(tuned: Tuned) -> impl IntoView {
    let radio = tuned.radio;
    let split = tuned.split();
    let shape = Memo::new(move |_| {
        radio.read(|set| Shape {
            merged: lanes_merged(set),
            split: split.get(),
            tunable: is_tunable(tuning_range(&set.capabilities)),
        })
    });
    move || {
        let shape = shape.get()?;
        let tools = tools(&tuned, &shape);
        Some(if shape.merged && shape.split {
            AnyView::new(view! {
                row(class = "kit-dial-row") {
                    text(class = "kit-legend") {"Per lane"}
                    spacer() {}
                    {tools}
                }
            })
        } else {
            AnyView::new(dial_row(&tuned, shape.merged, tools))
        })
    }
}

fn tools(tuned: &Tuned, shape: &Shape) -> AnyView {
    let store = tuned.store;
    let radio = tuned.radio;
    let holds = tuned.holds();
    let hold = Memo::new(move |_| holds.with(|holds| dial_hold(holds, 0, false)));
    let merged = shape.merged;
    let split = shape.split;
    let tunable = shape.tunable;
    let streams = tuned.locked_streams();
    let locked = Signal::derive(move || {
        let streams = streams.get();
        radio
            .read(|set| {
                if merged {
                    all_locked(&streams, &set.capabilities)
                } else {
                    streams.contains(&0)
                }
            })
            .unwrap_or(false)
    });
    let tuned = tuned.clone();
    AnyView::new(move || {
        if let Some(hold) = hold.get() {
            return Some(held_badge(store, hold));
        }
        if !tunable {
            return None;
        }
        let link = merged.then(|| link_button(&tuned));
        let auto = auto_tuning_button(&tuned, if merged { None } else { Some(0) });
        let entry = (!split || !merged).then(|| dial_entry(&tuned, locked));
        let lock_tuned = tuned.clone();
        let lock = lock_button(
            locked,
            Rc::new(move |held| {
                let current = lock_tuned.locked_streams().get_untracked();
                let next = lock_tuned
                    .radio
                    .read_untracked(|set| {
                        if merged {
                            lock_all(&set.capabilities, held)
                        } else {
                            lock_stream(&current, 0, held)
                        }
                    })
                    .unwrap_or_default();
                lock_tuned.edit(move |device| device.locked_streams = next);
            }),
        );
        Some(AnyView::new(view! {
            row(class = "kit-tools") { {entry} {link} {auto} {lock} }
        }))
    })
}

fn dial_entry(tuned: &Tuned, locked: Signal<bool>) -> AnyView {
    let radio = tuned.radio;
    let reach = reach_of(radio);
    let hz = dial_hz(radio, 0);
    let merged = radio.read_untracked(lanes_merged).unwrap_or(false);
    let tuned = tuned.clone();
    AnyView::new(tune_to(
        "Type a frequency",
        hz,
        reach_hint(reach),
        move |entered| in_tuning_range(entered, reach.get_untracked()),
        locked,
        move |hz| {
            if merged {
                tuned.tune_all(hz);
            } else {
                tuned.tune_radio(0, hz);
            }
        },
    ))
}

fn dial_row(tuned: &Tuned, merged: bool, tools: AnyView) -> impl IntoView {
    let radio = tuned.radio;
    let store = tuned.store;
    let node = tuned.node.clone();
    let dial = NodeRef::new();
    crate::ui::hotkeys::focus_dial_of(node.clone(), dial);
    let reach = reach_of(radio);
    let holds = tuned.holds();
    let streams = tuned.locked_streams();
    let held = Signal::derive(move || {
        let locked = radio
            .read(|set| {
                let streams = streams.get();
                if merged {
                    all_locked(&streams, &set.capabilities)
                } else {
                    streams.contains(&0)
                }
            })
            .unwrap_or(false);
        !is_tunable(reach.get()) || locked || holds.with(|holds| !holds.is_empty())
    });
    let spec = DialSpec {
        hz: dial_hz(radio, 0),
        reach,
        disabled: held,
        wheel: Signal::derive(move || store.selected.get().as_deref() == Some(node.as_str())),
    };
    let tuned = tuned.clone();
    view! {
        row(class = "kit-dial-row") {
            box(node_ref = dial) {
                {frequency_dial(spec, move |hz| if merged { tuned.tune_all(hz) } else { tuned.tune_radio(0, hz) })}
            }
            spacer() {}
            {tools}
        }
    }
}

pub fn lane_dial(tuned: &Tuned, stream: u32) -> AnyView {
    let radio = tuned.radio;
    let store = tuned.store;
    let node = tuned.node.clone();
    let port = radio
        .read_untracked(|set| {
            tuner_dials(set)
                .into_iter()
                .find(|dial| dial.stream == stream)
                .and_then(|dial| dial.port)
        })
        .flatten()
        .unwrap_or_default();
    let reach = reach_of(radio);
    let hz = dial_hz(radio, stream);
    let holds = tuned.holds();
    let hold = Memo::new(move |_| holds.with(|holds| dial_hold(holds, stream, true)));
    let streams = tuned.locked_streams();
    let held = Signal::derive(move || {
        !is_tunable(reach.get())
            || streams.with(|streams| streams.contains(&stream))
            || hold.with(Option::is_some)
    });
    let spec = DialSpec {
        hz,
        reach,
        disabled: held,
        wheel: Signal::derive(move || store.selected.get().as_deref() == Some(node.as_str())),
    };
    let title = format!("Type a frequency for {port}");
    let entry_tuned = tuned.clone();
    let side = move || match hold.get() {
        Some(hold) => held_badge(store, hold),
        None => {
            let tuned = entry_tuned.clone();
            AnyView::new(tune_to(
                title.clone(),
                hz,
                reach_hint(reach),
                move |entered| in_tuning_range(entered, reach.get_untracked()),
                held,
                move |hz| tuned.tune_radio(stream, hz),
            ))
        }
    };
    let tuned = tuned.clone();
    AnyView::new(view! {
        row(class = "kit-dial-row kit-lane-dial") {
            text(class = "kit-gain-row__name kit-gain-row__port") {{port}}
            {frequency_dial(spec, move |hz| tuned.tune_radio(stream, hz))}
            spacer() {}
            {side}
        }
    })
}
