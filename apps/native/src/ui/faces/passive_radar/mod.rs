use std::{cell::RefCell, rc::Rc};

use sdrmm_wire::{
    frame::{FrameKind, RangeDopplerFrame},
    patch::RADAR_TX_PORT,
    processor::ProcessorReading,
    radar::{Illuminator, RadarUpdate},
};
use zgui::{prelude::*, vocab::SharedString};

use crate::{
    store::Store,
    ui::{
        faces::{
            processor::{self, Processor},
            scope::colormap::Colormap,
        },
        kit_raster::button,
    },
};

mod controls;
mod plot;
mod radar;
mod readings;
mod settings;

use controls::Form;
use plot::Plot;
use radar::{Marks, PlotAxes, RadarScene, marks, plot_axes};
use readings::{
    RadarState, Stat, Subtitle, TrackRow, chips, is_stale, problems, stats, subtitle, track_rows,
};

const TRACK_HEADS: [&str; 7] = ["Track", "km", "m/s", "Hz", "Bearing", "dB", "ADS-B"];

fn reading(reading: &ProcessorReading) -> Option<&RadarUpdate> {
    match reading {
        ProcessorReading::PassiveRadar(update) => Some(update),
        _ => None,
    }
}

fn listen(
    store: Store,
    node: String,
    scene: Rc<RefCell<RadarScene>>,
    frame_axes: RwSignal<Option<PlotAxes>>,
    refused: RwSignal<Option<String>>,
) {
    let named = node.clone();
    processor::surface(store, node, FrameKind::RangeDoppler, None, move |bytes| {
        let frame = match RangeDopplerFrame::decode(bytes) {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(node = named, %error, "a range-doppler frame did not decode");
                refused.set(Some(format!("Bad frame: {error}")));
                return;
            }
        };
        let axes = Some(PlotAxes::of_frame(&frame));
        if frame_axes.get_untracked() != axes {
            frame_axes.set(axes);
        }
        if refused.with_untracked(Option::is_some) {
            refused.set(None);
        }
        match scene.try_borrow_mut() {
            Ok(mut scene) => scene.show(frame.ranges, frame.dopplers, frame.cells),
            Err(_) => tracing::warn!(node = named, "a range-doppler frame arrived mid paint"),
        }
    });
}

fn follow(
    scene: &Rc<RefCell<RadarScene>>,
    marks: Memo<Marks>,
    dim: Signal<bool>,
    colormap: RwSignal<Colormap>,
) {
    let marking = {
        let scene = scene.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let (shown, dim) = (marks.get(), dim.get());
            if let Ok(mut scene) = scene.try_borrow_mut() {
                scene.mark(shown);
                scene.dim(dim);
            }
        })
    };
    let colouring = {
        let scene = scene.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let map = colormap.get();
            if let Ok(mut scene) = scene.try_borrow_mut() {
                scene.recolour(map);
            }
        })
    };
    on_cleanup_local(move || drop((marking, colouring)));
}

fn head(subtitle: Memo<Subtitle>) -> impl IntoView {
    view! {
        row(class = "kit-head") {
            spacer() {}
            text(
                class = "kit-legend",
                class:warn = move || subtitle.with(|shown| shown.warn)
            ) {{move || subtitle.with(|shown| shown.text.clone())}}
        }
    }
}

fn stat_view(stat: Stat) -> impl IntoView {
    view! {
        row(
            class = "rd__stat",
            class:danger = stat.danger,
            a11y:description = SharedString::from(stat.title)
        ) {
            text(class = "rd__stat_label") {{stat.label}}
            text(class = "rd__stat_value") {{stat.value}}
        }
    }
}

fn strip(shown: impl Fn() -> Vec<Stat> + 'static) -> impl IntoView {
    view! {
        row(class = "rd__stats") {
            {move || shown().into_iter().map(|stat| AnyView::new(stat_view(stat))).collect::<Vec<_>>()}
        }
    }
}

fn problem_list(update: Memo<Option<RadarUpdate>>) -> impl IntoView {
    move || {
        let found = update.with(|update| problems(update.as_ref()));
        (!found.is_empty()).then(|| {
            let shown: Vec<_> = found
                .into_iter()
                .map(|label| view! { text(class = "rd__problem") {{label}} })
                .collect();
            view! { row(class = "rd__problems") {{shown}} }
        })
    }
}

fn track_head() -> impl IntoView {
    let cells: Vec<_> = TRACK_HEADS
        .iter()
        .map(|head| view! { text(class = "rd__cell rd__cell--head") {{*head}} })
        .collect();
    view! { row(class = "rd__row") {{cells}} }
}

