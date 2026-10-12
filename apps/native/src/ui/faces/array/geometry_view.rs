use sdrmm_wire::array::{
    ArrayElement, ArrayGeometry, MAX_ARRAY_EXTENT_M, MIN_ARRAY_LANES, Winding,
};
use zgui::{
    canvas::{Brush, CanvasScene, ShapeBuilder, zgui_color::Color},
    elements::{kurbo, kurbo::Shape as _},
    prelude::*,
    vocab::SharedString,
};

use super::{
    Array,
    geometry::{
        Axis, MIN_SIZE_M, PreviewPoint, Shape, aliasing, convert_geometry, moved_element,
        overlapping_rows, padded_positions, preview_points, unambiguous_hz,
    },
    status::{ALIASING, ALIASING_TITLE},
};
use crate::ui::{
    kit_raster::{Bounds, number},
    kit_sources::{Tone, button, units},
    widgets::{row_field, segments},
};

pub const NOT_WIRED: &str = "Not wired";
pub const SAME_PLACE: &str = "Same place as another lane";
const PREVIEW_PX: f64 = 96.0;
const DOT_PX: f64 = 5.0;

fn titled(title: &'static str, body: impl IntoView + 'static) -> AnyView {
    AnyView::new(view! { box(a11y:description = SharedString::from(title)) {{body}} })
}

fn set_geometry(cx: &Array, geometry: ArrayGeometry) {
    cx.edit(|settings| settings.geometry = geometry);
}

fn circle_rows(cx: &Array) -> AnyView {
    let read = |pick: fn(f64, f64, Winding) -> f64| {
        cx.read(move |settings| match settings.geometry {
            ArrayGeometry::Uca {
                radius_m,
                first_deg,
                winding,
            } => pick(radius_m, first_deg, winding),
            _ => 0.0,
        })
    };
    let radius = read(|radius, _, _| radius);
    let first = read(|_, first, _| first);
    let winding = cx.read(|settings| match settings.geometry {
        ArrayGeometry::Uca { winding, .. } => winding,
        _ => Winding::Clockwise,
    });
    let edit_circle = |cx: &Array, change: fn(&mut f64, &mut f64, &mut Winding, f64)| {
        let cx = cx.clone();
        move |value: f64| {
            cx.edit(|settings| {
                if let ArrayGeometry::Uca {
                    radius_m,
                    first_deg,
                    winding,
                } = &mut settings.geometry
                {
                    change(radius_m, first_deg, winding, value);
                }
            });
        }
    };
    let on_radius = edit_circle(cx, |radius, _, _, value| *radius = value);
    let on_first = edit_circle(cx, |_, first, _, value| *first = value);
    let on_winding = {
        let cx = cx.clone();
        move |turn: Winding| {
            cx.edit(|settings| {
                if let ArrayGeometry::Uca { winding, .. } = &mut settings.geometry {
                    *winding = turn;
                }
            });
        }
    };
    let windings = vec![
        (Winding::Clockwise, "CW"),
        (Winding::CounterClockwise, "CCW"),
    ];
    AnyView::new(view! {
        column {
            {row_field("Radius m", number("Radius", radius, Bounds::new(MIN_SIZE_M, MAX_ARRAY_EXTENT_M), on_radius))}
            {titled("Where lane 1 sits, clockwise from forward", row_field("First °", number("First element", first, Bounds::new(-360.0, 360.0), on_first)))}
            {row_field("Winding", segments(windings, winding, on_winding))}
        }
    })
}

fn line_rows(cx: &Array) -> AnyView {
    let spacing = cx.read(|settings| match settings.geometry {
        ArrayGeometry::Ula { spacing_m, .. } => spacing_m,
        _ => 0.0,
    });
    let axis = cx.read(|settings| match settings.geometry {
        ArrayGeometry::Ula { axis_deg, .. } => axis_deg,
        _ => 0.0,
    });
    let (on_spacing, on_axis) = (cx.clone(), cx.clone());
    AnyView::new(view! {
        column {
            {row_field("Spacing m", number("Spacing", spacing, Bounds::new(MIN_SIZE_M, MAX_ARRAY_EXTENT_M), move |value| {
                on_spacing.edit(|settings| {
                    if let ArrayGeometry::Ula { spacing_m, .. } = &mut settings.geometry {
                        *spacing_m = value;
                    }
                });
            }))}
            {titled("Lane 1 to last lane, clockwise from forward", row_field("Axis °", number("Axis", axis, Bounds::new(0.0, 359.9), move |value| {
                on_axis.edit(|settings| {
                    if let ArrayGeometry::Ula { axis_deg, .. } = &mut settings.geometry {
                        *axis_deg = value;
                    }
                });
            })))}
        }
    })
}

