use sdrmm_wire::{
    limits::{Bounds as WireBounds, POLARIMETER_LIMITS as LIMITS},
    patch::NodeBody,
    processor::{
        ProcessorReading,
        polarimeter::{PolarimeterParams, PolarimeterReading},
    },
};
use zgui::{
    canvas::{Brush, CanvasScene, ShapeBuilder, zgui_color::Color},
    elements::kurbo,
    prelude::*,
    vocab::SharedString,
};

use crate::{
    store::Store,
    ui::{
        faces::processor::{Processor, edit, error, footer, head, settings},
        kit_raster::{Bounds, number, readout},
        widgets::{check, pick, row_field},
    },
};

pub mod logic;

use logic::{
    ELLIPSE_POINTS, OUTPUTS, Output, PICK_TWO, Point, Which, ellipse_points, lane_edit, left_rows,
    output_of, output_title, right_rows, sense_arrow,
};

const SIZE: f64 = 96.0;
const CENTRE: f64 = SIZE / 2.0;
const RADIUS: f64 = CENTRE - 8.0;
const KHZ: f64 = 1_000.0;
const MIN_LANES: usize = 2;

fn reading_of(reading: &ProcessorReading) -> Option<&PolarimeterReading> {
    match reading {
        ProcessorReading::Polarimeter(found) => Some(found),
        _ => None,
    }
}

fn body(body: &NodeBody) -> Option<&PolarimeterParams> {
    match body {
        NodeBody::Polarimeter(found) => Some(&found.settings),
        _ => None,
    }
}

fn body_mut(body: &mut NodeBody) -> Option<&mut PolarimeterParams> {
    match body {
        NodeBody::Polarimeter(found) => Some(&mut found.settings),
        _ => None,
    }
}

fn whole(bounds: WireBounds<u32>) -> Bounds {
    Bounds::whole(f64::from(bounds.min), f64::from(bounds.max))
}

fn khz(bounds: WireBounds<f64>) -> Bounds {
    Bounds::new(bounds.min / KHZ, bounds.max / KHZ)
}

#[must_use]
pub fn lane_options(lanes: usize) -> Vec<(u32, String)> {
    (0..lanes.max(MIN_LANES) as u32)
        .map(|lane| (lane, (lane + 1).to_string()))
        .collect()
}

fn titled(title: impl Into<String>, body: impl IntoView + 'static) -> AnyView {
    let title = SharedString::from(title.into());
    AnyView::new(view! { box(a11y:description = title) {{body}} })
}

#[derive(Clone)]
struct Polar {
    store: Store,
    node: String,
    params: Memo<Option<PolarimeterParams>>,
}

impl Polar {
    fn set(&self, change: impl FnOnce(&mut PolarimeterParams)) {
        edit(self.store, &self.node, body_mut, change);
    }

    fn read<T: Send + Sync + 'static>(
        &self,
        pick: fn(&PolarimeterParams) -> T,
    ) -> Signal<Option<T>> {
        let params = self.params;
        Signal::derive(move || params.with(|params| params.as_ref().map(pick)))
    }

    fn number(
        &self,
        name: &'static str,
        title: &'static str,
        pick: fn(&PolarimeterParams) -> f64,
        bounds: Bounds,
        write: fn(&mut PolarimeterParams, f64),
    ) -> AnyView {
        let polar = self.clone();
        let value = self.read(pick);
        let shown = Signal::derive(move || value.get().unwrap_or_default());
        titled(
            title,
            row_field(
                name,
                number(name, shown, bounds, move |found| {
                    polar.set(|params| write(params, found));
                }),
            ),
        )
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    let titled = node.clone();
    install_stylesheet("polarimeter-face", SHEET);
    let processor = Processor::new(store, node.clone());
    let params = settings(store, node.clone(), body);
    let reading = processor.reading(reading_of);
    let period = Signal::derive(move || {
        params.with(|found| found.as_ref().map_or(0, |found| found.report_ms))
    });
    let stale = processor.stale(period);
    let age = processor.age();
    let lanes = processor.lanes();
    let polar = Polar {
        store,
        node,
        params,
    };
    let health = processor.health;
    head(store, titled, processor.subtitle(period));
    view! {
        column(class = "face") {
            row(class = "pm__top", class:stale = move || stale.get()) {
                box(class = "pm__glyph", a11y:label = "Polarisation") {
                    {glyph(reading)}
                    text(class = "pm__h") {"H"}
                    text(class = "pm__v") {"V"}
                }
                row(class = "pm__read") {
                    {readout(move || left_rows(reading.get().as_ref()))}
                    {readout(move || right_rows(reading.get().as_ref(), age.get()))}
                }
            }
            {controls(polar, lanes)}
            {error(health)}
            {footer(health, Vec::new)}
        }
    }
}

