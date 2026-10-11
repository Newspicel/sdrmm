pub mod actions;
pub mod caps;
pub mod devices;
pub mod extras;
pub mod gain;
pub mod lanes;
pub mod layout;
pub mod open;
pub mod patch;
pub mod radio;
pub mod saved;
pub mod settings;
pub mod tuner;

use std::rc::Rc;

use sdrmm_wire::{
    device::DeviceInfo,
    patch::{DeviceRef, NodeBody},
    state::DeviceSet,
};
use zgui::prelude::*;

use self::{
    actions::{Busy, create_set, edit_body, opened_device, release},
    gain::LaneExtras,
    lanes::{
        Hearing, Tone as HeardTone, bond_said, clipping_said, fault_said, hearing, lanes_merged,
        ref_label, refusal_said,
    },
    layout::loss_said,
    open::{Choices, device_choices},
    radio::{Radio, radio_settings},
    tuner::{Tuned, lane_dial, tuner},
};
use super::{array::make_array_button, set_signal};
use crate::{
    head::{self, NodeHead, Tone as HeadTone},
    store::Store,
    ui::{
        kit_audio::{Stat, Tone as StatTone},
        kit_sources::{Tone, collapsible, footer, health::source_health, install, titled_button},
    },
};

pub const LOSS_HINT: &str =
    "The radio sends more than its link or this computer carries. Lower the rate or the lanes";
pub const CLIPPING_HINT: &str = "The ADC is at full scale. Lower the gain";
pub const BOND_HINT: &str = "The lanes sample on one clock, so their streams line up in time";
pub const INSIDE: &str = "Decoders wired to this radio that sit inside its window";

#[derive(Clone, PartialEq)]
enum Phase {
    Unnamed,
    Named(DeviceRef),
    Open,
}

fn reference_of(store: Store, node: &str) -> Option<DeviceRef> {
    store.graph.with(|graph| {
        graph.node(node).and_then(|found| match &found.body {
            NodeBody::Device(device) => device.device.clone(),
            _ => None,
        })
    })
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install();
    let set = set_signal(store, node.clone());
    let phase = {
        let node = node.clone();
        Memo::new(
            move |_| match (reference_of(store, &node), set.with(Option::is_some)) {
                (None, _) => Phase::Unnamed,
                (Some(_), true) => Phase::Open,
                (Some(reference), false) => Phase::Named(reference),
            },
        )
    };
    let busy = Busy::new();
    move || match phase.get() {
        Phase::Unnamed => AnyView::new(choose(store, node.clone(), busy)),
        Phase::Named(reference) => AnyView::new(named(store, node.clone(), reference, busy)),
        Phase::Open => AnyView::new(opened(store, node.clone(), set, busy)),
    }
}

fn choose(store: Store, node: String, busy: Busy) -> impl IntoView {
    let bind = {
        let node = node.clone();
        move |info: DeviceInfo| {
            let chosen = DeviceRef::from_info(&info);
            let already = store.state.with_untracked(|state| {
                state
                    .device_sets
                    .iter()
                    .any(|set| chosen.matches(&set.device))
            });
            let node = node.clone();
            busy.run(store, async move {
                if !already {
                    create_set(store, devices::device_id(&info)).await?;
                }
                name_radio(store, node, Some(chosen));
                Ok(())
            });
        }
    };
    let network = {
        let node = node.clone();
        move |id: String| {
            let node = node.clone();
            busy.run(store, async move {
                let set = create_set(store, id).await?;
                match opened_device(store, set).await? {
                    Some(info) => name_radio(store, node, Some(DeviceRef::from_info(&info))),
                    None => store.apply().await,
                }
                Ok(())
            });
        }
    };
    let choices = Choices {
        on_choose: Box::new(bind),
        on_network: Box::new(network),
    };
    view! {
        column(class = "face") {
            {device_choices(store, node, busy, choices)}
        }
    }
}

fn name_radio(store: Store, node: String, chosen: Option<DeviceRef>) {
    edit_body(store, node, move |body| {
        if let NodeBody::Device(device) = body {
            device.device = chosen;
        }
    });
}

