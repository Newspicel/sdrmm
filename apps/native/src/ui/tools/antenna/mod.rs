pub mod model;
pub mod view;

use sdrmm_wire::tools::{
    AntennaDesign, AntennaPart, AntennaReport, AntennaRequest, GroundPlaneParams, InvertedVParams,
    ToolResponse, YagiParams,
};
use zgui::prelude::*;

use self::model::{
    DESIGNS, ISOMETRIC, Mode, Unit, antenna_report, antenna_request, default_design,
    format_impedance, format_length, uses_feedline,
};
use crate::{
    store::Store,
    ui::{
        faces::channel::settings::NumberLimit,
        tools::kit::{Query, alert, chip, format_hz, labelled, number, run_tool},
        widgets::{pick, segments},
    },
};

const FREQUENCY: NumberLimit = NumberLimit::new(0.01, 300_000.0, 0.005);
const ELEMENT_FACTOR: NumberLimit = NumberLimit::new(0.5, 1.0, 0.01);
const COAX_FACTOR: NumberLimit = NumberLimit::new(0.4, 1.0, 0.01);
const APEX: NumberLimit = NumberLimit::new(60.0, 180.0, 5.0);
const RADIALS: NumberLimit = NumberLimit::new(1.0, 32.0, 1.0);
const SLOPE: NumberLimit = NumberLimit::new(0.0, 60.0, 5.0);
const DIRECTORS: NumberLimit = NumberLimit::new(0.0, 20.0, 1.0);
const SPACING: NumberLimit = NumberLimit::new(0.1, 0.4, 0.01);

#[derive(Clone, Copy)]
struct Choices {
    kind: RwSignal<&'static str>,
    apex: RwSignal<f64>,
    radials: RwSignal<f64>,
    slope: RwSignal<f64>,
    directors: RwSignal<f64>,
    spacing: RwSignal<f64>,
}

impl Choices {
    fn new() -> Self {
        Self {
            kind: RwSignal::new("dipole"),
            apex: RwSignal::new(120.0),
            radials: RwSignal::new(4.0),
            slope: RwSignal::new(45.0),
            directors: RwSignal::new(2.0),
            spacing: RwSignal::new(0.2),
        }
    }

    fn pick(self, kind: &'static str) {
        self.kind.set(kind);
        match default_design(kind) {
            AntennaDesign::InvertedV(params) => self.apex.set(params.apex_angle_deg),
            AntennaDesign::GroundPlane(params) => {
                self.radials.set(f64::from(params.radials));
                self.slope.set(params.radial_slope_deg);
            }
            AntennaDesign::Yagi(params) => {
                self.directors.set(f64::from(params.directors));
                self.spacing.set(params.spacing_wavelengths);
            }
            _ => {}
        }
    }

    fn design(self) -> AntennaDesign {
        match default_design(self.kind.get()) {
            AntennaDesign::InvertedV(_) => AntennaDesign::InvertedV(InvertedVParams {
                apex_angle_deg: self.apex.get(),
            }),
            AntennaDesign::GroundPlane(_) => AntennaDesign::GroundPlane(GroundPlaneParams {
                radials: self.radials.get().round() as u8,
                radial_slope_deg: self.slope.get(),
            }),
            AntennaDesign::Yagi(_) => AntennaDesign::Yagi(YagiParams {
                directors: self.directors.get().round() as u8,
                spacing_wavelengths: self.spacing.get(),
            }),
            other => other,
        }
    }
}

