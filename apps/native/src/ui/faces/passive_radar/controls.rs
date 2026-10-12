use std::rc::Rc;

use sdrmm_wire::{
    limits::RADAR_LIMITS as LIMITS,
    radar::{CfarKind, CfarWindow, ClutterMethod, PassiveRadarParams},
};
use zgui::{prelude::*, vocab::SharedString};

use super::settings::{
    CFAR_OPTIONS, CFAR_WINDOW_OPTIONS, CLUTTER_OPTIONS, CleaningKind, GPU_OPTIONS,
    ILLUMINATOR_OPTIONS, WINDOW_OPTIONS, all_elements, bandwidth_of, cfar_choice, cfar_kind_of,
    cleaning_kind, cleaning_of, cleaning_options, cma_read, cma_step, cma_taps, colour_options,
    eca, element_options, kind_of, nlms, options, params_mut, params_of, pfa_exponent, pfa_of,
    pfa_options, rank_read, rank_write, set_reference, span, start_hits, start_looks,
    surveillance_of, switch_illuminator, toggled_surveillance, whole, with_bandwidth,
};
use crate::{
    store::Store,
    ui::{
        faces::{processor, scope::colormap::Colormap},
        kit_raster::{Bounds, number},
        kit_sources::collapsible,
        widgets::{check, pick, row_field},
    },
};

type Params = PassiveRadarParams;

const MIN_LANES: usize = 2;

#[derive(Clone)]
pub struct Form {
    store: Store,
    node: Rc<str>,
    pub settings: Memo<Option<Params>>,
}

impl Form {
    pub fn new(store: Store, node: String) -> Self {
        Self {
            store,
            settings: processor::settings(store, node.clone(), params_of),
            node: node.into(),
        }
    }

    fn apply(&self, change: impl FnOnce(&mut Params)) {
        processor::edit(self.store, &self.node, params_mut, change);
    }

    fn read<V>(&self, pick: fn(&Params) -> V) -> Signal<V>
    where
        V: Default + Clone + Send + Sync + 'static,
    {
        let settings = self.settings;
        Signal::derive(move || settings.with(|found| found.as_ref().map(pick).unwrap_or_default()))
    }

    fn when(&self, test: fn(&Params) -> bool) -> Memo<bool> {
        let settings = self.settings;
        Memo::new(move |_| settings.with(|found| found.as_ref().is_some_and(test)))
    }

    fn number(
        &self,
        label: &'static str,
        title: &'static str,
        value: fn(&Params) -> f64,
        bounds: Bounds,
        write: fn(&mut Params, f64),
    ) -> AnyView {
        let form = self.clone();
        let commit = move |next: f64| form.apply(|params| write(params, next));
        titled(
            title,
            row_field(label, number(label, self.read(value), bounds, commit)),
        )
    }

    fn choice<T>(
        &self,
        label: &'static str,
        title: &'static str,
        choices: Vec<(T, String)>,
        value: fn(&Params) -> T,
        write: fn(&mut Params, T),
    ) -> AnyView
    where
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        let settings = self.settings;
        let chosen = Signal::derive(move || settings.with(|found| found.as_ref().map(value)));
        let form = self.clone();
        let on_pick = move |next: T| form.apply(|params| write(params, next));
        titled(title, row_field(label, pick(choices, chosen, on_pick)))
    }

    fn toggle(
        &self,
        label: &'static str,
        title: &'static str,
        value: fn(&Params) -> bool,
        write: fn(&mut Params, bool),
    ) -> AnyView {
        let form = self.clone();
        let change = move |on: bool| form.apply(|params| write(params, on));
        titled(title, row_field(label, check(self.read(value), change)))
    }
}

fn titled(title: &'static str, body: impl IntoView + 'static) -> AnyView {
    if title.is_empty() {
        return AnyView::new(body);
    }
    AnyView::new(view! { box(a11y:description = SharedString::from(title)) {{body}} })
}

fn colours(colormap: RwSignal<Colormap>) -> AnyView {
    let chosen = Signal::derive(move || Some(colormap.get().index() as u8));
    let on_pick = move |index: u8| colormap.set(Colormap::from_index(index));
    titled(
        "Plot colours",
        row_field("Colours", pick(colour_options(), chosen, on_pick)),
    )
}

fn reference(form: &Form, lanes: Signal<usize>) -> impl IntoView {
    let form = form.clone();
    move || {
        let count = lanes.get().max(MIN_LANES) as u32;
        form.choice(
            "Ref",
            "Element pointed at the transmitter",
            element_options(count),
            |params| params.reference_element,
            set_reference,
        )
    }
}

