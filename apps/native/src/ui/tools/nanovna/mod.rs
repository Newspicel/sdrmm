pub mod analysis;
pub mod calibration;
pub mod chart;
pub mod export;
pub mod readout;
pub mod rf;
#[cfg(test)]
mod testdata;
pub mod traces;

use sdrmm_wire::tools::{
    NanoVnaCalibration, NanoVnaDevice, NanoVnaDeviceReport, NanoVnaMatch, NanoVnaSweep,
    NanoVnaSweepRequest, NanoVnaSweepState, ToolResponse,
};
use zgui::prelude::*;

use self::{
    analysis::{PointReadout, SweepAnalysis, analyse, readouts},
    calibration::{Calibrate, correction_chip},
    chart::{SweepInputs, smith_chart, sweep_chart},
    export::{FORMATS, Format, export_filename, sweep_csv, touchstone_s1p, touchstone_s2p},
    readout::{device_report, marker_readout, sweep_summary},
    rf::{
        describe_request, devices_of, devices_request, ignored_ports_of, report_of, sweep_of,
        sweep_request,
    },
    traces::{CHART_VIEWS, ChartId},
};
use crate::{
    store::Store,
    ui::{
        faces::channel::settings::NumberLimit,
        kit_channel::tip_text,
        tools::kit::{
            Query, Tone, alert, button, chip, format_hz, labelled, number, run_tool, save_for,
            text_field,
        },
        widgets::{pick, segments, slide},
    },
};

const SWEEP_MHZ: NumberLimit = NumberLimit::new(0.01, 6300.0, 0.001);
const POINTS: NumberLimit = NumberLimit::new(11.0, 10_001.0, 10.0);
const AVERAGES: NumberLimit = NumberLimit::new(1.0, 16.0, 1.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Measure,
    Calibrate,
    Device,
}

const RANGE_PRESETS: [(&str, f64, f64); 5] = [
    ("HF", 0.05, 30.0),
    ("6 m", 50.0, 54.0),
    ("2 m", 144.0, 148.0),
    ("70 cm", 430.0, 440.0),
    ("Full", 0.05, 900.0),
];

#[must_use]
pub fn sweep_state(start_mhz: f64, stop_mhz: f64, points: f64) -> NanoVnaSweepState {
    NanoVnaSweepState {
        start_hz: (start_mhz * 1e6).round() as u64,
        stop_hz: (stop_mhz * 1e6).round() as u64,
        points: points.round() as u32,
    }
}

#[derive(Clone, Copy)]
struct Vna {
    store: Store,
    port: RwSignal<String, LocalStorage>,
    start: RwSignal<f64>,
    stop: RwSignal<f64>,
    points: RwSignal<f64>,
    averages: RwSignal<f64>,
    devices: Query<ToolResponse>,
    sweep: Query<ToolResponse>,
    describe: Query<ToolResponse>,
    effective: Memo<String>,
    range: Memo<NanoVnaSweepState>,
}

impl Vna {
    fn scan(self) {
        let store = self.store;
        self.devices.run(run_tool(store, devices_request()));
    }

    fn acquire(self) {
        let range = self.range.get_untracked();
        let request = NanoVnaSweepRequest {
            port: self.effective.get_untracked(),
            start_hz: range.start_hz,
            stop_hz: range.stop_hz,
            points: range.points,
            averages: self.averages.get_untracked().round() as u16,
        };
        self.sweep.run(run_tool(self.store, sweep_request(request)));
    }

    fn read_device(self) {
        let port = self.effective.get_untracked();
        if port.is_empty() {
            return;
        }
        self.describe
            .run(run_tool(self.store, describe_request(&port)));
    }
}

