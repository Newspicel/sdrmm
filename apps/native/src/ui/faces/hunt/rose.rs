use sdrmm_wire::hunt::HuntSweep;
use zgui::{
    canvas::{Brush, CanvasScene, ShapeBuilder, zgui_color::Color},
    elements::{kurbo, kurbo::Shape as _},
    prelude::*,
};

use crate::ui::faces::df::logic::{
    COMPASS_MARKS, Needle, NeedleWeight, TRUE_MARKS, polar_point, spectrum_points,
};

const SIZE: f64 = 160.0;
const CENTRE: f64 = SIZE / 2.0;
const OUTER: f64 = CENTRE - 16.0;
const INNER: f64 = SIZE * 0.12;
const MARK_GAP: f64 = 9.0;
const TICK: f64 = 5.0;

#[must_use]
pub fn sweep_needles(sweep: Option<&HuntSweep>) -> Vec<Needle> {
    let Some(sweep) = sweep else {
        return Vec::new();
    };
    let mut needles = Vec::new();
    if let Some(deg) = sweep.peak_deg {
        needles.push(Needle {
            deg: f64::from(deg),
            weight: NeedleWeight::Primary,
        });
    }
    if let Some(deg) = sweep.heading_deg {
        needles.push(Needle {
            deg: f64::from(deg),
            weight: NeedleWeight::Secondary,
        });
    }
    needles
}

fn line(alpha: f32) -> Brush {
    Brush::Solid(Color::srgb(0.33, 0.35, 0.39, alpha))
}

fn accent(dim: bool, alpha: f32) -> Brush {
    if dim {
        Brush::Solid(Color::srgb(0.55, 0.57, 0.6, alpha))
    } else {
        Brush::Solid(Color::srgb(0.48, 0.66, 1.0, alpha))
    }
}

fn closed(points: &[(f64, f64)], at: impl Fn((f64, f64)) -> kurbo::Point) -> kurbo::BezPath {
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

fn rings(scene: &mut CanvasScene, scale: f64, at: impl Fn((f64, f64)) -> kurbo::Point) {
    let centre = at((CENTRE, CENTRE));
    for (radius, alpha) in [(OUTER, 1.0), ((OUTER + INNER) / 2.0, 0.5)] {
        let ring = kurbo::Circle::new(centre, radius * scale).to_path(0.1);
        scene.push(ShapeBuilder::new(ring).stroke(line(alpha), 1.0).build());
    }
    for (bearing, _) in COMPASS_MARKS {
        let mut tick = kurbo::BezPath::new();
        tick.move_to(at(polar_point(bearing, OUTER - TICK, CENTRE)));
        tick.line_to(at(polar_point(bearing, OUTER, CENTRE)));
        scene.push(ShapeBuilder::new(tick).stroke(line(1.0), 1.0).build());
    }
}

fn needle(
    scene: &mut CanvasScene,
    dim: bool,
    needle: Needle,
    scale: f64,
    at: impl Fn((f64, f64)) -> kurbo::Point,
) {
    let mut path = kurbo::BezPath::new();
    path.move_to(at((CENTRE, CENTRE)));
    path.line_to(at(polar_point(needle.deg, OUTER, CENTRE)));
    let (brush, width, dashed) = match needle.weight {
        NeedleWeight::Primary => (accent(dim, 1.0), 2.0, false),
        NeedleWeight::Secondary | NeedleWeight::Mirror => (accent(dim, 0.6), 1.0, true),
    };
    let stroke = kurbo::Stroke::new(width * scale);
    let stroke = if dashed {
        stroke.with_dashes(0.0, [3.0 * scale, 2.0 * scale])
    } else {
        stroke
    };
    scene.push(ShapeBuilder::new(path).stroke_styled(brush, stroke).build());
}

fn dial(sweep: Memo<Option<HuntSweep>>, dim: Signal<bool>) -> impl IntoView {
    zgui::elements::canvas()
        .class("hunt-rose__dial")
        .draw(move |cx| {
            let scale = f64::from(cx.size.width.0) / SIZE;
            if scale <= 0.0 {
                return;
            }
            let at = |(x, y): (f64, f64)| kurbo::Point::new(x * scale, y * scale);
            let dim = dim.get();
            rings(cx.scene, scale, at);
            sweep.with(|sweep| {
                let bins = sweep
                    .as_ref()
                    .map(|sweep| sweep.bins.as_slice())
                    .unwrap_or_default();
                if !bins.is_empty() {
                    let outline = closed(&spectrum_points(bins, CENTRE, INNER, OUTER, 0.0), at);
                    cx.scene.push(
                        ShapeBuilder::new(outline.clone())
                            .fill(accent(dim, 0.15))
                            .build(),
                    );
                    cx.scene.push(
                        ShapeBuilder::new(outline)
                            .stroke(accent(dim, 0.8), 1.0)
                            .build(),
                    );
                }
                for shown in sweep_needles(sweep.as_ref()) {
                    needle(cx.scene, dim, shown, scale, at);
                }
            });
            let dot = kurbo::Circle::new(at((CENTRE, CENTRE)), 2.0 * scale).to_path(0.1);
            cx.scene
                .push(ShapeBuilder::new(dot).fill(accent(dim, 1.0)).build());
        })
        .into_view()
}

pub fn rose(sweep: Memo<Option<HuntSweep>>, dim: Signal<bool>) -> impl IntoView {
    let marks: Vec<AnyView> = TRUE_MARKS
        .iter()
        .map(|(bearing, label)| {
            let (x, y) = polar_point(*bearing, OUTER + MARK_GAP, CENTRE);
            AnyView::new(view! {
                text(
                    class = "hunt-rose__mark",
                    style:left = Some(format!("{:.1}px", x - 10.0)),
                    style:top = Some(format!("{:.1}px", y - 6.0))
                ) {
                    {*label}
                }
            })
        })
        .collect();
    view! {
        box(class = "hunt-rose", class:dim = move || dim.get(), a11y:label = "Sweep rose") {
            {dial(sweep, dim)}
            {marks}
        }
    }
}

pub const SHEET: &str = css!(
    r#"
.hunt-rose { position: relative; width: 160px; height: 160px; align-self: center; }
.hunt-rose.dim { opacity: 0.45; }
.hunt-rose__dial { position: absolute; left: 0; top: 0; width: 160px; height: 160px; }
.hunt-rose__mark {
    position: absolute; width: 20px; height: 12px; text-align: center;
    font-family: var(--mono); font-size: 9px; line-height: 12px; color: var(--ink-dim);
    pointer-events: none;
}
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peak_leads_and_the_heading_follows() {
        assert!(sweep_needles(None).is_empty());
        let sweep = HuntSweep {
            peak_deg: Some(40.0),
            heading_deg: Some(90.0),
            ..HuntSweep::default()
        };
        let needles = sweep_needles(Some(&sweep));
        assert_eq!(needles.len(), 2);
        assert_eq!(needles[0].weight, NeedleWeight::Primary);
        assert_eq!(needles[1].deg, 90.0);
        assert_eq!(needles[1].weight, NeedleWeight::Secondary);
    }
}
