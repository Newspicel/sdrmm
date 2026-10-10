use std::{cell::RefCell, rc::Rc};

use sdrmm_wire::{
    frame::{FrameKind, SpatialSpectrumFrame},
    processor::{ProcessorReading, spatial::SpatialReading},
};
use zgui::{prelude::*, reactive::RenderEffect, vocab::SharedString};

use crate::{
    store::Store,
    ui::{
        faces::{
            df::logic::{BearingFrame, frame_options, frame_title, shown_frame},
            processor::{self, Processor, age_label},
            scope::colormap::Colormap,
        },
        kit_raster::readout,
        kit_sources,
        widgets::segments,
    },
};

mod logic;
mod plot;
mod settings;
mod view;

use logic::{
    SpatialView, frame_offset, peak_bearing, readout_rows, spatial_chips, view_options, view_title,
};
use plot::{Grid, SpatialScene};
use settings::{Spatial, spatial_of};
use view::{Plot, colour_scale, plot};

fn spatial_reading(reading: &ProcessorReading) -> Option<&SpatialReading> {
    match reading {
        ProcessorReading::SpatialSpectrum(found) => Some(found),
        _ => None,
    }
}

fn listen(
    store: Store,
    node: String,
    scene: Rc<RefCell<SpatialScene>>,
    bad: RwSignal<u64>,
    axis: RwSignal<Option<(f64, f64)>>,
) {
    let named = node.clone();
    processor::surface(
        store,
        node,
        FrameKind::SpatialSpectrum,
        None,
        move |bytes| {
            let grid = match SpatialSpectrumFrame::decode(bytes) {
                Ok(frame) => Grid::of(&frame),
                Err(error) => {
                    tracing::warn!(node = named, %error, "a spatial spectrum frame did not decode");
                    bad.update(|count| *count += 1);
                    return;
                }
            };
            let edges = Some((grid.low_hz(), grid.low_hz() + grid.span_hz));
            if axis.get_untracked() != edges {
                axis.set(edges);
            }
            match scene.try_borrow_mut() {
                Ok(mut scene) => scene.show(grid),
                Err(_) => tracing::warn!(
                    node = named,
                    "the spatial plot was busy, a frame was skipped"
                ),
            }
        },
    );
}

fn mirror<T: 'static>(
    scene: &Rc<RefCell<SpatialScene>>,
    source: impl Fn() -> T + 'static,
    apply: fn(&mut SpatialScene, T),
) {
    let scene = scene.clone();
    let effect = RenderEffect::new(move |_| {
        let value = source();
        if let Ok(mut scene) = scene.try_borrow_mut() {
            apply(&mut scene, value);
        }
    });
    on_cleanup_local(move || drop(effect));
}

pub fn face(store: Store, node: String) -> impl IntoView {
    kit_sources::install();
    install_stylesheet("spatial-face", view::SHEET);
    let processor = Processor::new(store, node.clone());
    let spatial = Spatial {
        store,
        node: node.clone(),
        settings: processor::settings(store, node.clone(), spatial_of),
    };
    let report_ms = spatial.read(|params| params.report_ms);
    let span_db = spatial.read(|params| params.span_db);
    let reading = processor.reading(spatial_reading);
    let stale = processor.stale(report_ms);
    let age = processor.age();
    let picked = RwSignal::new(None::<BearingFrame>);
    let true_known = Memo::new(move |_| {
        reading.with(|found| found.as_ref().is_some_and(|r| r.azimuth_deg.is_some()))
    });
    let frame = Signal::derive(move || shown_frame(picked.get(), true_known.get()));
    let offset =
        Memo::new(move |_| reading.with(|found| frame_offset(frame.get(), found.as_ref())));
    let shown = RwSignal::new(SpatialView::Map);
    let colormap = RwSignal::new(Colormap::default());
    let bad = RwSignal::new(0_u64);
    let axis = RwSignal::new(None::<(f64, f64)>);
    let scene = Rc::new(RefCell::new(SpatialScene::default()));
    listen(store, node, scene.clone(), bad, axis);
    mirror(&scene, move || shown.get(), SpatialScene::set_view);
    mirror(&scene, move || offset.get(), SpatialScene::set_offset);
    mirror(&scene, move || colormap.get(), SpatialScene::set_colormap);
    mirror(
        &scene,
        move || stale.get() || reading.with(Option::is_none),
        SpatialScene::set_dim,
    );
    mirror(
        &scene,
        move || {
            let (frame, offset) = (frame.get(), offset.get());
            reading.with(|found| {
                found.as_ref().map_or_else(Vec::new, |found| {
                    found
                        .peaks
                        .iter()
                        .map(|peak| (peak.freq_hz, peak_bearing(peak, frame, offset)))
                        .collect()
                })
            })
        },
        SpatialScene::set_marks,
    );
    let rows = move || {
        reading.with(|found| {
            readout_rows(
                found.as_ref(),
                frame.get(),
                offset.get(),
                age_label(age.get()),
            )
        })
    };
    let chips = move || reading.with(|found| spatial_chips(found.as_ref(), bad.get()));
    view! {
        column(class = "face") {
            {processor::head(processor.subtitle(report_ms))}
            {bar(Bar { shown, frame, true_known, picked, colormap, span_db })}
            {plot(scene, Plot { view: shown.into(), axis })}
            {settings::settings(&spatial, colormap)}
            {readout(rows)}
            {processor::error(processor.health)}
            {processor::footer(processor.health, chips)}
        }
    }
}

struct Bar {
    shown: RwSignal<SpatialView>,
    frame: Signal<BearingFrame>,
    true_known: Memo<bool>,
    picked: RwSignal<Option<BearingFrame>>,
    colormap: RwSignal<Colormap>,
    span_db: Signal<f32>,
}

fn bar(bar: Bar) -> impl IntoView {
    let Bar {
        shown,
        frame,
        true_known,
        picked,
        colormap,
        span_db,
    } = bar;
    let frame_hint = move || SharedString::from(frame_title(true_known.get()));
    let view_hint = move || SharedString::from(view_title(shown.get()));
    view! {
        row(class = "ss__bar") {
            box(a11y:label = "View", a11y:description = view_hint) {
                {segments(view_options(), shown.into(), move |chosen| shown.set(chosen))}
            }
            box(a11y:label = "Bearing frame", a11y:description = frame_hint) {
                {move || segments(
                    frame_options(true_known.get()),
                    frame,
                    move |chosen| picked.set(Some(chosen)),
                )}
            }
            {move || (shown.get() == SpatialView::Map)
                .then(|| AnyView::new(colour_scale(colormap.into(), span_db)))}
        }
    }
}