pub fn panel(store: Store) -> impl IntoView {
    let port = RwSignal::new_local(String::new());
    let start = RwSignal::new(1.0);
    let stop = RwSignal::new(30.0);
    let points = RwSignal::new(101.0);
    let devices: Query<ToolResponse> = Query::new();
    let effective = Memo::new(move |_| {
        let typed = port.get();
        if typed.is_empty() {
            devices.data.with(|data| {
                devices_of(data.as_ref())
                    .first()
                    .map(|device| device.port.clone())
                    .unwrap_or_default()
            })
        } else {
            typed
        }
    });
    let vna = Vna {
        store,
        port,
        start,
        stop,
        points,
        averages: RwSignal::new(1.0),
        devices,
        sweep: Query::new(),
        describe: Query::new(),
        effective,
        range: Memo::new(move |_| sweep_state(start.get(), stop.get(), points.get())),
    };
    vna.scan();
    let tab = RwSignal::new(Tab::Measure);
    let wants_device = Memo::new(move |_| tab.get() != Tab::Measure);
    let described = zgui::reactive::RenderEffect::new(move |_| {
        if wants_device.get() && !effective.get().is_empty() {
            vna.read_device();
        }
    });
    on_cleanup_local(move || drop(described));
    let sweep = Memo::new(move |_| vna.sweep.data.with(|data| sweep_of(data.as_ref()).cloned()));
    let report = Memo::new(move |_| {
        vna.describe
            .data
            .with(|data| report_of(data.as_ref()).cloned())
            .or_else(|| sweep.with(|sweep| sweep.as_ref().map(|sweep| sweep.device.clone())))
    });
    let measured = RwSignal::new(None::<NanoVnaCalibration>);
    let calibration = Memo::new(move |_| {
        let port = effective.get();
        let known = report.with(|report| report.as_ref().map(|report| report.calibration.clone()));
        measured.with(|measured| shown_calibration(measured.as_ref(), known, &port))
    });
    let view = ChartState::new();

    view! {
        column(class = "tool-stack") {
            {controls(vna, sweep)}
            {presets(vna)}
            {device_bar(vna)}
            {segments(vec![(Tab::Measure, "Measure"), (Tab::Calibrate, "Calibrate"), (Tab::Device, "Device")], tab.into(), move |picked| tab.set(picked))}
            {alert(move || vna.sweep.error.get())}
            {alert(move || if tab.get() == Tab::Measure { None } else { vna.describe.error.get() })}
            {move || match tab.get() {
                Tab::Measure => measure(vna, sweep, view),
                Tab::Calibrate => AnyView::new(calibration::panel(Calibrate { store, port: effective, range: vna.range, state: calibration, measured })),
                Tab::Device => device_tab(vna, report),
            }}
        }
    }
}

#[must_use]
pub fn shown_calibration(
    measured: Option<&NanoVnaCalibration>,
    known: Option<NanoVnaCalibration>,
    port: &str,
) -> Option<NanoVnaCalibration> {
    measured
        .filter(|measured| measured.port == port)
        .cloned()
        .or(known)
}

fn controls(vna: Vna, sweep: Memo<Option<NanoVnaSweep>>) -> impl IntoView {
    let placeholder = move || {
        vna.devices.data.with(|data| {
            devices_of(data.as_ref()).first().map_or_else(
                || "no NanoVNA found".to_owned(),
                |device| device.port.clone(),
            )
        })
    };
    let label = move || {
        if vna.sweep.busy.get() {
            "Sweeping\u{2026}".to_owned()
        } else if sweep.with(Option::is_none) {
            "Sweep".to_owned()
        } else {
            "Sweep again".to_owned()
        }
    };
    let blocked = move || {
        vna.effective.with(String::is_empty)
            || vna.sweep.busy.get()
            || vna.stop.get() <= vna.start.get()
    };
    view! {
        row(class = "tool-row") {
            {labelled("Instrument", view! {
                row(class = "tool-bar") {
                    {move || text_field(vna.port, "NanoVNA serial port", placeholder(), "tool-text wide")}
                    {button("btn", move || if vna.devices.busy.get() { "Scanning\u{2026}".to_owned() } else { "Rescan".to_owned() }, move || vna.devices.busy.get(), move || vna.scan())}
                }
            })}
            {labelled("Start", number("Sweep start", vna.start, SWEEP_MHZ, "MHz"))}
            {labelled("Stop", number("Sweep stop", vna.stop, SWEEP_MHZ, "MHz"))}
            {labelled("Points", number("Sweep points", vna.points, POINTS, ""))}
            {labelled("Averages", number("Sweep averages", vna.averages, AVERAGES, ""))}
            {button("btn primary", label, blocked, move || vna.acquire())}
        }
    }
}

