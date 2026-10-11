use std::time::Duration;

use sdrmm_wire::processor::{
    ProcessorReading,
    beamformer::{BeamformerParams, BeamformerReading},
};
use zgui::{
    canvas::{Brush, CanvasScene, ShapeBuilder, zgui_color::Color},
    elements::{kurbo, kurbo::Shape as _},
    prelude::*,
    vocab::SharedString,
};

use crate::{
    store::Store,
    ui::{
        faces::processor::{Processor, age_label, error, footer, head, settings},
        kit_raster::readout,
    },
};

mod controls;
pub mod logic;

use logic::{
    amplitude_percent, beam_chips, gain_label, mode_label, null_close, out_text, pattern_points,
    polar_tick, snr_label, steer_label, weight_title,
};

const SIZE: f64 = 120.0;
const CENTRE: f64 = SIZE / 2.0;
const RADIUS: f64 = CENTRE - 6.0;

fn reading_of(reading: &ProcessorReading) -> Option<&BeamformerReading> {
    match reading {
        ProcessorReading::Beamformer(found) => Some(found),
        _ => None,
    }
}

#[must_use]
pub fn rows(
    reading: Option<&BeamformerReading>,
    params: Option<&BeamformerParams>,
    age: Option<Duration>,
) -> Vec<(String, String)> {
    let mode = reading
        .map(|found| found.mode)
        .or_else(|| params.map(|found| found.mode))
        .map_or("-", mode_label);
    let steer = match (reading, params) {
        (Some(found), Some(params)) => steer_label(found, params.steer),
        _ => "-".to_owned(),
    };
    let mut rows = vec![
        ("Mode", mode.to_owned()),
        ("SNR", snr_label(reading.and_then(|found| found.snr_db))),
        ("Gain", gain_label(reading)),
        ("Steer", steer),
    ];
    if let Some(cut) = reading.and_then(|found| found.cancelled_db) {
        rows.push(("Cut", format!("{cut:.0} dB")));
    }
    rows.extend([
        (
            "Level",
            reading.map_or_else(
                || "-".to_owned(),
                |found| format!("{:.1} dB", found.output_db),
            ),
        ),
        ("Age", age_label(age)),
        ("Out", out_text(reading)),
    ]);
    rows.into_iter()
        .map(|(label, value)| (label.to_owned(), value))
        .collect()
}

pub fn face(store: Store, node: String) -> impl IntoView {
    let titled = node.clone();
    install_stylesheet("beamformer-face", SHEET);
    let processor = Processor::new(store, node.clone());
    let params = settings(store, node.clone(), controls::body);
    let reading = processor.reading(reading_of);
    let period = Signal::derive(move || {
        params.with(|found| found.as_ref().map_or(0, |found| found.update_ms))
    });
    let stale = processor.stale(period);
    let age = processor.age();
    let lanes = {
        let wired = processor.lanes();
        Signal::derive(move || {
            let weights =
                reading.with(|found| found.as_ref().map_or(0, |found| found.weights.len()));
            wired.get().max(weights)
        })
    };
    let wired = {
        let node = node.clone();
        Memo::new(move |_| {
            store
                .graph
                .with(|graph| graph.steer_source(&node).is_some())
        })
    };
    let shown = move || rows(reading.get().as_ref(), params.get().as_ref(), age.get());
    let beam = controls::Beam {
        store,
        node,
        params,
        lanes,
        wired,
    };
    let health = processor.health;
    head(store, titled, processor.subtitle(period));
    view! {
        column(class = "face") {
            row(class = "bf__top", class:stale = move || stale.get()) {
                box(class = "bf__pattern", a11y:label = "Beam pattern") {
                    {pattern(reading)}
                    text(class = "bf__fwd") {"Fwd"}
                }
                column(class = "bf__side") {
                    box(class = "bf__read") {{readout(shown)}}
                    {weights(reading)}
                }
            }
            {controls::view(beam)}
            {error(health)}
            {footer(health, move || beam_chips(reading.get().as_ref()))}
        }
    }
}

