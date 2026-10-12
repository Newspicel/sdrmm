use sdrmm_wire::processor::df::DfReading;
use zgui::{
    canvas::{Brush, CanvasScene, ShapeBuilder, zgui_color::Color},
    elements::{kurbo, kurbo::Shape as _},
    prelude::*,
};

use super::logic::{
    BearingFrame, COMPASS_MARKS, Needle, NeedleWeight, Wedge, frame_rotation, heading_tick,
    polar_point, rose_letters, rose_needles, rose_wedge, spectrum_points, wedge_points,
};

const SIZE: f64 = 220.0;
const CENTRE: f64 = SIZE / 2.0;
const OUTER: f64 = CENTRE - 18.0;
const INNER: f64 = SIZE * 0.12;
const MARK_GAP: f64 = 10.0;
const TICK: f64 = 6.0;

#[derive(Clone, Copy)]
struct Ink {
    dim: bool,
}

impl Ink {
    fn line(self, alpha: f32) -> Brush {
        Brush::Solid(Color::srgb(0.33, 0.35, 0.39, alpha))
    }

    fn accent(self, alpha: f32) -> Brush {
        if self.dim {
            Brush::Solid(Color::srgb(0.55, 0.57, 0.6, alpha))
        } else {
            Brush::Solid(Color::srgb(0.48, 0.66, 1.0, alpha))
        }
    }

    fn faint(self) -> Brush {
        Brush::Solid(Color::srgb(0.45, 0.47, 0.5, 1.0))
    }

    fn array(self) -> Brush {
        if self.dim {
            self.faint()
        } else {
            Brush::Solid(Color::srgb(0.45, 0.52, 0.95, 1.0))
        }
    }
}

pub struct Rose {
    pub reading: Memo<Option<DfReading>>,
    pub frame: Signal<BearingFrame>,
    pub dim: Signal<bool>,
}

pub fn rose(rose: Rose) -> impl IntoView {
    let Rose {
        reading,
        frame,
        dim,
    } = rose;
    view! {
        box(class = "df__rose", a11y:label = "Bearing rose", class:dim = move || dim.get()) {
            {dial(reading, frame, dim)}
            {move || marks(frame.get())}
        }
    }
}

fn marks(frame: BearingFrame) -> Vec<AnyView> {
    rose_letters(frame)
        .iter()
        .map(|(bearing, label)| {
            let (x, y) = polar_point(*bearing, OUTER + MARK_GAP, CENTRE);
            AnyView::new(view! {
                text(
                    class = "df__mark",
                    style:left = Some(format!("{:.1}px", x - 10.0)),
                    style:top = Some(format!("{:.1}px", y - 6.0))
                ) {
                    {*label}
                }
            })
        })
        .collect()
}

fn dial(
    reading: Memo<Option<DfReading>>,
    frame: Signal<BearingFrame>,
    dim: Signal<bool>,
) -> impl IntoView {
    zgui::elements::canvas()
        .class("df__dial")
        .draw(move |cx| {
            let scale = f64::from(cx.size.width.0) / SIZE;
            if scale <= 0.0 {
                return;
            }
            let at = |(x, y): (f64, f64)| kurbo::Point::new(x * scale, y * scale);
            let ink = Ink { dim: dim.get() };
            let frame = frame.get();
            rings(cx.scene, ink, scale, at);
            reading.with(|reading| {
                let Some(reading) = reading.as_ref() else {
                    return;
                };
                if let Some(wedge) = rose_wedge(&reading.peaks, frame) {
                    sigma(cx.scene, ink, wedge, at);
                }
                let rotation = frame_rotation(Some(reading), frame);
                spectrum(cx.scene, ink, &reading.pseudospectrum, rotation, at);
                for needle in rose_needles(&reading.peaks, frame) {
                    draw_needle(cx.scene, ink, needle, scale, at);
                }
                if let Some(deg) = heading_tick(Some(reading), frame) {
                    forward(cx.scene, ink, deg, at);
                }
            });
            let dot = kurbo::Circle::new(at((CENTRE, CENTRE)), 2.0 * scale).to_path(0.1);
            cx.scene
                .push(ShapeBuilder::new(dot).fill(ink.accent(1.0)).build());
        })
        .into_view()
}