pub fn panel(store: Store) -> impl IntoView {
    let frequency_mhz = RwSignal::new(145.5);
    let velocity = RwSignal::new(0.95);
    let feedline = RwSignal::new(0.66);
    let unit = RwSignal::new(Unit::Metres);
    let choices = Choices::new();
    let run: Query<ToolResponse> = Query::new();
    let request = Memo::new(move |_| {
        antenna_request(AntennaRequest {
            frequency_hz: frequency_mhz.get() * 1e6,
            velocity_factor: velocity.get(),
            feedline_velocity_factor: feedline.get(),
            design: choices.design(),
        })
    });
    let ask = zgui::reactive::RenderEffect::new(move |_| {
        let request = request.get();
        run.run(run_tool(store, request));
    });
    on_cleanup_local(move || drop(ask));
    let report = Memo::new(move |_| run.data.with(|data| antenna_report(data.as_ref()).cloned()));
    let ready = Memo::new(move |_| report.with(Option::is_some));

    view! {
        column(class = "tool-stack") {
            {controls(choices, frequency_mhz, velocity, feedline, unit)}
            {alert(move || run.error.get())}
            {move || ready.get().then(|| AnyView::new(report_view(report, unit)))}
        }
    }
}

fn controls(
    choices: Choices,
    frequency_mhz: RwSignal<f64>,
    velocity: RwSignal<f64>,
    feedline: RwSignal<f64>,
    unit: RwSignal<Unit>,
) -> impl IntoView {
    let designs: Vec<(&'static str, String)> = DESIGNS
        .iter()
        .map(|(design, label)| (design.type_id(), (*label).to_owned()))
        .collect();
    let kind = choices.kind;
    view! {
        row(class = "tool-row") {
            {labelled("Frequency", number("Frequency", frequency_mhz, FREQUENCY, "MHz"))}
            {labelled("Design", pick(designs, Signal::derive(move || Some(kind.get())), move |picked| choices.pick(picked)))}
            {labelled("Element factor", number("Element velocity factor", velocity, ELEMENT_FACTOR, ""))}
            {move || uses_feedline(&default_design(kind.get())).then(|| AnyView::new(labelled("Coax factor", number("Feedline velocity factor", feedline, COAX_FACTOR, ""))))}
            {move || design_settings(choices)}
            {labelled("Units", segments(vec![(Unit::Metres, "m"), (Unit::Feet, "ft")], unit.into(), move |picked| unit.set(picked)))}
        }
    }
}

fn design_settings(choices: Choices) -> Vec<AnyView> {
    match choices.kind.get() {
        "inverted_v" => vec![AnyView::new(labelled(
            "Apex angle",
            number("Apex angle", choices.apex, APEX, "\u{b0}"),
        ))],
        "ground_plane" => vec![
            AnyView::new(labelled(
                "Radials",
                number("Radial count", choices.radials, RADIALS, ""),
            )),
            AnyView::new(labelled(
                "Radial slope",
                number("Radial slope", choices.slope, SLOPE, "\u{b0}"),
            )),
        ],
        "yagi" => vec![
            AnyView::new(labelled(
                "Directors",
                number("Director count", choices.directors, DIRECTORS, ""),
            )),
            AnyView::new(labelled(
                "Spacing",
                number(
                    "Element spacing in wavelengths",
                    choices.spacing,
                    SPACING,
                    "\u{3bb}",
                ),
            )),
        ],
        _ => Vec::new(),
    }
}

fn report_view(report: Memo<Option<AntennaReport>>, unit: RwSignal<Unit>) -> impl IntoView {
    let highlight = RwSignal::new(None::<String>);
    let mode = RwSignal::new(Mode::Plan);
    let orbit = RwSignal::new(ISOMETRIC);
    let drawing = view::antenna_view(view::Inputs {
        report,
        unit,
        highlight,
        mode,
        orbit,
    });
    let chips = move || {
        report.with(|report| {
            report.as_ref().map_or_else(Vec::new, |report| {
                vec![
                    chip("\u{3bb}", format_length(report.wavelength_m, unit.get())),
                    chip("at", format_hz(report.frequency_hz)),
                    chip("feedpoint", format_impedance(report.feedpoint_ohms)),
                    chip(
                        "",
                        if report.balanced {
                            "Balanced: wants a balun"
                        } else {
                            "Unbalanced"
                        },
                    ),
                ]
            })
        })
    };
    view! {
        column(class = "tool-stack") {
            row(class = "tool-chips") {{chips}}
            {drawing}
            {parts(report, unit, highlight)}
            {notes(report)}
        }
    }
}

fn parts(
    report: Memo<Option<AntennaReport>>,
    unit: RwSignal<Unit>,
    highlight: RwSignal<Option<String>>,
) -> impl IntoView {
    move || {
        let rows: Vec<AntennaPart> = report.with(|report| {
            report
                .as_ref()
                .map(|report| report.parts.clone())
                .unwrap_or_default()
        });
        let unit = unit.get();
        let mut cells: Vec<AnyView> = ["Part", "Qty", "Length", "On the boom"]
            .into_iter()
            .map(|head| AnyView::new(view! { text(class = "tool-th") {{head}} }))
            .collect();
        for part in rows {
            cells.extend(part_cells(part, unit, highlight));
        }
        view! { box(class = "tool-table ant-parts") {{cells}} }
    }
}

fn part_cells(part: AntennaPart, unit: Unit, highlight: RwSignal<Option<String>>) -> Vec<AnyView> {
    let name = part.name.clone();
    let cell = move |content: AnyView, class: &'static str| {
        let lit = name.clone();
        let enter = name.clone();
        AnyView::new(view! {
            box(
                class = class,
                class:lit = move || highlight.get().as_deref() == Some(lit.as_str()),
                on:pointer_enter = move |_| highlight.set(Some(enter.clone())),
                on:pointer_leave = move |_| highlight.set(None)
            ) {{content}}
        })
    };
    let detail = part.detail.clone();
    let title = part.name.clone();
    vec![
        cell(
            AnyView::new(view! {
                column {
                    text(class = "tool-ink") {{title}}
                    {detail.map(|detail| view! { text(class = "tool-dim tool-sans") {{detail}} })}
                }
            }),
            "tool-td",
        ),
        cell(
            AnyView::new(if part.count > 1 {
                format!("\u{d7} {}", part.count)
            } else {
                String::new()
            }),
            "tool-td tool-dim",
        ),
        cell(
            AnyView::new(format_length(part.length_m, unit)),
            "tool-td tool-accent",
        ),
        cell(
            AnyView::new(
                part.position_m
                    .map(|at| format_length(at, unit))
                    .unwrap_or_default(),
            ),
            "tool-td tool-dim",
        ),
    ]
}

