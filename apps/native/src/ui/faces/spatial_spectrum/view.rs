use std::{cell::RefCell, rc::Rc};

use zgui::prelude::*;

use super::{
    logic::{BEARING_TICKS, SpatialView, plot_fraction, ticks},
    plot::SpatialScene,
};
use crate::ui::{faces::scope::colormap::Colormap, kit_raster::raster_view};

const FREQ_TICKS: usize = 5;

#[derive(Clone, Copy)]
pub struct Plot {
    pub view: Signal<SpatialView>,
    pub axis: RwSignal<Option<(f64, f64)>>,
}

pub fn plot(scene: Rc<RefCell<SpatialScene>>, plot: Plot) -> impl IntoView {
    let Plot { view, axis } = plot;
    let cursor = RwSignal::new(None::<String>);
    let area = NodeRef::new();
    let hover = {
        let scene = scene.clone();
        move |ev: &mut EventCx<'_, events::PointerMove>| {
            let (Some(bounds), scale) = (area.window_bounds(), area.scale().max(f32::EPSILON))
            else {
                return;
            };
            let at = plot_fraction(
                (bounds.left().0 / scale, bounds.top().0 / scale),
                (bounds.width().0 / scale, bounds.height().0 / scale),
                (ev.position.x.0, ev.position.y.0),
            );
            let text = at.and_then(|(fx, fy)| {
                (view.get_untracked() == SpatialView::Map)
                    .then(|| scene.try_borrow().ok()?.cursor(fx, fy))
                    .flatten()
            });
            cursor.set(text);
        }
    };
    view! {
        box(class = "ss__frame") {
            box(class = "ss__area") {
                {raster_view("ss__raster", scene)}
                box(
                    node_ref = area,
                    class = "ss__hover",
                    on:pointer_move = hover,
                    on:pointer_leave = move |_| cursor.set(None)
                ) {
                    {move || cursor.get().map(|text| AnyView::new(view! { text(class = "ss__cursor") {{text}} }))}
                }
            }
            box(class = "ss__left") {
                {move || if view.get() == SpatialView::Map { bearing_ticks() } else { Vec::new() }}
            }
            box(class = "ss__bottom") {
                {move || axis.get().map_or_else(Vec::new, |(low, high)| freq_ticks(low, high))}
            }
            text(class = "ss__unit ss__unit--x") {"MHz"}
            text(class = "ss__unit ss__unit--y") {
                {move || if view.get() == SpatialView::Map { "\u{b0}" } else { "t" }}
            }
        }
    }
}

fn bearing_ticks() -> Vec<AnyView> {
    BEARING_TICKS
        .iter()
        .map(|deg| {
            let top = f64::from(*deg) / 360.0 * 100.0;
            AnyView::new(view! {
                text(class = "ss__tick ss__tick--y", style:top = Some(format!("calc({top:.2}% - 6px)"))) {
                    {deg.to_string()}
                }
            })
        })
        .collect()
}

fn freq_ticks(low_hz: f64, high_hz: f64) -> Vec<AnyView> {
    ticks(low_hz / 1e6, high_hz / 1e6, FREQ_TICKS)
        .into_iter()
        .map(|(at, label)| {
            let left = at * 100.0;
            AnyView::new(view! {
                text(class = "ss__tick ss__tick--x", style:left = Some(format!("calc({left:.2}% - 24px)"))) {
                    {label}
                }
            })
        })
        .collect()
}

pub fn colour_scale(colormap: Signal<Colormap>, span_db: Signal<f32>) -> impl IntoView {
    view! {
        row(class = "ss__scale", a11y:description = "Below the strongest cell") {
            text {{move || format!("{:.0}", -span_db.get())}}
            box(class = "ss__ramp", style:background-image = move || Some(colormap.get().gradient("to right")))
            text {"0 dB"}
        }
    }
}

pub const SHEET: &str = css!(
    r#"
.ss__bar { align-items: center; gap: 8px; flex-wrap: wrap; }
.ss__frame { position: relative; height: 200px; background-color: rgb(8 9 11); border-radius: 3px; }
.ss__area { position: absolute; left: 32px; right: 6px; top: 4px; bottom: 18px; }
.ss__raster { position: absolute; left: 0; top: 0; right: 0; bottom: 0; }
.ss__hover { position: absolute; left: 0; top: 0; right: 0; bottom: 0; }
.ss__cursor {
    position: absolute;
    left: 4px;
    top: 4px;
    padding: 0 4px;
    border-radius: 2px;
    background-color: color-mix(in oklab, var(--bg) 80%, transparent);
    font-family: var(--mono);
    font-size: 10px;
    color: var(--ink);
}
.ss__left { position: absolute; left: 0; top: 4px; bottom: 18px; width: 30px; }
.ss__bottom { position: absolute; left: 32px; right: 6px; bottom: 0; height: 16px; }
.ss__tick { position: absolute; font-family: var(--mono); font-size: 9px; color: var(--ink-faint); }
.ss__tick--y { right: 2px; height: 12px; line-height: 12px; }
.ss__tick--x { bottom: 2px; width: 48px; text-align: center; }
.ss__unit { position: absolute; font-family: var(--mono); font-size: 9px; color: var(--ink-faint); }
.ss__unit--x { right: 4px; bottom: 2px; }
.ss__unit--y { left: 4px; top: 2px; }
.ss__scale { align-items: center; gap: 4px; margin-left: auto; font-family: var(--mono); font-size: 9px; color: var(--ink-faint); }
.ss__ramp { width: 64px; height: 6px; border-radius: 1px; }
"#
);