fn cleaning(form: &Form) -> impl IntoView {
    let form = form.clone();
    let settings = form.settings;
    let lit =
        Memo::new(move |_| settings.with(|found| found.as_ref().map(|p| kind_of(&p.illuminator))));
    move || {
        let choices = lit.get().map(cleaning_options).unwrap_or_default();
        form.choice(
            "Clean",
            "Reference cleaning",
            choices,
            |params| cleaning_kind(&params.reference),
            |params, kind| params.reference = cleaning_of(kind, params.reference),
        )
    }
}

pub fn main(form: &Form, lanes: Signal<usize>, colormap: RwSignal<Colormap>) -> impl IntoView {
    view! {
        column(class = "params") {
            {colours(colormap)}
            {form.choice("Illum", "Transmitter type", options(&ILLUMINATOR_OPTIONS), |p| kind_of(&p.illuminator), switch_illuminator)}
            {form.number("Offset kHz", "Illuminator minus array centre", |p| p.offset_hz / 1e3, span(LIMITS.offset_hz, 1e-3, 1.0), |p, khz| p.offset_hz = khz * 1e3)}
            {reference(form, lanes)}
            {cleaning(form)}
            {form.number("Range km", "Bistatic excess range", |p| f64::from(p.max_range_km), span(LIMITS.max_range_km, 1.0, 1.0), |p, km| p.max_range_km = km as f32)}
            {form.number("CPI ms", "Coherent processing interval", |p| f64::from(p.cpi_ms), whole(LIMITS.cpi_ms), |p, ms| p.cpi_ms = ms as u32)}
            {form.toggle("AoA", "Angle of arrival per echo, needs calibration", |p| p.aoa, |p, on| p.aoa = on)}
            {form.choice("GPU", "Where the heavy maths runs", options(&GPU_OPTIONS), |p| p.gpu, |p, gpu| p.gpu = gpu)}
            {form.choice("Clutter", "Clutter method", options(&CLUTTER_OPTIONS), |p| p.clutter.method, |p, method| p.clutter.method = method)}
            {form.choice("Pfa", "False alarm chance per cell", pfa_options(), |p| pfa_exponent(p.cfar.pfa), |p, exponent| p.cfar.pfa = pfa_of(exponent))}
        }
    }
}

fn surveillance(form: &Form, lanes: Signal<usize>) -> impl IntoView {
    let form = form.clone();
    move || {
        let count = lanes.get().max(MIN_LANES) as u32;
        let items: Vec<_> = all_elements(count)
            .into_iter()
            .map(|element| AnyView::new(element_toggle(&form, element, count)))
            .collect();
        titled(
            "Elements that listen for echoes",
            row_field("Surveillance", view! { row(class = "seg") {{items}} }),
        )
    }
}

fn element_toggle(form: &Form, element: u32, count: u32) -> impl IntoView {
    let settings = form.settings;
    let on = Signal::derive(move || {
        settings.with(|found| {
            found.as_ref().is_some_and(|params| {
                surveillance_of(params.surveillance, params.reference_element, count)
                    .contains(&element)
            })
        })
    });
    let reference = Signal::derive(move || {
        settings.with(|found| {
            found
                .as_ref()
                .is_some_and(|params| params.reference_element == element)
        })
    });
    let form = form.clone();
    let flip = move || {
        form.apply(|params| {
            params.surveillance = toggled_surveillance(
                params.surveillance,
                element,
                params.reference_element,
                count,
            );
        });
    };
    view! {
        control(
            class = "seg__item",
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            class:on = move || on.get(),
            state:disabled = move || reference.get(),
            on:click:stop = move |_| flip()
        ) {
            {(element + 1).to_string()}
        }
    }
}

fn bandwidth(form: &Form) -> impl IntoView {
    let form = form.clone();
    let carried = form.when(|p| bandwidth_of(&p.illuminator).is_some());
    move || {
        carried.get().then(|| {
            form.number(
                "Bandwidth kHz",
                "",
                |p| bandwidth_of(&p.illuminator).unwrap_or_default() / 1e3,
                span(LIMITS.bandwidth_hz, 1e-3, 1.0),
                |p, khz| p.illuminator = with_bandwidth(p.illuminator, khz * 1e3),
            )
        })
    }
}