fn lane_pick(polar: Polar, which: Which, lanes: Signal<usize>, clash: RwSignal<bool>) -> AnyView {
    let (name, title, chosen) = match which {
        Which::H => (
            "H",
            "Lane of the horizontal antenna",
            polar.read(|params| params.h_lane),
        ),
        Which::V => (
            "V",
            "Lane of the vertical antenna",
            polar.read(|params| params.v_lane),
        ),
    };
    let params = polar.params;
    let picked = move |lane: u32| {
        let next = params
            .with_untracked(|params| params.as_ref().map(|found| lane_edit(found, which, lane)));
        let Some(next) = next else {
            return;
        };
        clash.set(next.is_none());
        if let Some(next) = next {
            polar.set(|params| *params = next);
        }
    };
    let options = move || {
        row_field(
            name,
            pick(lane_options(lanes.get()), chosen, picked.clone()),
        )
    };
    titled(title, options)
}

fn controls(polar: Polar, lanes: Signal<usize>) -> impl IntoView {
    let clash = RwSignal::new(false);
    let output = Signal::derive({
        let matched = polar.read(|params| params.matched);
        move || matched.get().map(output_of)
    });
    let output_note = move || {
        SharedString::from(format!(
            "What the beam output carries. {}",
            output.get().map_or("", output_title)
        ))
    };
    let flip = {
        let flipped = polar.read(|params| params.flip_hand);
        Signal::derive(move || flipped.get().unwrap_or_default())
    };
    let outputs = OUTPUTS
        .iter()
        .map(|(value, label, _)| (*value, (*label).to_owned()))
        .collect::<Vec<_>>();
    let set_output = {
        let polar = polar.clone();
        move |chosen: Output| polar.set(|params| params.matched = chosen == Output::Matched)
    };
    let set_flip = {
        let polar = polar.clone();
        move |on: bool| polar.set(|params| params.flip_hand = on)
    };
    view! {
        column(class = "pm__controls") {
        column(class = "params") {
            {lane_pick(polar.clone(), Which::H, lanes, clash)}
            {lane_pick(polar.clone(), Which::V, lanes, clash)}
            box(a11y:description = output_note) {
                {row_field("Output", pick(outputs, output, set_output))}
            }
            {polar.number("Offset kHz", "From the array centre", |params| params.offset_hz / KHZ, khz(LIMITS.band.offset_hz), |params, value| params.offset_hz = value * KHZ)}
            {polar.number("Width kHz", "Band around the offset", |params| params.bandwidth_hz / KHZ, khz(LIMITS.band.bandwidth_hz), |params, value| params.bandwidth_hz = value * KHZ)}
            {polar.number("Report ms", "Report", |params| f64::from(params.report_ms), whole(LIMITS.report_ms), |params, ms| params.report_ms = ms as u32)}
            {polar.number("Average ms", "Average", |params| f64::from(params.average_ms), whole(LIMITS.average_ms), |params, ms| params.average_ms = ms as u32)}
            {polar.number("Fade ms", "Blend old and new weights", |params| f64::from(params.crossfade_ms), whole(LIMITS.crossfade_ms), |params, ms| params.crossfade_ms = ms as u32)}
            {titled("Swap right and left hand", row_field("Flip", check(flip, set_flip)))}
        }
        {move || clash.get().then(|| view! { text(class = "kit-alert") {{PICK_TWO}} })}
        }
    }
}

fn ink(alpha: f32) -> Brush {
    Brush::Solid(Color::srgb(0.33, 0.35, 0.39, alpha))
}

fn accent(alpha: f32) -> Brush {
    Brush::Solid(Color::srgb(0.48, 0.66, 1.0, alpha))
}

