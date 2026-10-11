use sdrmm_wire::{
    device::{Capabilities, DeviceSettings},
    state::DeviceSet,
};
use zgui::prelude::*;

use super::{
    gain::{LaneExtras, gain_lanes, lane_peaks},
    patch,
    settings::setting_chips,
};
use crate::store::Store;

pub const LOOP_SETTING: &str = "loop";

#[derive(Clone, Copy)]
pub struct Radio {
    pub store: Store,
    pub set: Signal<Option<DeviceSet>>,
}

impl Radio {
    pub fn patch(self, delta: DeviceSettings) {
        if let Some(id) = self
            .set
            .with_untracked(|set| set.as_ref().map(|set| set.id))
        {
            patch::apply(self.store, id, delta);
        }
    }

    pub fn read<T>(self, read: impl FnOnce(&DeviceSet) -> T) -> Option<T> {
        self.set.with(|set| set.as_ref().map(read))
    }

    pub fn read_untracked<T>(self, read: impl FnOnce(&DeviceSet) -> T) -> Option<T> {
        self.set.with_untracked(|set| set.as_ref().map(read))
    }

    pub fn settings<T: Send + Sync + Clone + 'static>(
        self,
        read: impl Fn(&DeviceSettings) -> T + Send + Sync + 'static,
        fallback: T,
    ) -> Signal<T> {
        Signal::derive(move || {
            self.read(|set| read(&set.settings))
                .unwrap_or_else(|| fallback.clone())
        })
    }
}

#[derive(Clone, PartialEq)]
struct Shape {
    caps: Capabilities,
    playing: bool,
    split: bool,
    inputs: Option<Vec<u32>>,
}

pub fn radio_settings(radio: Radio, extras: LaneExtras) -> impl IntoView {
    let split = extras.split;
    let shape = Memo::new(move |_| {
        radio.read(|set| Shape {
            caps: set.capabilities.clone(),
            playing: set.playback.is_some(),
            split: split.get(),
            inputs: set.settings.rx_inputs.clone(),
        })
    });
    let peaks = lane_peaks(radio.store, radio);
    move || {
        shape.get()?;
        let set = radio.read_untracked(Clone::clone)?;
        let lanes = gain_lanes(radio, &set, extras.clone(), peaks);
        Some(AnyView::new(view! {
            column(class = "kit-radio") {
                {setting_chips(radio, &set)}
                column(class = "kit-gain-lanes") {{lanes}}
            }
        }))
    }
}
