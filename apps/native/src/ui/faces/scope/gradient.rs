use zgui::{
    geom::{Css, CssPx, Point},
    prelude::*,
    reactive::RenderEffect,
};
use zgui_ui::prelude::*;

use super::{
    ScopeCx,
    colormap::Rgb,
    palette::{CUSTOM_STOPS, Hsv, hex_to_rgb, hsv_to_rgb, rgb_to_hex, rgb_to_hsv},
};
use crate::ui::kit_shell::typing;

const KEY_STEP: f64 = 0.02;
const KEY_STEP_LARGE: f64 = 0.1;
const HUE_TRACK: &str =
    "linear-gradient(to right, #ff0000, #ffff00, #00ff00, #00ffff, #0000ff, #ff00ff, #ff0000)";

#[must_use]
pub fn with_stop(stops: [Rgb; CUSTOM_STOPS], index: usize, rgb: Rgb) -> [Rgb; CUSTOM_STOPS] {
    let mut next = stops;
    if let Some(slot) = next.get_mut(index) {
        *slot = rgb;
    }
    next
}

#[must_use]
pub fn ramp(stops: &[Rgb; CUSTOM_STOPS]) -> String {
    let hexes: Vec<String> = stops.iter().map(|stop| rgb_to_hex(*stop)).collect();
    format!("linear-gradient(to right, {})", hexes.join(", "))
}

#[must_use]
pub fn stop_at(index: usize) -> f64 {
    index as f64 / (CUSTOM_STOPS - 1) as f64 * 100.0
}

#[must_use]
pub fn sv_at(x: f64, y: f64, hue: f64) -> Hsv {
    Hsv {
        h: hue,
        s: x.clamp(0.0, 1.0),
        v: (1.0 - y).clamp(0.0, 1.0),
    }
}

#[must_use]
pub fn nudged(hsv: Hsv, key: &Key, large: bool) -> Option<Hsv> {
    let step = if large { KEY_STEP_LARGE } else { KEY_STEP };
    let unit = |value: f64| value.clamp(0.0, 1.0);
    match key {
        Key::Named(NamedKey::ArrowLeft) => Some(Hsv {
            s: unit(hsv.s - step),
            ..hsv
        }),
        Key::Named(NamedKey::ArrowRight) => Some(Hsv {
            s: unit(hsv.s + step),
            ..hsv
        }),
        Key::Named(NamedKey::ArrowDown) => Some(Hsv {
            v: unit(hsv.v - step),
            ..hsv
        }),
        Key::Named(NamedKey::ArrowUp) => Some(Hsv {
            v: unit(hsv.v + step),
            ..hsv
        }),
        _ => None,
    }
}

#[must_use]
pub fn pure_hue(hue: f64) -> String {
    rgb_to_hex(hsv_to_rgb(Hsv {
        h: hue,
        s: 1.0,
        v: 1.0,
    }))
}

fn fraction_in(node: NodeRef, position: Point<CssPx, Css>) -> Option<(f64, f64)> {
    let bounds = node.window_bounds()?;
    let scale = f64::from(node.scale().max(0.01));
    let width = f64::from(bounds.size.width.0) / scale;
    let height = f64::from(bounds.size.height.0) / scale;
    (width > 0.0 && height > 0.0).then(|| {
        (
            ((f64::from(position.x.0) - f64::from(bounds.origin.x.0) / scale) / width)
                .clamp(0.0, 1.0),
            ((f64::from(position.y.0) - f64::from(bounds.origin.y.0) / scale) / height)
                .clamp(0.0, 1.0),
        )
    })
}

fn percent(value: f64) -> Option<String> {
    Some(format!("{:.2}%", value * 100.0))
}