fn notes(report: Memo<Option<AntennaReport>>) -> impl IntoView {
    move || {
        let notes = report.with(|report| {
            report
                .as_ref()
                .map(|report| report.notes.clone())
                .unwrap_or_default()
        });
        let items: Vec<AnyView> = notes
            .into_iter()
            .map(|note| {
                AnyView::new(view! {
                    row(class = "tool-note") {
                        text(class = "tool-faint") {"\u{b7}"}
                        text {{note}}
                    }
                })
            })
            .collect();
        view! { column(class = "tool-notes") {{items}} }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit_channel::{number_text, step_value};

    #[test]
    fn the_frequency_steps_by_five_kilohertz_and_shows_no_float_noise() {
        assert_eq!(step_value(145.5, 0.005, 1.0, FREQUENCY), 145.505);
        assert_eq!(number_text(Some(145.5), FREQUENCY.step), "145.5");
        assert_eq!(step_value(0.01, 0.005, -1.0, FREQUENCY), 0.01);
    }

    #[test]
    fn counts_and_angles_step_in_whole_units_inside_their_limits() {
        assert_eq!(step_value(32.0, 1.0, 1.0, RADIALS), 32.0);
        assert_eq!(step_value(120.0, 5.0, 1.0, APEX), 125.0);
        assert_eq!(number_text(Some(2.0), DIRECTORS.step), "2");
        assert_eq!(step_value(0.4, 0.01, 1.0, SPACING), 0.4);
        assert_eq!(step_value(0.95, 0.01, -1.0, ELEMENT_FACTOR), 0.94);
        assert_eq!(step_value(0.4, 0.01, -1.0, COAX_FACTOR), 0.4);
        assert_eq!(step_value(45.0, 50.0, 1.0, SLOPE), 60.0);
    }
}
