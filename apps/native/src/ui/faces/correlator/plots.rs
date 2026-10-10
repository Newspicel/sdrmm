use sdrmm_wire::{
    frame::{FrameKind, VisibilityFrame, VisibilityOwned},
    ws::ServerEvent,
};
use zgui::{
    canvas::{CanvasScene, ShapeBuilder},
    elements::{kurbo, kurbo::Shape as _},
    prelude::*,
    reactive::RenderEffect,
    vocab::SharedString,
};

use super::logic::{
    FRINGE_POINTS, Gutters, Range, Rect, Tick, baseline_series, frequency_range, fringe_runs,
    level_range, linear_ticks, owned, phase_runs, phase_ticks, plot_rects, trace,
};
use crate::{
    store::Store,
    ui::{faces::processor::surface, tools::kit::Paint},
};

pub const HEIGHT: f64 = 200.0;
pub const FRINGE_W: f64 = 72.0;
pub const FRINGE_H: f64 = 22.0;
const GUTTERS: Gutters = Gutters {
    left: 34.0,
    right: 6.0,
    top: 12.0,
    bottom: 18.0,
};
const GAP: f64 = 16.0;
const FREQ_TICKS: f64 = 5.0;
const LEVEL_TICKS: f64 = 3.0;
const LABEL_HALF_PX: f64 = 6.0;
const DIM: f32 = 0.5;

fn polyline(points: &[(f64, f64)]) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    for (index, at) in points.iter().enumerate() {
        if index == 0 {
            path.move_to(*at);
        } else {
            path.line_to(*at);
        }
    }
    path
}

fn stroke(scene: &mut CanvasScene, points: &[(f64, f64)], paint: Paint, alpha: f32, width: f64) {
    if points.len() > 1 {
        scene.push(
            ShapeBuilder::new(polyline(points))
                .stroke(paint.brush(alpha), width)
                .build(),
        );
    }
}

fn outline(scene: &mut CanvasScene, rect: Rect) {
    let shape = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.w, rect.y + rect.h).to_path(0.1);
    scene.push(
        ShapeBuilder::new(shape)
            .stroke(Paint::Line.brush(1.0), 1.0)
            .build(),
    );
}

fn grid(scene: &mut CanvasScene, rect: Rect, rows: &[Tick], columns: &[Tick]) {
    for tick in rows {
        let y = rect.y + (1.0 - tick.at) * rect.h;
        stroke(
            scene,
            &[(rect.x, y), (rect.x + rect.w, y)],
            Paint::PlotGrid,
            1.0,
            1.0,
        );
    }
    for tick in columns {
        let x = rect.x + tick.at * rect.w;
        stroke(
            scene,
            &[(x, rect.y), (x, rect.y + rect.h)],
            Paint::PlotGrid,
            1.0,
            1.0,
        );
    }
}

fn paint(scene: &mut CanvasScene, width: f64, frame: &VisibilityOwned, index: usize, dim: bool) {
    let Some((amplitude, phase)) = plot_rects(width, HEIGHT, GUTTERS, GAP) else {
        return;
    };
    let (x, y) = (frequency_range(frame), level_range(frame));
    let columns = linear_ticks(megahertz(x), FREQ_TICKS);
    grid(scene, amplitude, &linear_ticks(y, LEVEL_TICKS), &columns);
    grid(scene, phase, &phase_ticks(), &columns);
    outline(scene, amplitude);
    outline(scene, phase);
    let alpha = if dim { DIM } else { 1.0 };
    let series = baseline_series(frame, index);
    stroke(
        scene,
        &trace(&series.hz, &series.db, amplitude, x, y),
        Paint::Trace,
        alpha,
        1.25,
    );
    for run in phase_runs(&series.hz, &series.deg, phase, x) {
        stroke(scene, &run, Paint::Accent, alpha, 1.25);
    }
}

fn megahertz(range: Range) -> Range {
    Range {
        lo: range.lo / 1e6,
        hi: range.hi / 1e6,
    }
}

fn rects() -> Option<(Rect, Rect)> {
    plot_rects(f64::from(u16::MAX), HEIGHT, GUTTERS, GAP)
}

fn phase_label_top() -> f64 {
    rects().map_or(0.0, |(_, phase)| phase.y - GUTTERS.top)
}