fn presets(vna: Vna) -> impl IntoView {
    let chips: Vec<AnyView> = RANGE_PRESETS
        .iter()
        .map(|(label, start, stop)| {
            let (start, stop) = (*start, *stop);
            AnyView::new(button(
                "tool-chip",
                move || (*label).to_owned(),
                || false,
                move || {
                    vna.start.set(start);
                    vna.stop.set(stop);
                },
            ))
        })
        .collect();
    view! {
        row(class = "tool-bar") {
            text(class = "legend") {"Range"}
            {chips}
        }
    }
}

fn device_bar(vna: Vna) -> impl IntoView {
    move || {
        if let Some(error) = vna.devices.error.get() {
            return AnyView::new(view! { text(class = "tool-alert") {{error}} });
        }
        if vna.devices.busy.get() && vna.devices.data.with(Option::is_none) {
            return AnyView::new(
                view! { text(class = "tool-dim") {"Looking for a NanoVNA\u{2026}"} },
            );
        }
        let (found, ignored) = vna.devices.data.with(|data| {
            (
                devices_of(data.as_ref()).to_vec(),
                ignored_ports_of(data.as_ref()).to_vec(),
            )
        });
        let selected = vna.effective.get();
        let mut items: Vec<AnyView> = if found.is_empty() {
            vec![AnyView::new(
                view! { text(class = "tool-dim") {"No NanoVNA found. Connect one and rescan, or type its port."} },
            )]
        } else {
            found
                .into_iter()
                .map(|device| device_chip(vna, device, &selected))
                .collect()
        };
        if !ignored.is_empty() {
            let count = ignored.len();
            let noun = if count == 1 { "port" } else { "ports" };
            let list = ignored.join("\n");
            items.push(tip_text(
                move || list.clone(),
                AnyView::new(view! { text(class = "tool-faint") {{format!("{count} other serial {noun} ignored")}} }),
            ));
        }
        AnyView::new(view! { row(class = "tool-bar") {{items}} })
    }
}