fn weights(reading: Memo<Option<BeamformerReading>>) -> impl IntoView {
    move || {
        let weights = reading.with(|found| {
            found
                .as_ref()
                .map(|found| found.weights.clone())
                .unwrap_or_default()
        });
        (!weights.is_empty()).then(|| {
            let percent = amplitude_percent(&weights);
            let bars = weights
                .iter()
                .zip(percent)
                .enumerate()
                .map(|(lane, (weight, width))| {
                    let title = SharedString::from(weight_title(lane, *weight));
                    view! {
                        row(class = "bf__weight", a11y:description = title) {
                            text(class = "bf__lane") {{(lane + 1).to_string()}}
                            box(class = "bf__bar") {
                                box(class = "bf__fill", style:width = Some(format!("{width}%")))
                            }
                        }
                    }
                })
                .collect::<Vec<_>>();
            view! { column(class = "bf__weights", a11y:label = "Lane weights") {{bars}} }
        })
    }
}

fn ink(alpha: f32) -> Brush {
    Brush::Solid(Color::srgb(0.33, 0.35, 0.39, alpha))
}

fn accent(alpha: f32) -> Brush {
    Brush::Solid(Color::srgb(0.48, 0.66, 1.0, alpha))
}

fn danger() -> Brush {
    Brush::Solid(Color::srgb(0.93, 0.42, 0.38, 1.0))
}

fn warn() -> Brush {
    Brush::Solid(Color::srgb(0.86, 0.72, 0.3, 1.0))
}

fn pattern(reading: Memo<Option<BeamformerReading>>) -> impl IntoView {
    zgui::elements::canvas()
        .class("bf__plot")
        .draw(move |cx| {
            let scale = f64::from(cx.size.width.0) / SIZE;
            if scale <= 0.0 {
                return;
            }
            let at = |(x, y): (f64, f64)| kurbo::Point::new(x * scale, y * scale);
            rings(cx.scene, scale, at);
            reading.with(|found| {
                if let Some(found) = found {
                    lobe(cx.scene, &found.pattern, at);
                    ticks(cx.scene, found, scale, at);
                }
            });
        })
        .into_view()
}

fn rings(scene: &mut CanvasScene, scale: f64, at: impl Fn((f64, f64)) -> kurbo::Point) {
    let centre = at((CENTRE, CENTRE));
    let well = kurbo::Circle::new(centre, RADIUS * scale).to_path(0.1);
    scene.push(ShapeBuilder::new(well.clone()).fill(ink(0.15)).build());
    scene.push(ShapeBuilder::new(well).stroke(ink(1.0), 1.0).build());
    let half = kurbo::Circle::new(centre, RADIUS / 2.0 * scale).to_path(0.1);
    scene.push(ShapeBuilder::new(half).stroke(ink(0.6), 1.0).build());
}

fn lobe(scene: &mut CanvasScene, pattern: &[u8], at: impl Fn((f64, f64)) -> kurbo::Point) {
    let points = pattern_points(pattern, RADIUS, CENTRE);
    let Some(first) = points.first() else {
        return;
    };
    let mut outline = kurbo::BezPath::new();
    outline.move_to(at(*first));
    for point in &points[1..] {
        outline.line_to(at(*point));
    }
    outline.close_path();
    scene.push(ShapeBuilder::new(outline.clone()).fill(accent(0.2)).build());
    scene.push(ShapeBuilder::new(outline).stroke(accent(1.0), 1.0).build());
}

fn tick(
    scene: &mut CanvasScene,
    ends: [(f64, f64); 2],
    brush: Brush,
    width: f64,
    at: impl Fn((f64, f64)) -> kurbo::Point,
) {
    let mut line = kurbo::BezPath::new();
    line.move_to(at(ends[0]));
    line.line_to(at(ends[1]));
    scene.push(ShapeBuilder::new(line).stroke(brush, width).build());
}