fn level_labels(frame: Option<&VisibilityOwned>) -> Vec<AnyView> {
    let Some(frame) = frame else {
        return Vec::new();
    };
    let Some((amplitude, phase)) = rects() else {
        return Vec::new();
    };
    let levels = linear_ticks(level_range(frame), LEVEL_TICKS);
    side_labels(amplitude, &levels)
        .into_iter()
        .chain(side_labels(phase, &phase_ticks()))
        .collect()
}

fn side_labels(rect: Rect, ticks: &[Tick]) -> Vec<AnyView> {
    ticks
        .iter()
        .map(|tick| {
            let top = rect.y + (1.0 - tick.at) * rect.h - LABEL_HALF_PX;
            AnyView::new(view! {
                text(class = "cor__tick", style:top = Some(format!("{top:.1}px"))) {{tick.label.clone()}}
            })
        })
        .collect()
}

fn frequency_labels(frame: Option<&VisibilityOwned>) -> Vec<AnyView> {
    let Some(frame) = frame else {
        return Vec::new();
    };
    linear_ticks(megahertz(frequency_range(frame)), FREQ_TICKS)
        .into_iter()
        .map(|tick| {
            let left = tick.at * 100.0;
            AnyView::new(view! {
                text(class = "cor__freq", style:left = Some(format!("{left:.2}%"))) {{tick.label}}
            })
        })
        .collect()
}

fn subscribe(
    store: Store,
    node: String,
    known: Memo<bool>,
    frame: RwSignal<Option<VisibilityOwned>>,
    refused: RwSignal<Option<String>>,
) {
    let show = move |bytes: &[u8]| match VisibilityFrame::decode(bytes) {
        Ok(decoded) => {
            frame.set(Some(owned(&decoded)));
            if refused.with_untracked(Option::is_some) {
                refused.set(None);
            }
        }
        Err(error) => refused.set(Some(format!("Bad frame: {error}"))),
    };
    let watched = node.clone();
    store.on_event(move |event| {
        if let ServerEvent::SurfaceRefused { node, reason } = event
            && *node == watched
        {
            refused.set(Some(reason.label().to_owned()));
        }
    });
    let holding = RenderEffect::new(move |_| {
        if known.get() {
            surface(
                store,
                node.clone(),
                FrameKind::Visibility,
                None,
                show,
            );
        }
    });
    on_cleanup_local(move || drop(holding));
}

pub fn plots(
    store: Store,
    node: String,
    known: Memo<bool>,
    index: Signal<usize>,
    dim: Signal<bool>,
) -> impl IntoView {
    let frame = RwSignal::new(None::<VisibilityOwned>);
    let refused = RwSignal::new(None::<String>);
    subscribe(store, node, known, frame, refused);
    let canvas = zgui::elements::canvas()
        .class("cor__canvas")
        .draw(move |cx| {
            let width = f64::from(cx.size.width.0);
            let (index, dim) = (index.get(), dim.get());
            frame.with(|frame| {
                if let Some(frame) = frame {
                    paint(cx.scene, width, frame, index, dim);
                }
            });
        })
        .into_view();
    let phase_top = format!("{:.0}px", phase_label_top());
    view! {
        box(class = "cor__plot", a11y:role = Role::Image, a11y:label = "Amplitude and phase over frequency") {
            {canvas}
            text(class = "cor__name", style:top = "0px") {"Amplitude dB"}
            text(class = "cor__name", style:top = Some(phase_top)) {"Phase \u{b0}"}
            text(class = "cor__name cor__unit") {"MHz"}
            {move || frame.with(|frame| level_labels(frame.as_ref()))}
            box(class = "cor__axis") {
                {move || frame.with(|frame| frequency_labels(frame.as_ref()))}
            }
            {move || refused.get().map(|text| view! { text(class = "kit-alert cor__refused") {{text}} })}
        }
    }
}

pub fn fringe_line(history: Signal<Vec<f64>>) -> impl IntoView {
    let canvas = zgui::elements::canvas()
        .class("cor__fringe")
        .draw(move |cx| {
            let rect = Rect {
                x: 0.0,
                y: 0.0,
                w: FRINGE_W,
                h: FRINGE_H,
            };
            history.with(|history| {
                for run in fringe_runs(history, rect) {
                    stroke(cx.scene, &run, Paint::Accent, 1.0, 1.0);
                }
            });
        })
        .into_view();
    let title = format!("Phase over the last {FRINGE_POINTS} results");
    view! {
        box(
            class = "cor__fringe_box",
            a11y:role = Role::Image,
            a11y:label = "Fringe phase",
            a11y:description = SharedString::from(title)
        ) {
            {canvas}
        }
    }
}