pub fn editor(cx: ScopeCx) -> impl IntoView {
    let selected = RwSignal::new(CUSTOM_STOPS - 1);
    let marks: Vec<_> = (0..CUSTOM_STOPS)
        .map(|index| {
            view! {
                control(
                    class = "scope__stop",
                    class:on = move || selected.get() == index,
                    a11y:role = Role::Button,
                    a11y:label = format!("Colour stop {}", index + 1),
                    style:left = Some(format!("{:.2}%", stop_at(index))),
                    style:background-color = move || Some(rgb_to_hex(cx.custom.get()[index])),
                    on:click:stop = move |_| selected.set(index)
                )
            }
        })
        .collect();
    view! {
        column(class = "scope__gradient") {
            box(class = "scope__gradient-ramp", style:background-image = move || Some(ramp(&cx.custom.get())))
            box(class = "scope__stops") {{marks}}
            {move || {
                let index = selected.get();
                AnyView::new(picker(cx, index))
            }}
        }
    }
}

fn picker(cx: ScopeCx, index: usize) -> impl IntoView {
    let hsv = RwSignal::new(rgb_to_hsv(cx.custom.get_untracked()[index]));
    let apply = move |next: Hsv| {
        hsv.set(next);
        cx.shape_custom(with_stop(
            cx.custom.get_untracked(),
            index,
            hsv_to_rgb(next),
        ));
    };
    let label = format!("Colour stop {}", index + 1);
    view! {
        column(class = "scope__picker-body") {
            {saturation_value(cx, hsv, apply, label.clone())}
            {hue_slider(cx, hsv, apply, format!("{label} hue"))}
            {hex_row(cx, index, hsv, label)}
        }
    }
}

fn saturation_value(
    cx: ScopeCx,
    hsv: RwSignal<Hsv>,
    apply: impl Fn(Hsv) + Copy + 'static,
    label: String,
) -> impl IntoView {
    let area = NodeRef::new();
    let held = RwSignal::new(false);
    let pick = move |position: Point<CssPx, Css>| {
        if let Some((x, y)) = fraction_in(area, position) {
            apply(sv_at(x, y, hsv.get_untracked().h));
        }
    };
    view! {
        box(
            node_ref = area,
            class = "scope__sv",
            tabindex = Focus::Sequential,
            a11y:role = Role::Slider,
            a11y:label = label,
            style:background-color = move || Some(pure_hue(hsv.get().h)),
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                ev.stop_propagation();
                ev.capture_pointer();
                held.set(true);
                pick(ev.position);
            },
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                if held.get_untracked() {
                    pick(ev.position);
                }
            },
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                ev.release_pointer();
                held.set(false);
                cx.keep_custom();
            },
            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                if let Some(next) = nudged(hsv.get_untracked(), &ev.key, ev.modifiers.shift()) {
                    ev.prevent_default();
                    ev.stop_propagation();
                    apply(next);
                    cx.keep_custom();
                }
            }
        ) {
            box(class = "scope__sv-white")
            box(class = "scope__sv-black")
            box(
                class = "scope__sv-thumb",
                style:left = move || percent(hsv.get().s),
                style:top = move || percent(1.0 - hsv.get().v),
                style:background-color = move || Some(rgb_to_hex(hsv_to_rgb(hsv.get())))
            )
        }
    }
}

fn hue_slider(
    cx: ScopeCx,
    hsv: RwSignal<Hsv>,
    apply: impl Fn(Hsv) + Copy + 'static,
    label: String,
) -> impl IntoView {
    let track = NodeRef::new();
    let held = RwSignal::new(false);
    let seek = move |position: Point<CssPx, Css>| {
        if let Some((x, _)) = fraction_in(track, position) {
            apply(Hsv {
                h: (x * 359.0).round(),
                ..hsv.get_untracked()
            });
        }
    };
    view! {
        box(
            node_ref = track,
            class = "scope__hue",
            tabindex = Focus::Sequential,
            a11y:role = Role::Slider,
            a11y:label = label,
            style:background-image = Some(HUE_TRACK.to_owned()),
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                ev.stop_propagation();
                ev.capture_pointer();
                held.set(true);
                seek(ev.position);
            },
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                if held.get_untracked() {
                    seek(ev.position);
                }
            },
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                ev.release_pointer();
                held.set(false);
                cx.keep_custom();
            },
            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                let step = if ev.modifiers.shift() { 10.0 } else { 1.0 };
                let now = hsv.get_untracked();
                let h = match ev.key {
                    Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowDown) => now.h - step,
                    Key::Named(NamedKey::ArrowRight | NamedKey::ArrowUp) => now.h + step,
                    _ => return,
                };
                ev.prevent_default();
                ev.stop_propagation();
                apply(Hsv { h: h.clamp(0.0, 359.0), ..now });
                cx.keep_custom();
            }
        ) {
            box(
                class = "scope__hue-thumb",
                style:left = move || percent(hsv.get().h / 359.0),
                style:background-color = move || Some(pure_hue(hsv.get().h))
            )
        }
    }
}

