mod denoise;

use sdrmm_wire::{
    AudioAgcMode, AudioProcessing, DenoiseMode, DenoiseModel, MAX_AUDIO_NOTCHES, MAX_AUDIO_TONE_HZ,
    MAX_CLICK_THRESHOLD, MAX_NOTCH_WIDTH_HZ, MIN_AUDIO_TONE_HZ, MIN_CLICK_THRESHOLD,
    MIN_NOTCH_WIDTH_HZ, NotchSettings, patch::NodeBody,
};
use zgui::prelude::*;

use self::denoise::{Models, model_missing, model_state, status_of};
use crate::{
    binding,
    head::{self, NodeHead},
    store::Store,
    ui::{
        faces::channel::settings::{
            NumberLimit, audio_chain_active, with_notch_added, with_notch_at, with_notch_removed,
        },
        kit_audio::{self, edit_body},
        kit_channel::{
            self, CLOSE_ICON, NumberSpec, button, format_hz, icon, number_field, setting_row,
            slider_field, tip, titled_segments,
        },
        widgets::{check, segments},
    },
};

const HZ_STEP: f64 = 10.0;

type Edit = Box<dyn FnOnce(&mut AudioProcessing)>;

const AGC_MODES: [(AudioAgcMode, &str); 4] = [
    (AudioAgcMode::Off, "Off"),
    (AudioAgcMode::Slow, "Slow"),
    (AudioAgcMode::Medium, "Med"),
    (AudioAgcMode::Fast, "Fast"),
];

const DENOISE_MODES: &[(DenoiseMode, &str, Option<&str>)] = &[
    (
        DenoiseMode::Spectral,
        "Spectral",
        Some("Light and fast, for steady hiss"),
    ),
    (
        DenoiseMode::Neural,
        "DPDFNet",
        Some("Best speech model, downloaded once"),
    ),
];

const DENOISE_MODELS: &[(DenoiseModel, &str, Option<&str>)] = &[
    (DenoiseModel::Dpdfnet2, "DPDFNet 2", Some("Light")),
    (DenoiseModel::Dpdfnet8, "DPDFNet 8", Some("Best, more CPU")),
];

#[must_use]
pub fn declick_summary(enabled: bool, threshold: f32) -> String {
    if enabled {
        format!("{threshold:.1}\u{d7}")
    } else {
        String::from("off")
    }
}

#[must_use]
pub fn denoise_summary(enabled: bool, missing: bool, strength: f32) -> String {
    match (enabled, missing) {
        (false, _) => String::from("off"),
        (true, true) => String::from("no model"),
        (true, false) => format!("{:.0}%", strength * 100.0),
    }
}

#[must_use]
pub fn passband_summary(enabled: bool, low_hz: f64, high_hz: f64) -> String {
    if enabled {
        format!("{} \u{2013} {}", format_hz(low_hz), format_hz(high_hz))
    } else {
        String::from("off")
    }
}

#[derive(Clone, Copy)]
struct Chain {
    store: Store,
    node: StoredValue<String>,
    settings: Signal<AudioProcessing>,
}

impl Chain {
    fn edit(self, change: impl FnOnce(&mut AudioProcessing) + 'static) {
        let change: Edit = Box::new(change);
        let node = self.node.get_value();
        edit_body(self.store, &node, move |body| {
            if let NodeBody::AudioFx(fx) = body {
                change(&mut fx.settings);
            }
        });
    }

    fn read<T: Send + Sync + 'static>(
        self,
        get: impl Fn(&AudioProcessing) -> T + Send + Sync + 'static,
    ) -> Signal<T> {
        let settings = self.settings;
        Signal::derive(move || settings.with(&get))
    }
}

fn summary(text: impl Fn() -> String + 'static, danger: impl Fn() -> bool + 'static) -> AnyView {
    AnyView::new(view! {
        text(class = "kc-legend", class:kc-danger = danger) {{text}}
    })
}

pub fn face(store: Store, node: String) -> impl IntoView {
    kit_audio::install();
    kit_channel::install();
    let settings = {
        let node = node.clone();
        Signal::derive(move || {
            store
                .graph
                .get()
                .node(&node)
                .and_then(|found| match &found.body {
                    NodeBody::AudioFx(fx) => Some(fx.settings.clone()),
                    _ => None,
                })
                .unwrap_or_default()
        })
    };
    head::show(store, node.clone(), move || {
        if audio_chain_active(Some(&settings.get())) {
            NodeHead::said("on")
        } else {
            NodeHead::default()
        }
    });
    let chain = Chain {
        store,
        node: StoredValue::new(node.clone()),
        settings,
    };
    let wired = {
        let node = node.clone();
        Signal::derive(move || !binding::sources_of(&store.graph.get(), &node, "audio").is_empty())
    };
    view! {
        column(class = "face kc-grid") {
            if move || !wired.get() {
                text(class = "hint") {"Wire channel audio in"}
            }
            {agc(chain)}
            {declick(chain)}
            {denoise(chain)}
            {auto_notch(chain)}
            {passband(chain)}
            {notches(chain)}
        }
    }
}