fn cma(form: &Form) -> impl IntoView {
    let form = form.clone();
    let shown = form.when(|p| cleaning_kind(&p.reference) == CleaningKind::Cma);
    move || {
        shown.get().then(|| {
            AnyView::new(view! {
                column(class = "params") {
                    {form.number("CMA taps", "", |p| cma_read(p).0, whole(LIMITS.cma_taps), cma_taps)}
                    {form.number("CMA step", "", |p| cma_read(p).1, span(LIMITS.cma_step, 1.0, 1e-4), cma_step)}
                }
            })
        })
    }
}

fn extras(form: &Form, lanes: Signal<usize>) -> AnyView {
    AnyView::new(view! {
        column(class = "params") {
            {bandwidth(form)}
            {surveillance(form, lanes)}
            {cma(form)}
            {form.number("Speed m/s", "Largest range rate", |p| f64::from(p.max_speed_mps), span(LIMITS.max_speed_mps, 1.0, 10.0), |p, mps| p.max_speed_mps = mps as f32)}
            {form.number("Overlap %", "Share of each CPI reused by the next", |p| f64::from(p.overlap * 100.0).round(), span(LIMITS.overlap, 100.0, 5.0), |p, percent| p.overlap = (percent / 100.0) as f32)}
        }
    })
}

fn eca_rows(form: &Form) -> impl IntoView {
    let form = form.clone();
    let batch = form.when(|p| p.clutter.method == ClutterMethod::EcaBatch);
    let taps = form.number(
        "Doppler taps",
        "Doppler shifts each side",
        |p| f64::from(p.clutter.doppler_taps),
        whole(LIMITS.doppler_taps),
        |p, taps| p.clutter.doppler_taps = taps as u32,
    );
    let loading = form.number(
        "Loading",
        "Diagonal loading",
        |p| f64::from(p.clutter.loading),
        span(LIMITS.loading, 1.0, 1e-4),
        |p, loading| p.clutter.loading = loading as f32,
    );
    let timing = move || {
        if batch.get() {
            AnyView::new(view! {
                column(class = "params") {
                    {form.number("Batch ms", "", |p| f64::from(p.clutter.batch_ms), span(LIMITS.batch_ms, 1.0, 1.0), |p, ms| p.clutter.batch_ms = ms as f32)}
                    {form.toggle("Taper", "Blend weights between batches", |p| p.clutter.taper, |p, on| p.clutter.taper = on)}
                }
            })
        } else {
            form.number(
                "Extension ms",
                "",
                |p| f64::from(p.clutter.extension_ms),
                span(LIMITS.extension_ms, 1.0, 1.0),
                |p, ms| p.clutter.extension_ms = ms as f32,
            )
        }
    };
    view! {
        column(class = "params") {
            {taps}
            {timing}
            {loading}
        }
    }
}

fn clutter_fields(form: &Form) -> AnyView {
    let ecas = form.when(|p| eca(p.clutter.method));
    let nlmses = form.when(|p| nlms(p.clutter.method));
    let (eca_form, nlms_form) = (form.clone(), form.clone());
    AnyView::new(view! {
        column(class = "params") {
            {form.number("Reach km", "Clutter extent removed", |p| f64::from(p.clutter.reach_km), span(LIMITS.reach_km, 1.0, 0.1), |p, km| p.clutter.reach_km = km as f32)}
            {form.number("Lead", "Taps before zero delay", |p| f64::from(p.clutter.lead), whole(LIMITS.lead), |p, lead| p.clutter.lead = lead as u32)}
            {move || ecas.get().then(|| AnyView::new(eca_rows(&eca_form)))}
            {move || nlmses.get().then(|| nlms_form.number("Step", "", |p| f64::from(p.clutter.step), span(LIMITS.clutter_step, 1.0, 1e-3), |p, step| p.clutter.step = step as f32))}
        }
    })
}

fn cfar_cells(form: &Form) -> AnyView {
    let plane = form.when(|p| p.cfar.window == CfarWindow::Plane);
    let ranked = form.when(|p| matches!(p.cfar.kind, CfarKind::Os { .. }));
    let (plane_form, rank_form) = (form.clone(), form.clone());
    AnyView::new(view! {
        column(class = "params") {
            {form.number("Guard", "Cells skipped next to the cell tested", |p| f64::from(p.cfar.guard_range), whole(LIMITS.guard), |p, cells| p.cfar.guard_range = cells as u32)}
            {form.number("Train", "Cells averaged for the noise", |p| f64::from(p.cfar.train_range), whole(LIMITS.train_range), |p, cells| p.cfar.train_range = cells as u32)}
            {move || plane.get().then(|| AnyView::new(view! {
                column(class = "params") {
                    {plane_form.number("Guard Doppler", "Cells skipped next to the cell tested", |p| f64::from(p.cfar.guard_doppler), whole(LIMITS.guard), |p, cells| p.cfar.guard_doppler = cells as u32)}
                    {plane_form.number("Train Doppler", "Cells averaged for the noise", |p| f64::from(p.cfar.train_doppler), whole(LIMITS.train_doppler), |p, cells| p.cfar.train_doppler = cells as u32)}
                }
            }))}
            {move || ranked.get().then(|| rank_form.number("Rank %", "Ordered cell used as the noise estimate", rank_read, span(LIMITS.os_rank, 100.0, 1.0), rank_write))}
        }
    })
}