fn polyline(points: &[(f64, f64)], at: impl Fn((f64, f64)) -> kurbo::Point) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    if let Some((first, rest)) = points.split_first() {
        path.move_to(at(*first));
        for point in rest {
            path.line_to(at(*point));
        }
        path.close_path();
    }
    path
}

fn rings(scene: &mut CanvasScene, ink: Ink, scale: f64, at: impl Fn((f64, f64)) -> kurbo::Point) {
    let centre = at((CENTRE, CENTRE));
    for (radius, alpha) in [(OUTER, 1.0), ((OUTER + INNER) / 2.0, 0.5)] {
        let ring = kurbo::Circle::new(centre, radius * scale).to_path(0.1);
        scene.push(ShapeBuilder::new(ring).stroke(ink.line(alpha), 1.0).build());
    }
    for (bearing, _) in COMPASS_MARKS {
        let mut tick = kurbo::BezPath::new();
        tick.move_to(at(polar_point(bearing, OUTER - TICK, CENTRE)));
        tick.line_to(at(polar_point(bearing, OUTER, CENTRE)));
        scene.push(ShapeBuilder::new(tick).stroke(ink.line(1.0), 1.0).build());
    }
}

fn sigma(scene: &mut CanvasScene, ink: Ink, wedge: Wedge, at: impl Fn((f64, f64)) -> kurbo::Point) {
    let path = polyline(&wedge_points(wedge, CENTRE, OUTER), at);
    scene.push(ShapeBuilder::new(path).fill(ink.accent(0.2)).build());
}

fn spectrum(
    scene: &mut CanvasScene,
    ink: Ink,
    levels: &[u8],
    rotation: f64,
    at: impl Fn((f64, f64)) -> kurbo::Point,
) {
    if levels.is_empty() {
        return;
    }
    let outline = polyline(&spectrum_points(levels, CENTRE, INNER, OUTER, rotation), at);
    scene.push(
        ShapeBuilder::new(outline.clone())
            .fill(ink.accent(0.15))
            .build(),
    );
    scene.push(
        ShapeBuilder::new(outline)
            .stroke(ink.accent(0.8), 1.0)
            .build(),
    );
}

fn draw_needle(
    scene: &mut CanvasScene,
    ink: Ink,
    needle: Needle,
    scale: f64,
    at: impl Fn((f64, f64)) -> kurbo::Point,
) {
    let mut line = kurbo::BezPath::new();
    line.move_to(at((CENTRE, CENTRE)));
    line.line_to(at(polar_point(needle.deg, OUTER, CENTRE)));
    let (brush, width, dashes) = match needle.weight {
        NeedleWeight::Primary => (ink.accent(1.0), 2.0, None),
        NeedleWeight::Secondary => (ink.accent(0.6), 1.0, Some([3.0, 2.0])),
        NeedleWeight::Mirror => (ink.faint(), 1.0, Some([1.0, 2.0])),
    };
    let stroke = kurbo::Stroke::new(width * scale);
    let stroke = match dashes {
        Some(pattern) => stroke.with_dashes(0.0, pattern.map(|dash| dash * scale)),
        None => stroke,
    };
    scene.push(ShapeBuilder::new(line).stroke_styled(brush, stroke).build());
}

fn forward(scene: &mut CanvasScene, ink: Ink, deg: f64, at: impl Fn((f64, f64)) -> kurbo::Point) {
    let tip = polar_point(deg, OUTER - TICK, CENTRE);
    let left = polar_point(deg - 4.0, OUTER + 2.0, CENTRE);
    let right = polar_point(deg + 4.0, OUTER + 2.0, CENTRE);
    let path = polyline(&[tip, left, right], at);
    scene.push(ShapeBuilder::new(path).fill(ink.array()).build());
}

pub const SHEET: &str = css!(
    r#"
.df__rose { position: relative; width: 220px; height: 220px; align-self: center; }
.df__rose.dim { opacity: 0.45; }
.df__dial { position: absolute; left: 0; top: 0; width: 220px; height: 220px; }
.df__mark {
    position: absolute;
    width: 20px;
    height: 12px;
    text-align: center;
    font-family: var(--mono);
    font-size: 9px;
    line-height: 12px;
    color: var(--ink-dim);
    pointer-events: none;
}
"#
);