fn agc(chain: Chain) -> AnyView {
    setting_row(
        "AGC",
        Some("Audio AGC speed"),
        segments(
            AGC_MODES.to_vec(),
            chain.read(|audio| audio.agc),
            move |agc| {
                chain.edit(move |settings| settings.agc = agc);
            },
        ),
    )
}

fn declick(chain: Chain) -> AnyView {
    let on = chain.read(|audio| audio.click_removal.enabled);
    let threshold = chain.read(|audio| f64::from(audio.click_removal.threshold));
    let shown = move || {
        let audio = chain.settings.get();
        declick_summary(audio.click_removal.enabled, audio.click_removal.threshold)
    };
    setting_row(
        "De-click",
        Some("Click removal"),
        view! {
            {check(on, move |enabled: bool| chain.edit(move |settings| settings.click_removal.enabled = enabled))}
            {slider_field(
                threshold,
                (f64::from(MIN_CLICK_THRESHOLD), f64::from(MAX_CLICK_THRESHOLD), 0.5),
                Signal::derive(move || !on.get()),
                |value| format!("{value:.1}\u{d7}"),
                move |value| chain.edit(move |settings| settings.click_removal.threshold = value as f32),
            )}
            {summary(shown, || false)}
        },
    )
}

fn denoise(chain: Chain) -> AnyView {
    let held = chain.read(|audio| audio.denoise);
    let on = Signal::derive(move || held.get().enabled);
    let neural = Signal::derive(move || held.get().mode == DenoiseMode::Neural);
    let models = Models::watch(
        chain.store,
        Signal::derive(move || on.get() && neural.get()),
    );
    let status = Signal::derive(move || {
        models
            .held
            .with(|listed| status_of(listed.as_ref(), held.get().model))
    });
    let missing = Signal::derive(move || model_missing(&held.get(), status.get().as_ref()));
    let shown = move || {
        let denoise = held.get();
        denoise_summary(denoise.enabled, missing.get(), denoise.strength)
    };
    let model_row = move || {
        neural.get().then(|| {
            let state = move || status.get().map(|status| model_state(models, status));
            setting_row(
                "Model",
                Some("DPDFNet model"),
                view! {
                    {titled_segments(DENOISE_MODELS, Signal::derive(move || held.get().model), move |model| {
                        chain.edit(move |settings| settings.denoise.model = model);
                    })}
                    {state}
                },
            )
        })
    };
    AnyView::new(view! {
        column(class = "kc-grid") {
            {setting_row("Denoise", Some("Noise reduction"), view! {
                {check(on, move |enabled: bool| chain.edit(move |settings| settings.denoise.enabled = enabled))}
                {titled_segments(DENOISE_MODES, Signal::derive(move || held.get().mode), move |mode| {
                    chain.edit(move |settings| settings.denoise.mode = mode);
                })}
                {summary(shown, move || missing.get())}
            })}
            {model_row}
            {setting_row("Strength", Some("Noise reduction strength"), slider_field(
                Signal::derive(move || f64::from(held.get().strength)),
                (0.0, 1.0, 0.05),
                Signal::derive(move || !on.get()),
                |value| format!("{:.0}%", value * 100.0),
                move |value| chain.edit(move |settings| settings.denoise.strength = value as f32),
            ))}
        }
    })
}

fn auto_notch(chain: Chain) -> AnyView {
    setting_row(
        "Auto notch",
        Some("Finds and removes steady carriers"),
        check(
            chain.read(|audio| audio.auto_notch),
            move |enabled: bool| {
                chain.edit(move |settings| settings.auto_notch = enabled);
            },
        ),
    )
}

fn hz_field(
    value: Signal<f64>,
    label: &'static str,
    unit: &'static str,
    range: (f64, f64),
    commit: impl Fn(f64) + Clone + 'static,
) -> impl IntoView {
    let spec = NumberSpec::new(label)
        .limit(NumberLimit::new(range.0, range.1, HZ_STEP))
        .unit(unit)
        .size("kc-num kc-num--narrow");
    number_field(
        Signal::derive(move || Some(value.get())),
        spec,
        move |next| {
            if let Some(next) = next {
                commit(next);
            }
        },
    )
}