fn forget(store: Store, node: String, busy: Busy) {
    release(store, node, busy, |body| {
        if let NodeBody::Device(device) = body {
            device.device = None;
        }
    });
}

#[must_use]
pub fn open_title(on_bus: bool) -> &'static str {
    if on_bus {
        "Open this radio and start the channels wired to it"
    } else {
        "Nothing to open until the radio is plugged back in"
    }
}

fn named(store: Store, node: String, reference: DeviceRef, busy: Busy) -> impl IntoView {
    let probe = reference.clone();
    let on_bus = Signal::derive(move || {
        store
            .devices
            .with(|devices| devices.iter().any(|info| probe.matches(info)))
    });
    head::show(store, node.clone(), move || {
        NodeHead::said(if on_bus.get() {
            "not open"
        } else {
            "disconnected"
        })
    });
    let label = ref_label(&reference);
    view! {
        column(class = "face") {
            text(class = "kit-mono") {{label}}
            {footer(view! {
                {titled_button(|| "Forget radio".to_owned(), Tone::Quiet, busy.busy.into(), || "Free this node so you can pick a different radio".to_owned(), move || forget(store, node.clone(), busy))}
                {titled_button(|| "Open radio".to_owned(), Tone::Primary, Signal::derive(move || !on_bus.get()), move || open_title(on_bus.get()).to_owned(), move || {
                    zgui::task::spawn_local(async move { store.apply().await });
                })}
            })}
        }
    }
}

#[must_use]
pub fn heard_head(heard: Hearing) -> NodeHead {
    let tone = match heard.tone {
        HeardTone::Ok => HeadTone::Plain,
        HeardTone::Warn => HeadTone::Warn,
        HeardTone::Danger => HeadTone::Danger,
    };
    NodeHead::said(format!("{}/{}", heard.heard, heard.total)).toned(tone)
}

#[must_use]
pub fn heard_tip(heard: Hearing) -> String {
    if heard.tone == HeardTone::Ok {
        INSIDE.to_owned()
    } else {
        format!("{INSIDE}. Raise the sample rate, or move the ones it misses to another radio.")
    }
}

fn lane_extras(tuned: &Tuned) -> LaneExtras {
    let radio = tuned.radio;
    let holds = tuned.holds();
    let split = tuned.split();
    let lead_tuned = tuned.clone();
    LaneExtras {
        advised: Signal::derive(move || holds.with(|holds| holds.keys().copied().collect())),
        held_by: Signal::derive(move || {
            holds.with(|holds| {
                holds
                    .iter()
                    .map(|(stream, hold)| (*stream, hold.label.clone()))
                    .collect()
            })
        }),
        split: Signal::derive(move || split.get() && radio.read(lanes_merged).unwrap_or(false)),
        lane_lead: Some(Rc::new(move |stream| lane_dial(&lead_tuned, stream))),
    }
}

fn opened(store: Store, node: String, set: Signal<Option<DeviceSet>>, busy: Busy) -> impl IntoView {
    let radio = Radio { store, set };
    head::show(store, node.clone(), move || {
        radio
            .read(hearing)
            .map_or_else(NodeHead::default, heard_head)
    });
    let tuned = Tuned::new(store, node.clone(), radio);
    let extras = lane_extras(&tuned);
    let closing = busy.busy;
    let make_array = make_array_button(store, node.clone(), set);
    let label = Signal::derive(move || {
        radio
            .read(|set| set.device.label.clone())
            .unwrap_or_default()
    });
    let heard = Signal::derive(move || radio.read(hearing).map(heard_tip).unwrap_or_default());
    view! {
        column(class = "face") {
            {crate::ui::kit_sources::tip(move || heard.get(), AnyView::new(view! { text(class = "kit-head__title") {{move || label.get()}} }))}
            {tuner(tuned)}
            {radio_settings(radio, extras)}
            {fault(radio)}
            {footer(view! {
                {device_health(radio)}
                {make_array}
                {titled_button(move || if closing.get() { "Closing…".to_owned() } else { "Forget radio".to_owned() }, Tone::Quiet, closing.into(), || "Close this radio and free the node: the device is released and the wires stay drawn".to_owned(), move || forget(store, node.clone(), busy))}
            })}
        }
    }
}

