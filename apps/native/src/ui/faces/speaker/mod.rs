mod meter;

use std::{cell::RefCell, rc::Rc};

use sdrmm_wire::AudioRoute;
use zgui::prelude::*;

use self::meter::{gain_meter, peak_signal};
use crate::{
    audio::{
        Audio, Health, SAMPLE_RATE,
        spectrogram::{self, AudioSpectrogram},
        use_audio,
    },
    binding::{self, Input},
    store::Store,
    ui::{
        faces::scope::{
            gpu::{RowLevel, WaterfallFeed, waterfall_view},
            palette::Palette,
        },
        kit_audio::{self, Stat, Tone, titled},
        kit_sources::{self, icon_button, icons},
        widgets::check,
    },
};

const TICKS_HZ: [f32; 4] = [3_000.0, 6_000.0, 12_000.0, 18_000.0];

const SHEET: &str = css!(
    r#"
.gram { position: relative; width: 100%; height: 96px; border-radius: 3px; overflow: hidden; background-color: var(--bg); }
.gram__fall { width: 100%; height: 96px; }
.gram__tick { position: absolute; bottom: 1px; padding: 0 2px; font-size: 9px; color: var(--ink-dim); background-color: var(--bg); pointer-events: none; }
.mute { flex-direction: row; gap: 4px; align-items: center; font-size: 10px; color: var(--ink-faint); }
.sp-row { flex-direction: row; gap: 8px; align-items: center; min-width: 0; }
.sp-name { flex: 0 0 56px; min-width: 0; font-size: 10px; text-transform: uppercase; color: var(--ink-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sp-volume { flex: 0 0 36px; text-align: right; font-family: var(--mono); font-size: 11px; color: var(--ink); }
"#
);

const BUFFER_HINT: &str = "Audio waiting for playback";
const TRIMMED_HINT: &str = "Old audio discarded to stay live";
const DROPPED_HINT: &str = "Audio lost before playback: dropped at the radio, the encoder or the link, or decoded too late on this machine to be played.";
const STALLS_HINT: &str = "Audio arrived but playback ran dry before it could be played: this machine's scheduling or a clock the buffer could not track. The buffer holds more after each one.";

#[must_use]
pub fn speaker_stats(healths: &[Health], debug: bool) -> Vec<Stat> {
    let buffered: f32 = healths.iter().map(|health| health.buffered_ms).sum();
    let trimmed: f32 = healths.iter().map(|health| health.trimmed_ms).sum();
    let dropped: f32 = healths.iter().map(|health| health.lost_ms).sum();
    let underruns: u32 = healths.iter().map(|health| health.underruns).sum();
    let mut shown = Vec::new();
    if debug && buffered >= 1.0 {
        shown.push(Stat::new(
            "Buffer",
            format!("{buffered:.0} ms"),
            BUFFER_HINT,
        ));
    }
    if debug && trimmed >= 1.0 {
        shown.push(Stat::new(
            "Trimmed",
            format!("{trimmed:.0} ms"),
            TRIMMED_HINT,
        ));
    }
    if dropped >= 1.0 {
        shown.push(Stat::new("Dropped", format!("{dropped:.0} ms"), DROPPED_HINT).tone(Tone::Warn));
    }
    if underruns > 0 {
        shown.push(Stat::new("Stalls", underruns.to_string(), STALLS_HINT).tone(Tone::Warn));
    }
    shown
}

pub fn face(store: Store, node: String) -> impl IntoView {
    kit_audio::install();
    kit_sources::install();
    install_stylesheet("speaker", SHEET);
    install_stylesheet("speaker-meter", meter::SHEET);
    let inputs = Signal::derive(move || {
        let state = store.state.get();
        binding::inputs_of(
            &store.graph.get(),
            &node,
            "audio",
            &state.device_sets,
            &state.trunk_systems,
        )
    });
    let Some(audio) = use_audio() else {
        return AnyView::new(
            view! { column(class = "face") { text(class = "hint") {"No audio output"} } },
        );
    };
    let healths = move || {
        inputs
            .get()
            .iter()
            .map(|input| audio.entry(&input.route()).health)
            .collect::<Vec<_>>()
    };
    let shown = move || speaker_stats(&healths(), cfg!(debug_assertions));
    AnyView::new(view! {
        column(class = "face") {
            if move || inputs.get().is_empty() {
                text(class = "hint") {"Wire a channel's audio in"}
            }
            for input in move || inputs.get(), key = |input: &Input| input.route() {
                {lane(store, audio, input)}
            }
            if move || !shown().is_empty() {
                row(class = "face__foot rec-foot") {
                    {kit_audio::stats(shown)}
                }
            }
        }
    })
}

fn play_toggle(wanted: Signal<bool>, toggle: impl Fn() + Clone + 'static) -> impl IntoView {
    move || {
        let on = wanted.get();
        let (svg, label, hint) = if on {
            (icons::STOP, "Stop", "Stop listening")
        } else {
            (icons::PLAY, "Play", "Listen")
        };
        titled(
            hint,
            AnyView::new(icon_button(
                svg,
                label,
                wanted,
                Signal::stored(false),
                toggle.clone(),
            )),
        )
    }
}

fn lane(store: Store, audio: Audio, input: Input) -> impl IntoView {
    let route = input.route();
    let entry = {
        let route = route.clone();
        Signal::derive(move || audio.entry(&route))
    };
    let name = {
        let node = input.node.clone();
        let kind = input.channel.settings.params.type_id().to_owned();
        kit_audio::node_label(store, &node, &kind)
    };
    let wanted = Signal::derive(move || entry.get().wanted);
    let playing = Signal::derive(move || entry.get().live);
    let toggle = {
        let route = route.clone();
        move || {
            if audio.entry(&route).wanted {
                audio.stop(&route);
            } else {
                audio.play(&route);
            }
        }
    };
    let volume = Signal::derive(move || f64::from(entry.get().volume));
    let set_volume = {
        let route = route.clone();
        move |value: f64| audio.set_volume(&route, value as f32)
    };
    let muted = Signal::derive(move || entry.get().muted);
    let mute = {
        let route = route.clone();
        move |on: bool| audio.set_muted(&route, on)
    };
    let (peak, listen) = peak_signal(playing);
    audio.watch(route.clone(), listen);
    let title = name.clone();
    view! {
        column(class = "lane") {
            row(class = "sp-row") {
                {titled(title, AnyView::new(view! { text(class = "sp-name") {{name.clone()}} }))}
                {gain_meter(format!("{name} volume"), volume, peak, set_volume)}
                text(class = "sp-volume") {{move || format!("{:.0}%", volume.get() * 100.0)}}
                {play_toggle(wanted, toggle)}
                row(class = "mute") {
                    {check(muted, mute)}
                    text {"Mute"}
                }
            }
            {gram(audio, route)}
            {kit_audio::alert(move || entry.get().error)}
        }
    }
}

fn gram(audio: Audio, route: AudioRoute) -> impl IntoView {
    let feed = Rc::new(RefCell::new(WaterfallFeed::default()));
    feed.borrow_mut().set_palette(Palette::default());
    let failure = RwSignal::new(None::<String>);
    let sink = feed.clone();
    let analyser = Rc::new(RefCell::new(AudioSpectrogram::new(
        spectrogram::FFT_SIZE,
        spectrogram::HOP,
    )));
    audio.watch(route, move |pcm, channels| {
        analyser.borrow_mut().push(pcm, channels, |row| {
            sink.borrow_mut().push_row(row, RowLevel::PLAIN)
        });
    });
    let ticks: Vec<_> = TICKS_HZ
        .into_iter()
        .map(|hz| {
            let left = spectrogram::tick_fraction(hz, SAMPLE_RATE as f32) * 100.0;
            view! {
                text(class = "gram__tick", style:left = Some(format!("{left}%"))) {
                    {format!("{:.0}k", hz / 1_000.0)}
                }
            }
        })
        .collect();
    view! {
        box(class = "gram") {
            {waterfall_view("gram__fall", feed, failure)}
            {ticks}
        }
        {kit_audio::alert(move || failure.get())}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health(buffered_ms: f32, trimmed_ms: f32, lost_ms: f32, underruns: u32) -> Health {
        Health {
            buffered_ms,
            trimmed_ms,
            lost_ms,
            underruns,
        }
    }

    #[test]
    fn buffer_and_trim_show_only_in_a_debug_build() {
        let healths = [health(40.0, 12.0, 0.0, 0), health(20.0, 0.0, 0.0, 0)];
        assert!(speaker_stats(&healths, false).is_empty());
        let shown = speaker_stats(&healths, true);
        let labels: Vec<_> = shown.iter().map(|stat| stat.label.as_str()).collect();
        assert_eq!(labels, vec!["Buffer", "Trimmed"]);
        assert_eq!(shown[0].value.as_deref(), Some("60 ms"));
    }

    #[test]
    fn losses_warn_in_every_build() {
        let shown = speaker_stats(&[health(0.0, 0.0, 25.4, 2)], false);
        let labels: Vec<_> = shown
            .iter()
            .map(|stat| (stat.label.as_str(), stat.tone))
            .collect();
        assert_eq!(
            labels,
            vec![("Dropped", Tone::Warn), ("Stalls", Tone::Warn)]
        );
    }
}