fn ticks(
    scene: &mut CanvasScene,
    reading: &BeamformerReading,
    scale: f64,
    at: impl Fn((f64, f64)) -> kurbo::Point + Copy,
) {
    if let Some(steer) = reading.steer_deg {
        let ends = polar_tick(f64::from(steer), RADIUS - 8.0, RADIUS + 4.0, CENTRE);
        tick(scene, ends, accent(1.0), 2.0 * scale, at);
    }
    for deg in &reading.nulls_deg {
        let deg = f64::from(*deg);
        let brush = if null_close(deg, reading.steer_deg) {
            warn()
        } else {
            danger()
        };
        let ends = polar_tick(deg, RADIUS - 6.0, RADIUS + 4.0, CENTRE);
        tick(scene, ends, brush, 1.5 * scale, at);
    }
}

const SHEET: &str = css!(
    r#"
.bf__top { gap: 8px; align-items: flex-start; }
.bf__top.stale { opacity: 0.5; }
.bf__pattern { position: relative; flex: 0 0 auto; width: 120px; height: 120px; }
.bf__plot { position: absolute; left: 0; top: 0; width: 120px; height: 120px; }
.bf__fwd {
    position: absolute;
    left: 0;
    top: 0;
    width: 120px;
    text-align: center;
    font-family: var(--mono);
    font-size: 8px;
    color: var(--ink-faint);
    pointer-events: none;
}
.bf__side { flex: 1 1 auto; min-width: 0; gap: 8px; }
.bf__read .rdo__label { width: 40px; }
.bf__weights { gap: 2px; }
.bf__weight { gap: 6px; align-items: center; }
.bf__lane { width: 12px; font-family: var(--mono); font-size: 10px; color: var(--accent); }
.bf__bar { flex: 1 1 auto; height: 6px; border-radius: 999px; background-color: var(--line); overflow: hidden; }
.bf__fill { height: 6px; border-radius: 999px; background-color: var(--accent); }
.bf__group { gap: 6px; }
.bf__null { gap: 4px; align-items: center; }
.bf__ref { gap: 4px; align-items: center; }
"#
);

#[cfg(test)]
mod tests {
    use sdrmm_wire::processor::beamformer::{BeamMode, SteerSource};

    use super::*;

    fn labels(rows: &[(String, String)]) -> Vec<&str> {
        rows.iter().map(|(label, _)| label.as_str()).collect()
    }

    #[test]
    fn without_a_reading_the_readouts_fall_back_to_the_settings() {
        let params = BeamformerParams {
            mode: BeamMode::Mvdr,
            ..BeamformerParams::default()
        };
        let shown = rows(None, Some(&params), None);
        assert_eq!(
            labels(&shown),
            ["Mode", "SNR", "Gain", "Steer", "Level", "Age", "Out"]
        );
        assert_eq!(shown[0].1, "MVDR");
        assert!(shown[1..].iter().all(|(_, value)| value == "-"));
    }

    #[test]
    fn a_canceller_reading_adds_the_cut() {
        let reading = BeamformerReading {
            mode: BeamMode::Canceller,
            cancelled_db: Some(18.4),
            output_db: -40.26,
            steer_deg: Some(12.0),
            ..BeamformerReading::default()
        };
        let params = BeamformerParams {
            steer: SteerSource::Fixed {
                azimuth_deg: 12.0,
                elevation_deg: 0.0,
            },
            ..BeamformerParams::default()
        };
        let shown = rows(
            Some(&reading),
            Some(&params),
            Some(Duration::from_millis(1_500)),
        );
        assert_eq!(
            labels(&shown),
            ["Mode", "SNR", "Gain", "Steer", "Cut", "Level", "Age", "Out"]
        );
        assert_eq!(shown[0].1, "Cancel");
        assert_eq!(shown[3].1, "12° fixed");
        assert_eq!(shown[4].1, "18 dB");
        assert_eq!(shown[5].1, "-40.3 dB");
        assert_eq!(shown[6].1, "1.5 s");
    }
}
