use sdrmm_wire::{
    limits::{CORRELATOR_LIMITS as LIMITS, LIMITS as ALL_LIMITS},
    patch::NodeBody,
    processor::{
        ProcessorReading,
        correlator::{CorrelatorParams, CorrelatorReading},
    },
};
use zgui::{prelude::*, reactive::RenderEffect, vocab::SharedString};

use super::processor::{Processor, age_label, edit, error, footer, head, settings};
use crate::{
    store::Store,
    ui::{
        kit_raster::{Bounds, number, readout},
        widgets::{check, pick, row_field},
    },
};

pub mod logic;
pub mod plots;

use logic::{
    Fringe, baseline_options, baseline_rows, bin_options, channel_options, chosen_index, period_ms,
    with_bins,
};
use plots::{fringe_line, plots};

const KHZ: f64 = 1_000.0;

fn read(body: &NodeBody) -> Option<&CorrelatorParams> {
    match body {
        NodeBody::Correlator(node) => Some(&node.settings),
        _ => None,
    }
}

fn write(body: &mut NodeBody) -> Option<&mut CorrelatorParams> {
    match body {
        NodeBody::Correlator(node) => Some(&mut node.settings),
        _ => None,
    }
}

fn heard(reading: &ProcessorReading) -> Option<&CorrelatorReading> {
    match reading {
        ProcessorReading::Correlator(found) => Some(found),
        _ => None,
    }
}

fn titled(title: &'static str, body: impl IntoView + 'static) -> impl IntoView {
    view! { box(a11y:description = SharedString::from(title)) {{body}} }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("correlator-face", SHEET);
    let processor = Processor::new(store, node.clone());
    let params = settings(store, node.clone(), read);
    let reading = processor.reading(heard);
    let period = Signal::derive(move || params.with(|found| period_ms(found.as_ref())));
    let subtitle = processor.subtitle(period);
    let stale = processor.stale(period);
    let age = processor.age();
    let health = processor.health;
    let chosen = RwSignal::new(0usize);
    let count = Memo::new(move |_| {
        reading.with(|found| found.as_ref().map_or(0, |found| found.baselines.len()))
    });
    let index = Signal::derive(move || chosen_index(chosen.get(), count.get()));
    let baseline = Memo::new(move |_| {
        reading.with(|found| {
            found
                .as_ref()
                .and_then(|found| found.baselines.get(index.get()).copied())
        })
    });
    let fringe = fringe(reading, index);
    let dim = Signal::derive(move || stale.get() || reading.with(Option::is_none));
    let known = Memo::new(move |_| health.with(Option::is_some));
    let rows = move || {
        reading.with(|found| {
            baseline_rows(
                found.as_ref(),
                baseline.get().as_ref(),
                age_label(age.get()),
            )
        })
    };
    head(store, node.clone(), subtitle);
    view! {
        column(class = "face") {
            {plots(store, node.clone(), known, index, dim)}
            {controls(store, node, params, reading, chosen, index)}
            row(class = "cor__readouts") {
                box(class = "cor__rows") {{readout(rows)}}
                {fringe_line(fringe)}
            }
            {error(health)}
            {footer(health, Vec::new)}
        }
    }
}

fn fringe(reading: Memo<Option<CorrelatorReading>>, index: Signal<usize>) -> Signal<Vec<f64>> {
    let held = StoredValue::new(Fringe::default());
    let history = RwSignal::new(Vec::<f64>::new());
    let tracking = RenderEffect::new(move |_| {
        let index = index.get();
        let changed = reading.with(|found| {
            let Some(found) = found else {
                return false;
            };
            let Some(baseline) = found.baselines.get(index) else {
                return false;
            };
            held.update_value(|fringe| {
                fringe.heard(index, &found.at, f64::from(baseline.phase_deg));
            });
            true
        });
        if changed {
            history.set(held.with_value(|fringe| fringe.history.clone()));
        }
    });
    on_cleanup_local(move || drop(tracking));
    history.into()
}

