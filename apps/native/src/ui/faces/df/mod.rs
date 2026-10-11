use sdrmm_wire::processor::{ProcessorReading, df::DfReading};
use zgui::{prelude::*, vocab::SharedString};

use crate::{
    store::Store,
    ui::{
        faces::processor::{self, Processor, age_label},
        kit_raster::readout,
        kit_sources,
        widgets::segments,
    },
};

pub mod geometry;
pub mod logic;
pub mod options;
mod rose;
mod settings;

use logic::{BearingFrame, df_chips, frame_options, frame_title, readout_rows, shown_frame};
use rose::{Rose, rose};
use settings::{ArrayShape, Finder, df_of};

fn df_reading(reading: &ProcessorReading) -> Option<&DfReading> {
    match reading {
        ProcessorReading::Df(found) => Some(found),
        _ => None,
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    let titled = node.clone();
    kit_sources::install();
    install_stylesheet("df-face", SHEET);
    install_stylesheet("df-rose", rose::SHEET);
    let processor = Processor::new(store, node.clone());
    let settings = processor::settings(store, node.clone(), df_of);
    let report_ms = Signal::derive(move || {
        settings.with(|found| found.as_ref().map_or(0, |params| params.report_ms))
    });
    let reading = processor.reading(df_reading);
    let stale = processor.stale(report_ms);
    let gate = processor.gate();
    let age = processor.age();
    let array = processor.array;
    let shape = Memo::new(move |_| {
        store
            .graph
            .with(|graph| ArrayShape::of(graph, array.get().as_deref()))
    });
    let picked = RwSignal::new(None::<BearingFrame>);
    let true_available = Memo::new(move |_| {
        reading.with(|found| found.as_ref().is_some_and(|r| r.azimuth_deg.is_some()))
    });
    let frame = Signal::derive(move || shown_frame(picked.get(), true_available.get()));
    let dim = Signal::derive(move || stale.get() || gate.get().is_some());
    let finder = Finder {
        store,
        node,
        settings,
        shape,
    };
    let rows = move || {
        reading.with(|found| readout_rows(found.as_ref(), frame.get(), age_label(age.get())))
    };
    let chips = move || reading.with(|found| df_chips(found.as_ref(), gate.get(), stale.get()));
    processor::head(store, titled, processor.subtitle(report_ms));
    view! {
        column(class = "face") {
            column(class = "df__top") {
                {frames(frame, true_available, picked)}
                {rose(Rose { reading, frame, dim })}
            }
            {settings::main(&finder)}
            {readout(rows)}
            {processor::error(processor.health)}
            {settings::more(&finder)}
            {processor::footer(processor.health, chips)}
        }
    }
}

fn frames(
    frame: Signal<BearingFrame>,
    true_available: Memo<bool>,
    picked: RwSignal<Option<BearingFrame>>,
) -> impl IntoView {
    let title = move || SharedString::from(frame_title(true_available.get()));
    view! {
        box(a11y:label = "Bearing frame", a11y:description = title) {
            {move || segments(
                frame_options(true_available.get()),
                frame,
                move |chosen| picked.set(Some(chosen)),
            )}
        }
    }
}

const SHEET: &str = css!(
    r#"
.df__top { align-items: center; gap: 8px; }
.df__off { opacity: 0.45; }
"#
);