fn track_view(row: TrackRow, selected: RwSignal<Option<u32>>) -> impl IntoView {
    let id = row.id;
    let title = if row.coasting {
        "Coasting"
    } else {
        "Show on the plot"
    };
    let bearing_title = if row.array_frame { "Array frame" } else { "" };
    view! {
        row(
            class = "rd__row",
            class:coasting = row.coasting,
            class:on = move || selected.get() == Some(id)
        ) {
            control(
                class = "rd__cell rd__pick",
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                a11y:description = SharedString::from(title),
                on:click:stop = move |_| selected.update(|chosen| {
                    *chosen = if *chosen == Some(id) { None } else { Some(id) };
                })
            ) {
                {row.name}
            }
            text(class = "rd__cell") {{row.range}}
            text(class = "rd__cell") {{row.rate}}
            text(class = "rd__cell") {{row.doppler}}
            text(
                class = "rd__cell",
                class:faint = row.array_frame,
                a11y:description = SharedString::from(bearing_title)
            ) {{row.bearing}}
            text(class = "rd__cell") {{row.snr}}
            text(class = "rd__cell rd__cell--wide") {{row.adsb}}
        }
    }
}

fn track_table(
    update: Memo<Option<RadarUpdate>>,
    selected: RwSignal<Option<u32>>,
) -> impl IntoView {
    move || {
        let rows = update.with(|update| track_rows(update.as_ref()));
        (!rows.is_empty()).then(|| {
            let shown: Vec<_> = rows
                .into_iter()
                .map(|row| AnyView::new(track_view(row, selected)))
                .collect();
            view! {
                column(class = "rd__table") {
                    {track_head()}
                    {shown}
                }
            }
        })
    }
}

fn clear_tracks(store: Store, node: String, update: Memo<Option<RadarUpdate>>) -> impl IntoView {
    let pending = RwSignal::new(false);
    let disabled = Signal::derive(move || pending.get() || update.with(Option::is_none));
    let clear = move || {
        let path = format!("/api/radar/{node}/tracks");
        pending.set(true);
        zgui::task::spawn_local(async move {
            if let Err(error) = store.api().delete(&path).await {
                store.fail("Cannot clear the tracks", &error);
            }
            pending.set(false);
        });
    };
    view! {
        box(a11y:description = SharedString::from("Forget every track")) {
            {button("Clear", disabled, clear)}
        }
    }
}

struct Live {
    update: Memo<Option<RadarUpdate>>,
    subtitle: Memo<Subtitle>,
    stale: Signal<bool>,
}