fn limit_row(cx: &Array) -> AnyView {
    let (settings, wired, status) = (cx.settings, cx.wired, cx.status);
    let limit = Signal::derive(move || {
        settings
            .with(|settings| unambiguous_hz(&settings.geometry, wired.get()))
            .map_or_else(|| "-".to_owned(), units::mhz)
    });
    let aliased = Signal::derive(move || {
        let center = status.with(|status| status.as_ref().map(|found| found.center_hz));
        settings.with(|settings| aliasing(&settings.geometry, wired.get(), center))
    });
    titled(
        "Highest frequency without aliasing",
        row_field(
            "Max f",
            view! {
                row(class = "field__body") {
                    text(class = "kit-mono") {{move || limit.get()}}
                    text(class = "kit-alert", hidden = move || !aliased.get(), a11y:description = SharedString::from(ALIASING_TITLE)) {{ALIASING}}
                }
            },
        ),
    )
}

fn position_cell(cx: &Array, row: usize, axis: Axis, value: f64) -> AnyView {
    let (cx, wired) = (cx.clone(), cx.wired);
    let label = format!("Lane {} {}", row + 1, axis.label());
    AnyView::new(number(
        &label,
        Signal::stored(value),
        Bounds::new(-MAX_ARRAY_EXTENT_M, MAX_ARRAY_EXTENT_M),
        move |moved| {
            let lanes = wired.get_untracked();
            cx.edit(|settings| {
                if let ArrayGeometry::Explicit { positions } = &mut settings.geometry {
                    *positions =
                        moved_element(&padded_positions(positions, lanes), row, axis, moved);
                }
            });
        },
    ))
}

fn position_row(
    cx: &Array,
    row: usize,
    element: ArrayElement,
    unwired: bool,
    overlap: bool,
) -> AnyView {
    let title = if unwired {
        NOT_WIRED
    } else if overlap {
        SAME_PLACE
    } else {
        ""
    };
    let cells: Vec<AnyView> = Axis::ALL
        .iter()
        .map(|axis| position_cell(cx, row, *axis, axis.of(&element)))
        .collect();
    AnyView::new(view! {
        row(class = "af__pos", class:af__unwired = unwired, class:danger = overlap, a11y:description = SharedString::from(title)) {
            text(class = "af__pos_lane") {{(row + 1).to_string()}}
            {cells}
        }
    })
}

fn custom_rows(cx: &Array) -> AnyView {
    let (settings, wired) = (cx.settings, cx.wired);
    let stored = Memo::new(move |_| {
        settings.with(|settings| match &settings.geometry {
            ArrayGeometry::Explicit { positions } => positions.clone(),
            _ => Vec::new(),
        })
    });
    let cx = cx.clone();
    let body = move || {
        let lanes = wired.get();
        let positions = stored.get();
        let overlapping = overlapping_rows(&positions, lanes);
        let rows: Vec<AnyView> = padded_positions(&positions, lanes)
            .into_iter()
            .enumerate()
            .map(|(row, element)| {
                position_row(&cx, row, element, row >= lanes, overlapping.contains(&row))
            })
            .collect();
        let trim = (positions.len() > lanes && lanes >= MIN_ARRAY_LANES as usize).then(|| {
            let cx = cx.clone();
            let kept: Vec<ArrayElement> = positions.iter().take(lanes).copied().collect();
            titled(
                "Drop rows with no lane wired",
                button(
                    || "Trim".to_owned(),
                    Tone::Quiet,
                    Signal::stored(false),
                    move || {
                        set_geometry(
                            &cx,
                            ArrayGeometry::Explicit {
                                positions: kept.clone(),
                            },
                        );
                    },
                ),
            )
        });
        let heads: Vec<AnyView> = Axis::ALL
            .iter()
            .map(|axis| {
                AnyView::new(
                    view! { text(class = "af__pos_head") {{format!("{} m", axis.label())}} },
                )
            })
            .collect();
        view! {
            column(class = "af__positions", a11y:label = "Element positions") {
                row(class = "af__pos") {
                    text(class = "af__pos_lane af__pos_head") {"Lane"}
                    {heads}
                }
                {rows}
                {trim}
            }
        }
    };
    AnyView::new(body)
}

fn polar(bearing_deg: f64, radius: f64, centre: f64) -> (f64, f64) {
    let angle = bearing_deg.to_radians();
    (centre + radius * angle.sin(), centre - radius * angle.cos())
}

fn ink(alpha: f32) -> Brush {
    Brush::Solid(Color::srgb(0.55, 0.57, 0.62, alpha))
}

fn lane_tint(alpha: f32) -> Brush {
    Brush::Solid(Color::srgb(0.42, 0.78, 0.62, alpha))
}

