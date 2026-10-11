use sdrmm_wire::{device::DeviceSettings, patch::NodeBody, state::DeviceSetStatus};
use zgui::prelude::*;

use super::{
    device::{
        actions::{Busy, edit_body, release},
        gain::LaneExtras,
        radio::{Radio, radio_settings},
        tuner::reach_of,
    },
    set_signal,
};
use crate::{
    head::{self, NodeHead, Tone as HeadTone},
    store::Store,
    ui::kit_sources::{
        DialSpec, Tone, dial::in_tuning_range, footer, frequency_dial, health::source_health,
        install, titled_button, tune_to, units,
    },
};

pub const START_TIP: &str = "Generate a decodable signal to try a decoder without a radio";
pub const STOP_TIP: &str = "Stop generating and free the node: the wires stay drawn";

#[must_use]
pub fn failed_head(failed: bool) -> NodeHead {
    if failed {
        NodeHead::said("error").toned(HeadTone::Danger)
    } else {
        NodeHead::default()
    }
}

fn run(store: Store, node: String, running: bool) {
    edit_body(store, node, move |body| {
        if let NodeBody::SignalGen(generator) = body {
            generator.running = running;
        }
    });
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install();
    let set = set_signal(store, node.clone());
    let open = Memo::new(move |_| set.with(Option::is_some));
    let busy = Busy::new();
    move || {
        if open.get() {
            AnyView::new(running(store, node.clone(), Radio { store, set }, busy))
        } else {
            let node = node.clone();
            AnyView::new(view! {
                column(class = "face") {
                    {footer(titled_button(|| "Start".to_owned(), Tone::Primary, Signal::stored(false), || START_TIP.to_owned(), move || run(store, node.clone(), true)))}
                }
            })
        }
    }
}

fn running(store: Store, node: String, radio: Radio, busy: Busy) -> impl IntoView {
    let reach = reach_of(radio);
    let hz = Signal::derive(move || {
        radio
            .read(|set| set.settings.center_hz.unwrap_or(0.0))
            .unwrap_or(0.0)
    });
    let selected = {
        let node = node.clone();
        Signal::derive(move || store.selected.get().as_deref() == Some(node.as_str()))
    };
    let tune = move |center: f64| {
        radio.patch(DeviceSettings {
            center_hz: Some(center),
            ..DeviceSettings::default()
        })
    };
    let spec = DialSpec {
        hz,
        reach,
        disabled: Signal::stored(false),
        wheel: selected,
    };
    let hint = Signal::derive(move || {
        let reach = reach.get();
        format!(
            "Reaches {} to {}",
            units::mhz(reach.min),
            units::mhz(reach.max)
        )
    });
    let failed = Signal::derive(move || {
        radio
            .read(|set| set.status == DeviceSetStatus::Error)
            .unwrap_or(false)
    });
    head::show(store, node.clone(), move || failed_head(failed.get()));
    let error = Signal::derive(move || radio.read(|set| set.error.clone()).flatten());
    let stopping = busy.busy;
    let stop = move || {
        release(store, node.clone(), busy, |body| {
            if let NodeBody::SignalGen(generator) = body {
                generator.running = false;
            }
        });
    };
    view! {
        column(class = "face") {
            row(class = "kit-dial-row") {
                {frequency_dial(spec, tune)}
                spacer() {}
                {tune_to("Type the frequency to generate at", hz, hint, move |entered| in_tuning_range(entered, reach.get_untracked()), Signal::stored(false), tune)}
            }
            {radio_settings(radio, LaneExtras::none())}
            {move || error.get().map(|error| AnyView::new(view! { text(class = "kit-alert") {{error}} }))}
            {footer(view! {
                {source_health(store, radio.set, |_| Vec::new())}
                {titled_button(move || if stopping.get() { "Stopping…".to_owned() } else { "Stop".to_owned() }, Tone::Quiet, stopping.into(), || STOP_TIP.to_owned(), stop)}
            })}
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{device::SIGGEN_DRIVER_ID, patch::siggen_key};

    use super::*;

    #[test]
    fn a_failed_generator_says_so_in_its_head() {
        assert_eq!(failed_head(true).subtitle.as_deref(), Some("error"));
        assert_eq!(failed_head(true).tone, HeadTone::Danger);
        assert_eq!(failed_head(false), NodeHead::default());
    }

    fn device_id(node: &str) -> String {
        format!("{SIGGEN_DRIVER_ID}:{}", siggen_key(node))
    }

    #[test]
    fn a_node_name_is_made_safe_to_use_as_a_radio_key() {
        assert_eq!(siggen_key("signal_gen:a1b2"), "signal_gen-a1b2");
        assert_eq!(device_id("signal_gen:a1b2"), "siggen:signal_gen-a1b2");
        assert_eq!(siggen_key("plain"), "plain");
    }

    #[test]
    fn each_node_gets_its_own_generator() {
        assert_ne!(device_id("signal_gen:a"), device_id("signal_gen:b"));
    }
}