fn passband(chain: Chain) -> AnyView {
    let on = chain.read(|audio| audio.filter.enabled);
    let low = chain.read(|audio| audio.filter.low_hz);
    let high = chain.read(|audio| audio.filter.high_hz);
    let range = (MIN_AUDIO_TONE_HZ, MAX_AUDIO_TONE_HZ);
    let invalid = move || on.get() && low.get() >= high.get();
    let shown = move || passband_summary(on.get(), low.get(), high.get());
    AnyView::new(view! {
        column(class = "kc-grid") {
            {setting_row("Passband", Some("Audio filter"), view! {
                {check(on, move |enabled: bool| chain.edit(move |settings| settings.filter.enabled = enabled))}
                {hz_field(low, "Audio filter low cut", "Hz", range, move |hz| chain.edit(move |settings| settings.filter.low_hz = hz))}
                {hz_field(high, "Audio filter high cut", "Hz", range, move |hz| chain.edit(move |settings| settings.filter.high_hz = hz))}
                {summary(shown, invalid)}
            })}
            {kit_audio::alert(move || invalid().then(|| String::from("Low cut must sit below high cut")))}
        }
    })
}

fn notches(chain: Chain) -> AnyView {
    let count = Memo::new(move |_| chain.settings.with(|audio| audio.notches.len()));
    let rows = move || {
        (0..count.get())
            .map(|index| notch_row(chain, index))
            .collect::<Vec<_>>()
    };
    let add = move || {
        chain.edit(|settings| {
            if let Some(next) = with_notch_added(&settings.notches) {
                settings.notches = next;
            }
        });
    };
    AnyView::new(view! {
        column(class = "kc-grid") {
            {rows}
            row(class = "kc-row") {
                {tip(
                    "Add a notch, up to 4",
                    AnyView::new(button("+ Notch", "kc-btn kc-btn--sm", Signal::derive(move || count.get() >= MAX_AUDIO_NOTCHES), add)),
                )}
            }
        }
    })
}

fn notch_row(chain: Chain, index: usize) -> AnyView {
    let notch = chain.read(move |audio| audio.notches.get(index).copied().unwrap_or_default());
    let freq = Signal::derive(move || notch.get().freq_hz);
    let width = Signal::derive(move || notch.get().width_hz);
    let edit = move |change: fn(&mut NotchSettings, f64), hz: f64| {
        chain.edit(move |settings| {
            settings.notches = with_notch_at(&settings.notches, index, |found| change(found, hz));
        });
    };
    let remove = move || {
        chain.edit(move |settings| {
            settings.notches = with_notch_removed(&settings.notches, index);
        });
    };
    let name = format!("Notch {}", index + 1);
    let remove_label = format!("Remove notch {}", index + 1);
    setting_row(
        name,
        None,
        view! {
            {hz_field(freq, "Notch frequency", "Hz", (MIN_AUDIO_TONE_HZ, MAX_AUDIO_TONE_HZ), move |hz| edit(|found, hz| found.freq_hz = hz, hz))}
            {hz_field(width, "Notch width", "wide", (MIN_NOTCH_WIDTH_HZ, MAX_NOTCH_WIDTH_HZ), move |hz| edit(|found, hz| found.width_hz = hz, hz))}
            control(
                class = "kc-icon kc-btn--danger",
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                a11y:label = remove_label,
                on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                on:click:stop = move |_| remove()
            ) {
                {icon(CLOSE_ICON, "kc-glyph kc-glyph--sm")}
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_read_off_until_a_stage_is_on() {
        assert_eq!(declick_summary(false, 6.0), "off");
        assert_eq!(declick_summary(true, 3.0), "3.0\u{d7}");
        assert_eq!(denoise_summary(false, true, 0.5), "off");
        assert_eq!(denoise_summary(true, true, 0.5), "no model");
        assert_eq!(denoise_summary(true, false, 0.55), "55%");
        assert_eq!(passband_summary(false, 300.0, 3_000.0), "off");
        assert_eq!(
            passband_summary(true, 300.0, 3_000.0),
            "300 Hz \u{2013} 3 kHz"
        );
    }

    #[test]
    fn every_mode_and_model_has_a_tooltip() {
        assert!(DENOISE_MODES.iter().all(|(_, _, title)| title.is_some()));
        assert_eq!(DENOISE_MODES[1].1, "DPDFNet");
        assert_eq!(DENOISE_MODELS.len(), DenoiseModel::ALL.len());
    }
}