fn accent(alpha: f32) -> Brush {
    Brush::Solid(Color::srgb(0.48, 0.66, 1.0, alpha))
}

fn draw_frame(scene: &mut CanvasScene, scale: f64, azimuth: Option<f64>) {
    let at = |(x, y): (f64, f64)| kurbo::Point::new(x * scale, y * scale);
    let centre = PREVIEW_PX / 2.0;
    let mut forward = kurbo::BezPath::new();
    forward.move_to(at((centre, 1.0)));
    forward.line_to(at((centre - 3.0, 6.0)));
    forward.line_to(at((centre + 3.0, 6.0)));
    forward.close_path();
    scene.push(ShapeBuilder::new(forward).fill(ink(1.0)).build());
    if let Some(azimuth) = azimuth {
        let mut north = kurbo::BezPath::new();
        north.move_to(at((centre, centre)));
        north.line_to(at(polar(-azimuth, centre - 8.0, centre)));
        scene.push(ShapeBuilder::new(north).stroke(accent(0.7), scale).build());
    }
}

fn draw_points(scene: &mut CanvasScene, scale: f64, points: &[PreviewPoint]) {
    for point in points {
        let dot = kurbo::Circle::new(
            kurbo::Point::new(point.x * scale, point.y * scale),
            DOT_PX * scale,
        )
        .to_path(0.1);
        scene.push(ShapeBuilder::new(dot.clone()).fill(lane_tint(0.3)).build());
        scene.push(ShapeBuilder::new(dot).stroke(lane_tint(1.0), scale).build());
    }
}

fn preview(cx: &Array) -> impl IntoView {
    let (settings, wired, status) = (cx.settings, cx.wired, cx.status);
    let points = Memo::new(move |_| {
        settings.with(|settings| preview_points(&settings.geometry, wired.get(), PREVIEW_PX))
    });
    let azimuth = Memo::new(move |_| {
        status.with(|status| status.as_ref().and_then(|found| found.azimuth_deg))
    });
    let canvas = zgui::elements::canvas()
        .class("af__preview_draw")
        .draw(move |cx| {
            let scale = f64::from(cx.size.width.0) / PREVIEW_PX;
            if scale <= 0.0 {
                return;
            }
            draw_frame(cx.scene, scale, azimuth.get());
            points.with(|points| draw_points(cx.scene, scale, points));
        });
    let numbers = move || {
        points
            .get()
            .into_iter()
            .map(|point| {
                AnyView::new(view! {
                    text(
                        class = "af__preview_lane",
                        style:left = Some(format!("{:.1}px", point.x - DOT_PX)),
                        style:top = Some(format!("{:.1}px", point.y - DOT_PX))
                    ) {
                        {point.lane.to_string()}
                    }
                })
            })
            .collect::<Vec<_>>()
    };
    let north = move || {
        azimuth.get().map(|azimuth| {
            let (x, y) = polar(-azimuth, PREVIEW_PX / 2.0 - 8.0, PREVIEW_PX / 2.0);
            view! {
                text(class = "af__preview_north", style:left = Some(format!("{:.1}px", x - 4.0)), style:top = Some(format!("{:.1}px", y - 5.0))) {"N"}
            }
        })
    };
    view! {
        box(class = "af__preview", a11y:label = "Array layout") {
            {canvas.into_view()}
            {numbers}
            {north}
        }
    }
}

pub fn editor(cx: &Array) -> AnyView {
    let shape = Memo::new({
        let settings = cx.settings;
        move |_| settings.with(|settings| Shape::of(&settings.geometry))
    });
    let pick_shape = {
        let cx = cx.clone();
        move |to: Shape| {
            let lanes = cx.wired.get_untracked();
            let next = cx
                .settings
                .with_untracked(|settings| convert_geometry(to, &settings.geometry, lanes));
            set_geometry(&cx, next);
        }
    };
    let rows = {
        let cx = cx.clone();
        move || match shape.get() {
            Shape::Circle => Some(circle_rows(&cx)),
            Shape::Line => Some(line_rows(&cx)),
            Shape::Custom => None,
        }
    };
    let custom = {
        let cx = cx.clone();
        move || (shape.get() == Shape::Custom).then(|| custom_rows(&cx))
    };
    AnyView::new(view! {
        column(class = "af__geo") {
            row(class = "af__geo_top") {
                column(class = "params af__geo_rows") {
                    {titled("UCA: circle. ULA: line. Custom: your own positions.", row_field("Shape", segments(Shape::ALL.to_vec(), shape.into(), pick_shape)))}
                    {rows}
                    {limit_row(cx)}
                }
                {preview(cx)}
            }
            {custom}
        }
    })
}
