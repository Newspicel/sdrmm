use zgui::prelude::*;
use zgui::reactive::ArcRwSignal;

use super::style::{BasemapChoice, MapView, Preset, hex, set_map_view, with_custom};
use crate::ui::{kit_audio::titled, kit_maps::entry, widgets::check};

const SLIDERS_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 17H5"/><path d="M19 7h-9"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/></svg>"#;
const CUSTOM_FILL: &str = "repeating-linear-gradient(45deg, var(--line-strong) 0px, var(--line-strong) 3px, transparent 3px, transparent 6px)";
const CUSTOM_HINT: &str =
    "XYZ tile template ({z}/{x}/{y}) or MapLibre style URL. API keys go in the URL.";
const TRACKS_HINT: &str = "Path flown or sailed by each target";

pub const SHEET: &str = css!(
    r#"
.map__settings { position: absolute; left: 8px; bottom: 8px; z-index: 3; flex-direction: column; align-items: flex-start; gap: 4px; }
.map__gear {
    width: 24px;
    height: 24px;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--line);
    border-radius: 4px;
    background-color: color-mix(in oklab, var(--bg) 85%, transparent);
    color: var(--ink-dim);
    cursor: pointer;
}
.map__gear:hover, .map__gear.on { color: var(--accent); }
.map__icon { width: 14px; height: 14px; }
.map__panel {
    width: 230px;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background-color: var(--panel);
    cursor: default;
}
.map__section { flex-direction: column; gap: 4px; }
.map__section-head { align-items: center; gap: 6px; }
.map__section-name { font-family: var(--mono); font-size: 10px; text-transform: uppercase; color: var(--ink-faint); }
.map__swatches { flex-wrap: wrap; gap: 6px; }
.map__swatch { width: 66px; flex-direction: column; gap: 2px; padding: 3px; border: 1px solid var(--line); border-radius: 4px; cursor: pointer; }
.map__swatch.on { border-color: var(--accent); }
.map__fill { height: 14px; border-radius: 2px; }
.map__swatch-name { font-family: var(--mono); font-size: 10px; color: var(--ink-dim); }
.map__swatch.on .map__swatch-name { color: var(--accent); }
.map__panel .geo__input { width: 210px; }
"#
);

#[must_use]
pub fn changed(view: &MapView) -> bool {
    let fresh = MapView::default();
    view.basemap.preset != fresh.basemap.preset || view.tracks != fresh.tracks
}

#[must_use]
pub fn preset_fill(preset: Preset) -> String {
    if preset == Preset::Custom {
        return CUSTOM_FILL.to_owned();
    }
    let palette = preset.palette();
    format!(
        "linear-gradient(to right, {land} 0%, {land} 60%, {water} 60%, {water} 100%)",
        land = hex(palette.land),
        water = hex(palette.water)
    )
}

fn choose(view: &ArcRwSignal<MapView>, change: impl FnOnce(&mut MapView)) {
    let mut next = view.get_untracked();
    change(&mut next);
    set_map_view(next);
}

fn swatch(view: ArcRwSignal<MapView>, preset: Preset) -> impl IntoView {
    let shown = view.clone();
    view! {
        control(
            class = "map__swatch",
            class:on = move || shown.with(|view| view.basemap.preset == preset),
            a11y:role = Role::Button,
            a11y:label = preset.key(),
            on:click:stop = move |_| choose(&view, |next| next.basemap.preset = preset)
        ) {
            box(class = "map__fill", style:background-image = Some(preset_fill(preset)))
            text(class = "map__swatch-name") {{preset.key()}}
        }
    }
}

fn custom_field(view: ArcRwSignal<MapView>) -> impl IntoView {
    let shown = view.clone();
    let field = entry(
        "Custom map URL",
        Signal::derive(move || shown.with(|view| view.basemap.custom.clone())),
        Signal::stored(false),
        move |text| {
            let held: BasemapChoice = view.with_untracked(|view| view.basemap.clone());
            let next = with_custom(&held, text);
            if next != held {
                choose(&view, |view| view.basemap = next);
            }
        },
    );
    titled(CUSTOM_HINT, AnyView::new(field))
}

fn panel(view: ArcRwSignal<MapView>) -> impl IntoView {
    let swatches: Vec<AnyView> = Preset::STYLES
        .into_iter()
        .chain([Preset::Custom])
        .map(|preset| AnyView::new(swatch(view.clone(), preset)))
        .collect();
    let tracks_on = view.clone();
    let tracks = check(
        Signal::derive(move || tracks_on.with(|view| view.tracks)),
        {
            let view = view.clone();
            move |on| choose(&view, |next| next.tracks = on)
        },
    );
    let tracks_name = titled(
        TRACKS_HINT,
        AnyView::new(view! { text(class = "map__section-name") {"Tracks"} }),
    );
    view! {
        column(
            class = "map__panel",
            on:pointer_down:stop = |_| {},
            on:wheel:stop = |_| {}
        ) {
            column(class = "map__section") {
                text(class = "map__section-name") {"Style"}
                row(class = "map__swatches") {{swatches}}
            }
            column(class = "map__section") {
                {custom_field(view)}
            }
            row(class = "map__section-head") {
                {tracks_name}
                spacer() {}
                {tracks}
            }
        }
    }
}

pub fn settings() -> impl IntoView {
    install_stylesheet("map-settings", SHEET);
    let view = super::style::map_view();
    let open = RwSignal::new(false);
    let marked = view.clone();
    let body = view.clone();
    view! {
        column(class = "map__settings") {
            if move || open.get() {
                {panel(body.clone())}
            }
            control(
                class = "map__gear",
                class:on = move || marked.with(changed),
                a11y:role = Role::Button,
                a11y:label = "Map settings",
                on:pointer_down:stop = |_| {},
                on:click:stop = move |_| open.update(|open| *open = !*open)
            ) {
                {zgui::elements::vector().class("map__icon").document(SLIDERS_ICON)}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_new_style_or_hidden_tracks_count_as_changed() {
        assert!(!changed(&MapView::default()));
        let mut view = MapView::default();
        view.basemap.custom = "https://t/{z}/{x}/{y}.png".to_owned();
        assert!(!changed(&view));
        view.tracks = false;
        assert!(changed(&view));
        let dark = MapView {
            basemap: BasemapChoice {
                preset: Preset::Dark,
                custom: String::new(),
            },
            ..MapView::default()
        };
        assert!(changed(&dark));
    }

    #[test]
    fn a_swatch_shows_land_beside_water() {
        let fill = preset_fill(Preset::Liberty);
        assert!(fill.contains("#f8f4f0") && fill.contains("#9ebdff"));
        assert_eq!(preset_fill(Preset::Custom), CUSTOM_FILL);
    }
}