#[must_use]
pub fn device_stats(set: &DeviceSet) -> Vec<Stat> {
    let mut shown = Vec::new();
    if let Some(loss) = loss_said(set) {
        shown.push(Stat::new("Lost", loss, LOSS_HINT).tone(StatTone::Warn));
    }
    if let Some(clipping) = clipping_said(set) {
        shown.push(Stat::new("Clipping", clipping, CLIPPING_HINT).tone(StatTone::Danger));
    }
    if let Some(bond) = lanes_merged(set)
        .then(|| bond_said(set.capabilities.coherence))
        .flatten()
    {
        shown.push(Stat {
            label: bond.to_owned(),
            value: None,
            title: BOND_HINT.to_owned(),
            tone: StatTone::Plain,
        });
    }
    shown
}

fn device_health(radio: Radio) -> impl IntoView {
    source_health(radio.store, radio.set, device_stats)
}

fn fault(radio: Radio) -> impl IntoView {
    let said = Memo::new(move |_| {
        radio.read(|set| {
            let error = set.error.clone();
            let fault = error.as_ref().map(|error| (fault_said(set), error.clone()));
            let refused = refusal_said(set).map(|said| {
                (
                    said,
                    set.refused
                        .as_ref()
                        .map(|r| r.error.clone())
                        .unwrap_or_default(),
                )
            });
            (fault, refused)
        })
    });
    move || {
        let (fault, refused) = said.get()?;
        let alert = |(summary, detail): (String, String)| {
            AnyView::new(view! {
                box(class = "kit-alert-block") {
                    {collapsible(move || summary.clone(), "kit-fold__alert", move || AnyView::new(view! { text(class = "kit-meta") {{detail.clone()}} }))}
                }
            })
        };
        let fault = fault.map(|(said, error)| match said {
            Some(said) => alert((said, error)),
            None => AnyView::new(view! { text(class = "kit-alert kit-alert-block") {{format!("Device fault · {error}")}} }),
        });
        Some(AnyView::new(
            view! { column { {fault} {refused.map(alert)} } },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(heard: usize, total: usize, tone: HeardTone) -> Hearing {
        Hearing { heard, total, tone }
    }

    #[test]
    fn the_head_counts_heard_decoders_in_the_tone_of_their_health() {
        let all = heard_head(heard(2, 2, HeardTone::Ok));
        assert_eq!(all.subtitle.as_deref(), Some("2/2"));
        assert_eq!(all.tone, HeadTone::Plain);
        assert_eq!(
            heard_head(heard(1, 2, HeardTone::Warn)).tone,
            HeadTone::Warn
        );
        assert_eq!(
            heard_head(heard(0, 2, HeardTone::Danger)).tone,
            HeadTone::Danger
        );
        assert_eq!(heard_tip(heard(2, 2, HeardTone::Ok)), INSIDE);
        assert!(heard_tip(heard(1, 2, HeardTone::Warn)).ends_with("another radio."));
    }

    #[test]
    fn open_says_why_it_cannot() {
        assert!(open_title(false).contains("plugged back in"));
        assert!(open_title(true).starts_with("Open this radio"));
    }

    #[test]
    fn health_warns_on_loss_and_alarms_on_clipping() {
        let mut set: DeviceSet = serde_json::from_value(serde_json::json!({
            "id": 1,
            "device": { "driver": "rtlsdr", "key": "0", "label": "RTL-SDR" },
            "capabilities": { "freq_ranges": [], "sample_rates": [], "gains": [], "antennas": [], "bandwidths": [] },
            "settings": {}, "status": "running", "channels": []
        }))
        .expect("device set");
        assert!(device_stats(&set).is_empty());
        set.loss = Some(0.25);
        set.clipping = vec![0];
        let stats = device_stats(&set);
        assert_eq!(stats[0].value.as_deref(), Some("25%"));
        assert_eq!(stats[0].tone, StatTone::Warn);
        assert_eq!(stats[1].value.as_deref(), Some("yes"));
        assert_eq!(stats[1].tone, StatTone::Danger);
    }
}