fn detect_fields(form: &Form) -> AnyView {
    AnyView::new(view! {
        column(class = "params") {
            {form.choice("CFAR", "", options(&CFAR_OPTIONS), |p| cfar_choice(&p.cfar.kind), |p, choice| p.cfar.kind = cfar_kind_of(choice, p.cfar.kind))}
            {form.choice("Window", "Range: across delay per Doppler row", options(&CFAR_WINDOW_OPTIONS), |p| p.cfar.window, |p, window| p.cfar.window = window)}
            {cfar_cells(form)}
            {form.number("Min Doppler Hz", "Slower rows count as clutter", |p| f64::from(p.cfar.min_doppler_hz), span(LIMITS.min_doppler_hz, 1.0, 0.5), |p, hz| p.cfar.min_doppler_hz = hz as f32)}
            {form.number("Min range km", "", |p| f64::from(p.cfar.min_range_km), span(LIMITS.min_range_km, 1.0, 0.5), |p, km| p.cfar.min_range_km = km as f32)}
            {form.number("Min SNR dB", "", |p| f64::from(p.cfar.min_snr_db), span(LIMITS.min_snr_db, 1.0, 0.5), |p, db| p.cfar.min_snr_db = db as f32)}
            {form.choice("Doppler window", "Taper across batches", options(&WINDOW_OPTIONS), |p| p.window, |p, window| p.window = window)}
        }
    })
}

fn track_fields(form: &Form) -> AnyView {
    AnyView::new(view! {
        column(class = "params") {
            {form.number("Start hits", "Hits needed in a window of looks", |p| f64::from(p.tracker.confirm_hits), whole(LIMITS.track_window), start_hits)}
            {form.number("Start looks", "Window of looks", |p| f64::from(p.tracker.confirm_window), whole(LIMITS.track_window), start_looks)}
            {form.number("Coast", "Missed looks before a track ends", |p| f64::from(p.tracker.coast_looks), whole(LIMITS.coast_looks), |p, looks| p.tracker.coast_looks = looks as u32)}
            {form.number("Max accel m/s\u{b2}", "", |p| f64::from(p.tracker.max_accel_mps2), span(LIMITS.max_accel_mps2, 1.0, 1.0), |p, accel| p.tracker.max_accel_mps2 = accel as f32)}
            {form.number("Gate", "How far an echo may land from its track", |p| f64::from(p.tracker.gate), span(LIMITS.gate, 1.0, 0.1), |p, gate| p.tracker.gate = gate as f32)}
            {form.number("Jerk", "How fast targets may change acceleration", |p| f64::from(p.tracker.jerk), span(LIMITS.jerk, 1.0, 0.1), |p, jerk| p.tracker.jerk = jerk as f32)}
            {form.number("Altitude m", "Target height when no ADS-B match", |p| f64::from(p.assumed_altitude_m), span(LIMITS.altitude_m, 1.0, 100.0), |p, metres| p.assumed_altitude_m = metres as f32)}
        }
    })
}

fn fold(label: &'static str, body: impl Fn() -> AnyView + 'static) -> impl IntoView {
    collapsible(move || label.to_owned(), "kit-fold__link", body)
}

pub fn folds(form: &Form, lanes: Signal<usize>) -> impl IntoView {
    let cleared = form.when(|p| p.clutter.method != ClutterMethod::Off);
    let (more, clutter, detect, track) = (form.clone(), form.clone(), form.clone(), form.clone());
    let clutter = move || {
        let clutter = clutter.clone();
        cleared
            .get()
            .then(|| AnyView::new(fold("Clutter", move || clutter_fields(&clutter))))
    };
    view! {
        column(class = "rd__folds") {
            {fold("More", move || extras(&more, lanes))}
            {clutter}
            {fold("Detect", move || detect_fields(&detect))}
            {fold("Track", move || track_fields(&track))}
        }
    }
}