#[must_use]
pub fn device_title(device: &NanoVnaDevice) -> String {
    [&device.manufacturer, &device.product, &device.serial_number]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

fn device_chip(vna: Vna, device: NanoVnaDevice, selected: &str) -> AnyView {
    let port = device.port.clone();
    let on = device.port == selected;
    let unconfirmed = (device.match_kind == NanoVnaMatch::Probable)
        .then(|| view! { text(class = "tool-faint") {"unconfirmed"} });
    let title = device_title(&device);
    let face = AnyView::new(view! {
        control(
            class = "tool-chip",
            class:on = on,
            a11y:role = Role::Button,
            tabindex = Focus::Sequential,
            on:click:stop = move |_| vna.port.set(port.clone())
        ) {
            text {{device.label.clone()}}
            {unconfirmed}
        }
    });
    if title.is_empty() {
        face
    } else {
        tip_text(move || title.clone(), face)
    }
}

fn device_tab(vna: Vna, report: Memo<Option<NanoVnaDeviceReport>>) -> AnyView {
    AnyView::new(move || {
        let Some(report) = report.get() else {
            let hint = if vna.describe.busy.get() {
                "Reading the instrument\u{2026}"
            } else {
                "No instrument selected."
            };
            return AnyView::new(view! { text(class = "tool-dim") {{hint}} });
        };
        AnyView::new(view! {
            column(class = "tool-stack") {
                row(class = "tool-bar") {
                    {button("btn", move || if vna.describe.busy.get() { "Reading\u{2026}".to_owned() } else { "Re-read the instrument".to_owned() }, move || vna.describe.busy.get(), move || vna.read_device())}
                }
                {device_report(&report)}
            }
        })
    })
}

#[derive(Clone, Copy)]
struct ChartState {
    chart: RwSignal<ChartId>,
    format: RwSignal<Format>,
    zoom: RwSignal<Option<(usize, usize)>>,
    marker: RwSignal<Option<usize>>,
}

impl ChartState {
    fn new() -> Self {
        Self {
            chart: RwSignal::new(ChartId::Magnitude),
            format: RwSignal::new(Format::Ri),
            zoom: RwSignal::new(None),
            marker: RwSignal::new(None),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Waiting,
    Empty,
    Ready,
}

#[must_use]
pub fn shown_of(sweep: Option<&NanoVnaSweep>) -> Shown {
    match sweep {
        None => Shown::Waiting,
        Some(sweep) if sweep.points.is_empty() => Shown::Empty,
        Some(_) => Shown::Ready,
    }
}

fn measure(vna: Vna, sweep: Memo<Option<NanoVnaSweep>>, view: ChartState) -> AnyView {
    let shown = Memo::new(move |_| sweep.with(|sweep| shown_of(sweep.as_ref())));
    AnyView::new(move || match shown.get() {
        Shown::Waiting => {
            let hint = if vna.effective.with(String::is_empty) {
                "Connect a NanoVNA and rescan."
            } else {
                "Sweep to measure S11 and S21 across the range above."
            };
            AnyView::new(view! { text(class = "tool-dim") {{hint}} })
        }
        Shown::Empty => AnyView::new(
            view! { text(class = "tool-alert") {"The NanoVNA returned an empty sweep."} },
        ),
        Shown::Ready => AnyView::new(sweep_view(vna.store, sweep, view)),
    })
}

#[must_use]
pub fn retained_zoom(zoom: Option<(usize, usize)>, count: usize) -> Option<(usize, usize)> {
    zoom.filter(|&(from, to)| from <= to && to < count)
}

#[must_use]
pub fn visible_range(zoom: Option<(usize, usize)>, count: usize) -> (usize, usize) {
    retained_zoom(zoom, count).unwrap_or((0, count.saturating_sub(1)))
}

#[must_use]
pub fn marker_index(
    marker: Option<usize>,
    resonance: Option<usize>,
    zoom: Option<(usize, usize)>,
    count: usize,
) -> usize {
    let offset = zoom.map_or(0, |(from, _)| from);
    let fallback = resonance.unwrap_or(0).saturating_sub(offset);
    marker.unwrap_or(fallback).min(count.saturating_sub(1))
}

#[derive(Clone, Copy)]
struct Sweep {
    zoom: Memo<Option<(usize, usize)>>,
    visible: Memo<Vec<PointReadout>>,
    active: Memo<usize>,
    analysis: Memo<SweepAnalysis>,
}

impl Sweep {
    fn new(sweep: Memo<Option<NanoVnaSweep>>, view: ChartState) -> Self {
        let all = Memo::new(move |_| {
            sweep.with(|sweep| {
                sweep
                    .as_ref()
                    .map(|sweep| readouts(&sweep.points))
                    .unwrap_or_default()
            })
        });
        let analysis = Memo::new(move |_| {
            sweep.with(|sweep| analyse(sweep.as_ref().map_or(&[][..], |sweep| &sweep.points)))
        });
        let zoom = Memo::new(move |_| retained_zoom(view.zoom.get(), all.with(Vec::len)));
        let visible = Memo::new(move |_| {
            let (from, to) = visible_range(zoom.get(), all.with(Vec::len));
            all.with(|rows| rows.get(from..=to).map(<[_]>::to_vec).unwrap_or_default())
        });
        let active = Memo::new(move |_| {
            let resonance =
                analysis.with(|analysis| analysis.resonance.as_ref().map(|row| row.index));
            marker_index(
                view.marker.get(),
                resonance,
                zoom.get(),
                visible.with(Vec::len),
            )
        });
        Self {
            zoom,
            visible,
            active,
            analysis,
        }
    }
}

fn sweep_view(store: Store, sweep: Memo<Option<NanoVnaSweep>>, view: ChartState) -> impl IntoView {
    let shape = Sweep::new(sweep, view);
    let chart = view.chart;
    let on_marker = UnsyncCallback::new(move |index| view.marker.set(Some(index)));
    let on_zoom = UnsyncCallback::new(move |(from, to): (usize, usize)| {
        let base = shape.zoom.get_untracked().map_or(0, |(start, _)| start);
        view.zoom.set(Some((base + from, base + to)));
    });
    let chart_options: Vec<(ChartId, &'static str)> = CHART_VIEWS
        .iter()
        .map(|entry| (entry.id, entry.label))
        .collect();
    let figure = move || {
        if chart.get() == ChartId::Smith {
            AnyView::new(smith_chart(shape.visible, shape.active, on_marker))
        } else {
            AnyView::new(sweep_chart(SweepInputs {
                rows: shape.visible,
                chart,
                marker: shape.active,
                on_marker,
                on_zoom,
            }))
        }
    };
    let readout = move || {
        shape.visible.with(|rows| {
            rows.get(shape.active.get())
                .map(|row| AnyView::new(marker_readout(row)))
        })
    };
    view! {
        column(class = "tool-stack") {
            row(class = "tool-chips") {{move || sweep.with(|sweep| sweep.as_ref().map(sweep_chips).unwrap_or_default())}}
            row(class = "tool-bar") {
                {segments(chart_options, chart.into(), move |picked| chart.set(picked))}
                spacer() {}
                {move || shape.zoom.get().is_some().then(|| AnyView::new(button("btn", || "Reset zoom".to_owned(), || false, move || view.zoom.set(None))))}
                text(class = "tool-faint tool-mono") {"drag: marker \u{b7} shift-drag: zoom"}
            }
            {figure}
            row(class = "vna-marker") {
                text(class = "legend") {"Marker"}
                {marker_slider(shape, view.marker)}
            }
            {readout}
            {move || shape.analysis.with(|analysis| AnyView::new(sweep_summary(analysis)))}
            {exports(store, sweep, view.format)}
        }
    }
}

fn marker_slider(shape: Sweep, marker: RwSignal<Option<usize>>) -> impl IntoView {
    move || {
        let visible = shape.visible;
        let last = visible.with(Vec::len).saturating_sub(1).max(1) as f64;
        let read = move |value: f64| {
            visible
                .with(|rows| {
                    rows.get(value.round() as usize)
                        .map(|row| format_hz(row.frequency_hz))
                })
                .unwrap_or_default()
        };
        slide(
            Signal::derive(move || shape.active.get() as f64),
            0.0,
            last,
            read,
            move |value| marker.set(Some(value.round() as usize)),
        )
    }
}

fn exports(
    store: Store,
    sweep: Memo<Option<NanoVnaSweep>>,
    format: RwSignal<Format>,
) -> impl IntoView {
    let formats: Vec<(Format, String)> = FORMATS
        .iter()
        .map(|(format, label)| (*format, (*label).to_owned()))
        .collect();
    let save = move |extension: &'static str| {
        sweep.with_untracked(|sweep| {
            let Some(sweep) = sweep else {
                store.say("Sweep first, then export".to_owned());
                return;
            };
            let at = jiff::Timestamp::now().to_string();
            let text = match extension {
                "s2p" => touchstone_s2p(sweep, format.get_untracked(), Some(&at)),
                "s1p" => touchstone_s1p(sweep, format.get_untracked(), Some(&at)),
                _ => sweep_csv(sweep),
            };
            save_for(store, &export_filename(sweep, extension), &text);
        });
    };
    view! {
        row(class = "vna-export") {
            {labelled("Touchstone format", pick(formats, Signal::derive(move || Some(format.get())), move |picked| format.set(picked)))}
            {button("btn", || "Export .s2p".to_owned(), || false, move || save("s2p"))}
            {button("btn", || "Export .s1p".to_owned(), || false, move || save("s1p"))}
            {button("btn", || "Export CSV".to_owned(), || false, move || save("csv"))}
        }
    }
}

fn sweep_chips(sweep: &NanoVnaSweep) -> Vec<AnyView> {
    let mut chips = vec![
        chip("points", sweep.points.len().to_string()),
        chip("averages", sweep.averages.to_string()),
        chip("took", format!("{:.1} s", sweep.elapsed_ms as f64 / 1000.0)),
        correction_chip(sweep.device.calibration.applied, Tone::Danger),
    ];
    if let Some(bandwidth) = sweep.device.bandwidth_hz {
        chips.push(chip("IF", format_hz(f64::from(bandwidth))));
    }
    chips
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::tools::NanoVnaStandard;

    use super::*;
    use crate::ui::{
        kit_channel::{number_text, step_value},
        tools::nanovna::{
            rf::complex,
            testdata::{point, sweep_of as sample},
        },
    };

    #[test]
    fn a_range_in_megahertz_becomes_whole_hertz_and_points() {
        assert_eq!(
            sweep_state(0.05, 30.0, 101.4),
            NanoVnaSweepState {
                start_hz: 50_000,
                stop_hz: 30_000_000,
                points: 101
            }
        );
    }

    #[test]
    fn sweep_fields_step_like_the_web_panel() {
        assert_eq!(step_value(14.0, 0.001, 1.0, SWEEP_MHZ), 14.001);
        assert_eq!(number_text(Some(14.2), SWEEP_MHZ.step), "14.2");
        assert_eq!(step_value(101.0, 10.0, 1.0, POINTS), 111.0);
        assert_eq!(step_value(10_001.0, 10.0, 1.0, POINTS), 10_001.0);
        assert_eq!(step_value(1.0, 1.0, -1.0, AVERAGES), 1.0);
    }

    #[test]
    fn a_zoom_survives_a_new_sweep_that_still_holds_it() {
        assert_eq!(retained_zoom(Some((20, 60)), 101), Some((20, 60)));
        assert_eq!(retained_zoom(Some((20, 60)), 51), None);
        assert_eq!(retained_zoom(None, 101), None);
    }

    #[test]
    fn a_zoom_stays_inside_the_sweep() {
        assert_eq!(visible_range(None, 101), (0, 100));
        assert_eq!(visible_range(Some((20, 60)), 101), (20, 60));
        assert_eq!(visible_range(Some((90, 400)), 101), (0, 100));
        assert_eq!(visible_range(None, 0), (0, 0));
    }

    #[test]
    fn the_marker_starts_on_the_best_match_and_keeps_its_place() {
        assert_eq!(marker_index(None, Some(40), None, 101), 40);
        assert_eq!(marker_index(None, Some(40), Some((30, 60)), 31), 10);
        assert_eq!(marker_index(None, Some(10), Some((30, 60)), 31), 0);
        assert_eq!(marker_index(Some(7), Some(40), None, 101), 7);
        assert_eq!(marker_index(Some(500), None, None, 101), 100);
        assert_eq!(marker_index(None, None, None, 0), 0);
    }

    #[test]
    fn only_a_missing_sweep_waits_and_only_an_empty_one_alarms() {
        assert_eq!(shown_of(None), Shown::Waiting);
        assert_eq!(shown_of(Some(&sample(Vec::new()))), Shown::Empty);
        let one = sample(vec![point(1, complex(0.0, 0.0), complex(0.0, 0.0))]);
        assert_eq!(shown_of(Some(&one)), Shown::Ready);
    }

    fn calibration(port: &str, applied: bool) -> NanoVnaCalibration {
        NanoVnaCalibration {
            port: port.to_owned(),
            standards: vec![NanoVnaStandard::Open],
            error_terms: Vec::new(),
            applied,
            raw: String::new(),
        }
    }

    #[test]
    fn a_calibration_step_wins_until_the_port_changes() {
        let measured = calibration("COM3", true);
        let known = calibration("COM3", false);
        assert_eq!(
            shown_calibration(Some(&measured), Some(known.clone()), "COM3"),
            Some(measured.clone())
        );
        assert_eq!(
            shown_calibration(Some(&measured), Some(known.clone()), "COM4"),
            Some(known.clone())
        );
        assert_eq!(
            shown_calibration(None, Some(known.clone()), "COM3"),
            Some(known)
        );
        assert_eq!(shown_calibration(None, None, "COM3"), None);
    }

    #[test]
    fn a_device_reads_as_maker_product_and_serial() {
        let device = NanoVnaDevice {
            port: "COM3".to_owned(),
            label: "NanoVNA-H4".to_owned(),
            match_kind: NanoVnaMatch::Confirmed,
            model: None,
            manufacturer: Some("nanovna.com".to_owned()),
            product: None,
            serial_number: Some("400".to_owned()),
            usb_vid: None,
            usb_pid: None,
        };
        assert_eq!(device_title(&device), "nanovna.com \u{b7} 400");
    }
}