fn hex_row(cx: ScopeCx, index: usize, hsv: RwSignal<Hsv>, label: String) -> impl IntoView {
    let colour = Memo::new(move |_| rgb_to_hex(cx.custom.get()[index]));
    let draft = RwSignal::new_local(colour.get_untracked());
    let focused = RwSignal::new_local(false);
    let sync = RenderEffect::new(move |_| {
        let shown = colour.get();
        if !focused.get() {
            draft.set(shown);
        }
    });
    on_cleanup_local(move || drop(sync));
    let commit = move || {
        let text = draft.get_untracked();
        let typed = if text.starts_with('#') {
            text
        } else {
            format!("#{text}")
        };
        match hex_to_rgb(&typed) {
            Some(rgb) => {
                hsv.set(rgb_to_hsv(rgb));
                cx.shape_custom(with_stop(cx.custom.get_untracked(), index, rgb));
                cx.keep_custom();
            }
            None => draft.set(colour.get_untracked()),
        }
    };
    view! {
        row(class = "scope__hex") {
            box(class = "scope__hex-swatch", style:background-color = move || Some(colour.get()))
            box(class = "scope__field") {
                Input(
                    value = draft,
                    class = "native-input",
                    label = format!("{label} hex"),
                    on:focus_in = move |_| focused.set(true),
                    on:focus_out = move |_| { commit(); focused.set(false); },
                    on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                        if matches!(ev.key, Key::Named(NamedKey::Enter)) {
                            commit();
                            ev.prevent_default();
                        }
                        typing(ev);
                    },
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{super::palette::DEFAULT_CUSTOM, *};

    #[test]
    fn editing_a_stop_leaves_the_others_alone() {
        let next = with_stop(DEFAULT_CUSTOM, 1, [1.0, 1.0, 1.0]);
        assert_eq!(next[1], [1.0, 1.0, 1.0]);
        assert_eq!(next[0], DEFAULT_CUSTOM[0]);
        assert_eq!(with_stop(DEFAULT_CUSTOM, 9, [1.0; 3]), DEFAULT_CUSTOM);
    }

    #[test]
    fn the_stops_spread_evenly_along_the_ramp() {
        assert_eq!(stop_at(0), 0.0);
        assert_eq!(stop_at(2), 50.0);
        assert_eq!(stop_at(4), 100.0);
        assert!(ramp(&DEFAULT_CUSTOM).starts_with("linear-gradient(to right, #000020"));
    }

    #[test]
    fn the_area_reads_saturation_across_and_brightness_up() {
        let hsv = sv_at(0.25, 0.75, 120.0);
        assert_eq!((hsv.h, hsv.s, hsv.v), (120.0, 0.25, 0.25));
        let clamped = sv_at(2.0, -1.0, 0.0);
        assert_eq!((clamped.s, clamped.v), (1.0, 1.0));
    }

    #[test]
    fn arrow_keys_nudge_saturation_and_brightness() {
        let hsv = Hsv {
            h: 0.0,
            s: 0.5,
            v: 0.5,
        };
        let right = nudged(hsv, &Key::Named(NamedKey::ArrowRight), false);
        assert!(right.is_some_and(|next| (next.s - 0.52).abs() < 1e-9));
        let down = nudged(hsv, &Key::Named(NamedKey::ArrowDown), true);
        assert!(down.is_some_and(|next| (next.v - 0.4).abs() < 1e-9));
        assert_eq!(nudged(hsv, &Key::Named(NamedKey::Enter), false), None);
    }

    #[test]
    fn the_hue_backdrop_is_the_pure_colour() {
        assert_eq!(pure_hue(0.0), "#ff0000");
        assert_eq!(pure_hue(120.0), "#00ff00");
    }
}