fn controls(
    store: Store,
    node: String,
    params: Memo<Option<CorrelatorParams>>,
    reading: Memo<Option<CorrelatorReading>>,
    chosen: RwSignal<usize>,
    index: Signal<usize>,
) -> impl IntoView {
    let change =
        move |apply: Box<dyn FnOnce(&mut CorrelatorParams)>| edit(store, &node, write, apply);
    let value = move |of: fn(&CorrelatorParams) -> f64| {
        Signal::derive(move || params.with(|found| found.as_ref().map(of).unwrap_or_default()))
    };
    let options = Memo::new(move |_| {
        reading.with(|found| {
            found
                .as_ref()
                .map(|found| baseline_options(&found.baselines))
                .unwrap_or_default()
        })
    });
    let bins = Memo::new(move |_| params.with(|found| found.as_ref().map(|found| found.bins)));
    let channels =
        Memo::new(move |_| params.with(|found| found.as_ref().map(|found| found.channels)));
    let overlap = Signal::derive(move || {
        params.with(|found| found.as_ref().is_some_and(|found| found.overlap))
    });
    let baseline_pick = move || {
        let shown = options.get();
        (!shown.is_empty()).then(|| {
            AnyView::new(titled(
                "Baseline",
                row_field(
                    "Baseline",
                    pick(
                        shown,
                        Signal::derive(move || Some(index.get())),
                        move |picked| chosen.set(picked),
                    ),
                ),
            ))
        })
    };
    let channel_pick = {
        let change = change.clone();
        move || {
            let change = change.clone();
            let offered = channel_options(bins.get().unwrap_or(LIMITS.bins.max));
            AnyView::new(titled(
                "Frequency points sent per baseline",
                row_field(
                    "Channels",
                    pick(offered, channels.into(), move |picked: u32| {
                        change(Box::new(move |found| found.channels = picked));
                    }),
                ),
            ))
        }
    };
    let on_bins = {
        let change = change.clone();
        move |picked: u32| change(Box::new(move |found| with_bins(found, picked)))
    };
    let on_integrate = {
        let change = change.clone();
        move |seconds: f64| change(Box::new(move |found| found.integrate_s = seconds as f32))
    };
    let on_overlap = {
        let change = change.clone();
        move |on: bool| change(Box::new(move |found| found.overlap = on))
    };
    view! {
        column(class = "params") {
            {baseline_pick}
            {titled("Frequency bins", row_field("FFT", pick(bin_options(), bins.into(), on_bins)))}
            {channel_pick}
            {titled("Averaging time per result", row_field("Integrate s", number(
                "Integrate",
                value(|found| f64::from(found.integrate_s)),
                Bounds::new(f64::from(LIMITS.integrate_s.min), f64::from(LIMITS.integrate_s.max)),
                on_integrate,
            )))}
            {titled("Half-overlapping FFTs, twice the work", row_field("Overlap", check(overlap, on_overlap)))}
            {band(params, change)}
        }
    }
}

fn band(
    params: Memo<Option<CorrelatorParams>>,
    change: impl Fn(Box<dyn FnOnce(&mut CorrelatorParams)>) + Clone + 'static,
) -> impl IntoView {
    let offset = Signal::derive(move || {
        params.with(|found| found.as_ref().map_or(0.0, |found| found.offset_hz / KHZ))
    });
    let width = Memo::new(move |_| {
        params.with(|found| found.as_ref().and_then(|found| found.bandwidth_hz))
    });
    let full = Signal::derive(move || width.get().is_none());
    let on_offset = {
        let change = change.clone();
        move |khz: f64| change(Box::new(move |found| found.offset_hz = khz * KHZ))
    };
    let on_full = {
        let change = change.clone();
        move |on: bool| {
            let next = (!on).then_some(ALL_LIMITS.band_seed_hz);
            change(Box::new(move |found| found.bandwidth_hz = next));
        }
    };
    let width_field = move || {
        let change = change.clone();
        width.get().map(|hz| {
            AnyView::new(titled(
                "Band around the offset",
                row_field(
                    "Width kHz",
                    number(
                        "Width",
                        Signal::derive(move || hz / KHZ),
                        Bounds::new(
                            LIMITS.band.bandwidth_hz.min / KHZ,
                            LIMITS.band.bandwidth_hz.max / KHZ,
                        ),
                        move |khz: f64| {
                            change(Box::new(move |found| found.bandwidth_hz = Some(khz * KHZ)))
                        },
                    ),
                ),
            ))
        })
    };
    view! {
        {titled("From the array centre", row_field("Offset kHz", number(
            "Offset",
            offset,
            Bounds::new(LIMITS.band.offset_hz.min / KHZ, LIMITS.band.offset_hz.max / KHZ),
            on_offset,
        )))}
        {titled("Band around the offset, or every lane sample", row_field("Full band", check(full, on_full)))}
        {width_field}
    }
}

const SHEET: &str = css!(
    r#"
.cor__plot { position: relative; height: 200px; flex: 0 0 auto; background-color: #000000; overflow: hidden; }
.cor__canvas { position: absolute; left: 0; top: 0; width: 100%; height: 200px; }
.cor__name { position: absolute; left: 4px; font-family: var(--mono); font-size: 9px; color: var(--ink-faint); pointer-events: none; }
.cor__unit { left: auto; right: 4px; bottom: 2px; }
.cor__tick { position: absolute; left: 0; width: 30px; text-align: right; font-family: var(--mono); font-size: 9px; color: var(--plot-ink-dim); pointer-events: none; }
.cor__axis { position: absolute; left: 34px; right: 6px; bottom: 2px; height: 12px; pointer-events: none; }
.cor__freq { position: absolute; top: 0; width: 40px; margin-left: -20px; text-align: center; font-family: var(--mono); font-size: 9px; color: var(--plot-ink-dim); }
.cor__refused { position: absolute; left: 40px; right: 8px; top: 90px; text-align: center; }
.cor__readouts { align-items: center; gap: 8px; padding-top: 6px; border-top: 1px solid var(--line); }
.cor__rows { flex: 1 1 auto; min-width: 0; }
.cor__fringe_box { flex: 0 0 auto; width: 72px; height: 22px; border-radius: 2px; background-color: var(--bg); overflow: hidden; }
.cor__fringe { width: 72px; height: 22px; }
"#
);