fn polyline(
    points: &[Point],
    closed: bool,
    at: impl Fn(Point) -> kurbo::Point,
) -> Option<kurbo::BezPath> {
    let (first, rest) = points.split_first()?;
    let mut path = kurbo::BezPath::new();
    path.move_to(at(*first));
    for point in rest {
        path.line_to(at(*point));
    }
    if closed {
        path.close_path();
    }
    Some(path)
}

fn glyph(reading: Memo<Option<PolarimeterReading>>) -> impl IntoView {
    zgui::elements::canvas()
        .class("pm__plot")
        .draw(move |cx| {
            let scale = f64::from(cx.size.width.0) / SIZE;
            if scale <= 0.0 {
                return;
            }
            let at = |(x, y): Point| kurbo::Point::new(x * scale, y * scale);
            axes(cx.scene, at);
            reading.with(|found| {
                if let Some(found) = found {
                    wave(cx.scene, found, scale, at);
                }
            });
        })
        .into_view()
}

fn axes(scene: &mut CanvasScene, at: impl Fn(Point) -> kurbo::Point + Copy) {
    for ends in [
        [(4.0, CENTRE), (SIZE - 4.0, CENTRE)],
        [(CENTRE, 4.0), (CENTRE, SIZE - 4.0)],
    ] {
        if let Some(line) = polyline(&ends, false, at) {
            scene.push(ShapeBuilder::new(line).stroke(ink(1.0), 1.0).build());
        }
    }
}

fn wave(
    scene: &mut CanvasScene,
    reading: &PolarimeterReading,
    scale: f64,
    at: impl Fn(Point) -> kurbo::Point + Copy,
) {
    let (angle, chi) = (
        f64::from(reading.angle_deg),
        f64::from(reading.ellipticity_deg),
    );
    let points = ellipse_points(angle, chi, RADIUS, CENTRE, ELLIPSE_POINTS);
    if let Some(outline) = polyline(&points, true, at) {
        scene.push(
            ShapeBuilder::new(outline.clone())
                .fill(accent(0.15))
                .build(),
        );
        scene.push(
            ShapeBuilder::new(outline)
                .stroke(accent(1.0), 1.5 * scale)
                .build(),
        );
    }
    let Some(arrow) = sense_arrow(angle, chi, f64::from(reading.v), RADIUS, CENTRE) else {
        return;
    };
    if let Some(arc) = polyline(&arrow.arc, false, at) {
        scene.push(
            ShapeBuilder::new(arc)
                .stroke(accent(1.0), 2.5 * scale)
                .build(),
        );
    }
    if let Some(head) = polyline(&arrow.head, true, at) {
        scene.push(ShapeBuilder::new(head).fill(accent(1.0)).build());
    }
}

const SHEET: &str = css!(
    r#"
.pm__top { gap: 4px; align-items: center; }
.pm__top.stale { opacity: 0.5; }
.pm__glyph {
    position: relative;
    flex: 0 0 auto;
    width: 96px;
    height: 96px;
    margin: 8px 0;
    border-radius: 3px;
    background-color: color-mix(in oklab, var(--line) 35%, transparent);
}
.pm__plot { position: absolute; left: 0; top: 0; width: 96px; height: 96px; }
.pm__h {
    position: absolute;
    right: 4px;
    top: 36px;
    font-family: var(--mono);
    font-size: 8px;
    color: var(--ink-faint);
    pointer-events: none;
}
.pm__v {
    position: absolute;
    left: 51px;
    top: 2px;
    font-family: var(--mono);
    font-size: 8px;
    color: var(--ink-faint);
    pointer-events: none;
}
.pm__controls { gap: 6px; }
.pm__read { flex: 1 1 auto; min-width: 0; gap: 8px; }
.pm__read .rdo { flex: 1 1 0; min-width: 0; }
.pm__read .rdo__label { width: 32px; }
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_pickers_always_offer_at_least_two_lanes() {
        assert_eq!(lane_options(0).len(), 2);
        assert_eq!(lane_options(4)[3], (3, "4".to_owned()));
    }

    #[test]
    fn the_default_polarimeter_is_one_the_server_accepts() {
        assert!(PolarimeterParams::default().valid());
        assert!(khz(LIMITS.band.bandwidth_hz).read("20").is_ok());
    }
}
