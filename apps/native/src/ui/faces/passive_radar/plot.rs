use std::{cell::RefCell, rc::Rc};

use sdrmm_wire::limits::RADAR_LIMITS;
use zgui::prelude::*;

use super::radar::{
    Marks, PlotAxes, RadarScene, Tick, doppler_ticks, hover_text, range_ticks, speed_ticks,
    track_labels,
};
use crate::ui::{
    faces::scope::colormap::Colormap, kit_raster::raster_view, tools::kit::local_point,
};

#[derive(Clone)]
pub struct Plot {
    pub scene: Rc<RefCell<RadarScene>>,
    pub axes: Memo<Option<PlotAxes>>,
    pub marks: Memo<Marks>,
    pub refused: RwSignal<Option<String>>,
    pub colormap: RwSignal<Colormap>,
}

fn percent(fraction: f64) -> Option<String> {
    Some(format!("{:.2}%", fraction * 100.0))
}

fn left_tick(tick: Tick) -> AnyView {
    AnyView::new(view! {
        text(class = "rd__tick rd__tick--left", style:top = percent(tick.at)) {{tick.label}}
    })
}

fn right_tick(tick: Tick) -> AnyView {
    AnyView::new(view! {
        text(class = "rd__tick rd__tick--right", style:top = percent(tick.at)) {{tick.label}}
    })
}

fn bottom_tick(tick: Tick) -> AnyView {
    AnyView::new(view! {
        text(class = "rd__tick rd__tick--bottom", style:left = percent(tick.at)) {{tick.label}}
    })
}

fn axis_labels(axes: Memo<Option<PlotAxes>>) -> impl IntoView {
    move || {
        axes.get().map(|axes| {
            let mut shown: Vec<AnyView> = doppler_ticks(&axes).into_iter().map(left_tick).collect();
            shown.extend(speed_ticks(&axes).into_iter().map(right_tick));
            shown.extend(range_ticks(&axes).into_iter().map(bottom_tick));
            shown
        })
    }
}

fn mark_labels(marks: Memo<Marks>) -> impl IntoView {
    move || {
        marks.with(|marks| {
            track_labels(marks)
                .into_iter()
                .map(|label| {
                    view! {
                        text(
                            class = "rd__mark",
                            style:left = percent(label.x),
                            style:top = percent(label.y)
                        ) {{label.text}}
                    }
                })
                .collect::<Vec<_>>()
        })
    }
}

fn scale(plot: &Plot) -> impl IntoView {
    let (axes, colormap) = (plot.axes, plot.colormap);
    let low = move || {
        let db = axes
            .get()
            .map_or(f64::from(RADAR_LIMITS.surface_db.min), |axes| axes.db_min);
        format!("{db:.0}")
    };
    let high = move || {
        let db = axes
            .get()
            .map_or(f64::from(RADAR_LIMITS.surface_db.max), |axes| axes.db_max);
        format!("{db:.0} dB")
    };
    view! {
        row(class = "rd__scale") {
            text {{low}}
            box(class = "rd__ramp", style:background-image = move || Some(colormap.get().gradient("to right"))) {}
            text {{high}}
        }
    }
}

fn hover_label(plot: &Plot, hover: RwSignal<Option<(f64, f64)>>) -> impl IntoView {
    let (axes, scene) = (plot.axes, plot.scene.clone());
    move || {
        let (x, y) = hover.get()?;
        let axes = axes.get()?;
        let level = scene.try_borrow().ok().and_then(|scene| {
            scene
                .surface
                .as_ref()
                .and_then(|surface| surface.level_at(x, y))
        });
        let (range_km, doppler_hz) = axes.point_at(x, y);
        let text = hover_text(
            range_km,
            doppler_hz,
            axes.carrier_hz,
            level.map(|level| axes.level_db(level)),
        );
        Some(view! { text(class = "rd__hover") {{text}} })
    }
}

pub fn plot(plot: Plot) -> impl IntoView {
    let area = NodeRef::new();
    let hover = RwSignal::new(None::<(f64, f64)>);
    let track = move |ev: &mut EventCx<'_, events::PointerMove>| {
        let found = local_point(area, ev.position).and_then(|(x, y, width, height)| {
            (width > 0.0 && height > 0.0).then(|| (x / width, y / height))
        });
        hover.set(found);
    };
    let refused = plot.refused;
    view! {
        box(class = "rd__plot") {
            box(
                class = "rd__grid",
                node_ref = area,
                on:pointer_move = track,
                on:pointer_leave = move |_| hover.set(None)
            ) {
                {raster_view("rd__surface", plot.scene.clone())}
                {axis_labels(plot.axes)}
                {mark_labels(plot.marks)}
                {hover_label(&plot, hover)}
            }
            text(class = "rd__unit rd__unit--hz") {"Hz"}
            text(class = "rd__unit rd__unit--mps") {"m/s"}
            text(class = "rd__unit rd__unit--km") {"km"}
            {scale(&plot)}
            {move || refused.get().map(|text| view! { text(class = "rd__refused") {{text}} })}
        }
    }
}