fn live(processor: &Processor, form: &Form) -> Live {
    let update = processor.reading(reading);
    let age = processor.age();
    let stale = Signal::derive(move || {
        let hop = update.with(|update| update.as_ref().map(|update| update.axes.hop_ms));
        hop.zip(age.get())
            .is_some_and(|(hop, age)| is_stale(age, hop))
    });
    let (store, node, array) = (processor.store, processor.node.clone(), processor.array);
    let tx_wired = Memo::new(move |_| {
        store
            .graph
            .with(|graph| graph.sources_of(&node, RADAR_TX_PORT).next().is_some())
    });
    let (gate, lanes, settings) = (processor.gate(), processor.lanes(), form.settings);
    let subtitle = Memo::new(move |_| {
        let illuminator =
            settings.with(|found| found.as_ref().map_or(Illuminator::Fm, |p| p.illuminator));
        update.with(|update| {
            subtitle(&RadarState {
                wired: array.with(Option::is_some),
                tx_wired: tx_wired.get(),
                gate: gate.get(),
                update: update.as_ref(),
                stale: stale.get(),
                illuminator,
                lanes: lanes.get(),
            })
        })
    });
    Live {
        update,
        subtitle,
        stale,
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("passive-radar", SHEET);
    let processor = Processor::new(store, node.clone());
    let form = Form::new(store, node.clone());
    let Live {
        update,
        subtitle,
        stale,
    } = live(&processor, &form);
    let colormap = RwSignal::new(Colormap::default());
    let selected = RwSignal::new(None::<u32>);
    let frame_axes = RwSignal::new(None::<PlotAxes>);
    let refused = RwSignal::new(None::<String>);
    let scene = Rc::new(RefCell::new(RadarScene::new(Colormap::default())));
    listen(store, node.clone(), scene.clone(), frame_axes, refused);
    let axes =
        Memo::new(move |_| update.with(|update| plot_axes(frame_axes.get(), update.as_ref())));
    let shown = Memo::new(move |_| {
        update.with(|update| marks(update.as_ref(), axes.get().as_ref(), selected.get()))
    });
    let dim = Signal::derive(move || stale.get() || update.with(Option::is_none));
    follow(&scene, shown, dim, colormap);
    let (health, status, age, lanes) = (
        processor.health,
        processor.status,
        processor.age(),
        processor.lanes(),
    );
    let rate = move || status.with(|status| status.as_ref().map(|status| status.sample_rate));
    let figures = move || update.with(|update| stats(update.as_ref(), rate(), age.get()));
    let footer_chips = move || update.with(|update| chips(update.as_ref()));
    view! {
        column(class = "face rd") {
            {head(subtitle)}
            {plot::plot(Plot { scene, axes, marks: shown, refused, colormap })}
            {controls::main(&form, lanes, colormap)}
            {strip(figures)}
            {problem_list(update)}
            {track_table(update, selected)}
            {processor::error(health)}
            {controls::folds(&form, lanes)}
            row(class = "rd__foot") {
                box(class = "rd__chips") {{processor::footer(health, footer_chips)}}
                {clear_tracks(store, node, update)}
            }
        }
    }
}

const SHEET: &str = css!(
    r#"
.rd { gap: 6px; }
.rd__plot { position: relative; height: 220px; background-color: #08090b; border-radius: 4px; }
.rd__grid { position: absolute; left: 36px; right: 40px; top: 16px; bottom: 18px; }
.rd__surface { position: absolute; left: 0; top: 0; width: 100%; height: 100%; }
.rd__tick {
    position: absolute;
    height: 12px;
    margin-top: -6px;
    font-family: var(--mono);
    font-size: 9px;
    line-height: 12px;
    white-space: nowrap;
    color: var(--plot-ink-dim);
    pointer-events: none;
}
.rd__tick--left { right: 100%; padding-right: 3px; }
.rd__tick--right { left: 100%; padding-left: 3px; }
.rd__tick--bottom { top: 100%; width: 40px; margin-top: 2px; margin-left: -20px; text-align: center; }
.rd__mark {
    position: absolute;
    margin-left: 6px;
    margin-top: -14px;
    font-family: var(--mono);
    font-size: 9px;
    color: var(--accent);
    pointer-events: none;
}
.rd__hover {
    position: absolute;
    left: 4px;
    top: 4px;
    padding: 0 4px;
    border-radius: 2px;
    font-family: var(--mono);
    font-size: 10px;
    color: var(--ink);
    background-color: rgba(8, 9, 11, 0.8);
    pointer-events: none;
}
.rd__unit { position: absolute; font-family: var(--mono); font-size: 9px; color: var(--ink-faint); }
.rd__unit--hz { left: 4px; top: 2px; }
.rd__unit--mps { right: 4px; top: 2px; }
.rd__unit--km { right: 4px; bottom: 2px; }
.rd__scale {
    position: absolute;
    left: 50%;
    top: 2px;
    width: 140px;
    margin-left: -70px;
    gap: 4px;
    align-items: center;
    font-family: var(--mono);
    font-size: 9px;
    color: var(--ink-faint);
}
.rd__ramp { flex: 1 1 auto; height: 6px; border-radius: 2px; }
.rd__refused {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    margin-top: -7px;
    text-align: center;
    font-family: var(--mono);
    font-size: 10px;
    color: var(--danger);
    pointer-events: none;
}
.rd__stats { flex-wrap: wrap; gap: 2px 12px; }
.rd__stat { gap: 5px; align-items: baseline; }
.rd__stat_label { font-family: var(--mono); font-size: 9px; letter-spacing: 0.09em; text-transform: uppercase; color: var(--ink-faint); }
.rd__stat_value { font-family: var(--mono); font-size: 11px; color: var(--ink); }
.rd__stat.danger > .rd__stat_value { color: var(--danger); }
.rd__problems { flex-wrap: wrap; gap: 6px; }
.rd__problem { font-family: var(--mono); font-size: 10px; color: oklch(0.8 0.14 85); }
.rd__table { border-top: 1px solid var(--line); padding-top: 2px; }
.rd__row { gap: 4px; padding: 1px 2px; border-radius: 2px; }
.rd__row.on { background-color: var(--panel-3); }
.rd__row.coasting { opacity: 0.5; }
.rd__cell { width: 40px; flex: 0 0 auto; font-family: var(--mono); font-size: 10px; color: var(--ink); white-space: nowrap; overflow: hidden; }
.rd__cell--head { color: var(--ink-faint); font-size: 9px; }
.rd__cell--wide { width: 64px; text-overflow: ellipsis; }
.rd__cell.faint { color: var(--ink-faint); }
.rd__pick:hover { color: var(--accent); }
.rd__folds { gap: 2px; }
.rd__foot { gap: 6px; align-items: flex-end; }
.rd__chips { flex: 1 1 auto; min-width: 0; }
"#
);
