use std::{cell::RefCell, rc::Rc, sync::Arc};

use sdrmm_wire::{frame::FrameKind, frame::SymbolPlane, ws::ClientCommand};
use zgui::prelude::*;

use crate::{
    binding,
    bus::Source,
    store::Store,
    ui::{
        faces::{
            channel::settings::NumberLimit,
            scope::{chosen_palette, dismiss::dismissable, tip},
        },
        kit_channel::{self, NumberSpec, number_field},
        kit_raster::raster_view,
        widgets::{check, segments},
    },
};

pub mod frames;
pub mod grid;
pub mod measure;
pub mod scope;

use frames::{Block, Burst};
use grid::{Eye, SymbolState, samples_per_symbol, symbol_states};
use measure::{Measurement, SIGNAL_VIEWS, SYMBOL_VIEWS, View, measurements, waiting};
use scope::{Anchor, Label, Scope, Settings};

const BURST_EVERY: u64 = 10;
const BLOCK_EVERY: u64 = 4;
const STATE_EXTENT: f32 = 1.1;
const STATE_CAPTION_PX: f32 = 16.0;
const FIGURES_MIN_PX: f32 = 12.0;
const MIN_SYMBOL_RATE: f64 = 1.0;
const SYMBOL_RATE_STEP: f64 = 100.0;
const SYMBOLS_ONLY_HINT: &str = "Plot one point per symbol instead of every sample";
const GEAR_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 17H5"/><path d="M19 7h-9"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/></svg>"#;

#[derive(Clone, Copy)]
struct Controls {
    view: RwSignal<View>,
    eye: RwSignal<Eye>,
    rate: RwSignal<f64>,
    decimate: RwSignal<bool>,
    open: RwSignal<bool>,
}

impl Controls {
    fn new() -> Self {
        let settings = Settings::default();
        Self {
            view: RwSignal::new(settings.view),
            eye: RwSignal::new(settings.eye),
            rate: RwSignal::new(f64::from(settings.symbol_rate)),
            decimate: RwSignal::new(settings.decimate),
            open: RwSignal::new(false),
        }
    }

    fn settings(self) -> Settings {
        Settings {
            view: self.view.get(),
            eye: self.eye.get(),
            symbol_rate: self.rate.get() as f32,
            decimate: self.decimate.get(),
        }
    }
}

#[derive(Clone, Copy)]
struct Seen {
    burst: RwSignal<Option<Arc<Burst>>>,
    block: RwSignal<Option<Arc<Block>>>,
    stamp: RwSignal<u64>,
}

impl Seen {
    fn refresh(self) {
        self.stamp.update(|stamp| *stamp += 1);
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("wp11-baseband", SHEET);
    kit_channel::install();
    let tap = Memo::new(move |_| {
        binding::sources_of(&store.graph.get(), &node, "baseband")
            .into_iter()
            .find_map(|source| Some((store.device_set_of(&source)?, store.channel_of(&source)?.id)))
    });
    view! {
        column(class = "bb") {
            {move || match tap.get() {
                Some((set, channel)) => AnyView::new(watch(store, set, channel)),
                None => AnyView::new(view! { box(class = "bb__empty") { text(class = "hint") {"Wire a channel's baseband"} } }),
            }}
        }
    }
}

fn listen(store: Store, device_set: u32, channel: u32, scope: Rc<RefCell<Scope>>, seen: Seen) {
    store.hold(ClientCommand::SubscribeIq {
        device_set,
        channel,
    });
    store.hold(ClientCommand::SubscribeSymbols {
        device_set,
        channel,
    });
    let counts = Rc::new(RefCell::new((0u64, 0u64)));
    store.on_frame(move |frame| {
        let source = store.source_of(frame.stream_id);
        let Ok(mut held) = scope.try_borrow_mut() else {
            return;
        };
        let mut counts = counts.borrow_mut();
        match frame.kind {
            FrameKind::IqF32
                if source
                    == Some(Source::Iq {
                        device_set,
                        channel,
                    }) =>
            {
                let Some(burst) = Burst::read(&frame.bytes) else {
                    tracing::warn!(device_set, channel, "an IQ burst did not decode");
                    return;
                };
                counts.0 += 1;
                let publish = counts.0 == 1 || counts.0.is_multiple_of(BURST_EVERY);
                if publish {
                    seen.burst.set(Some(Arc::new(burst.clone())));
                }
                held.take_burst(burst);
                if publish {
                    seen.refresh();
                }
            }
            FrameKind::Symbols
                if source
                    == Some(Source::Symbols {
                        device_set,
                        channel,
                    }) =>
            {
                let Some(block) = Block::read(&frame.bytes) else {
                    tracing::warn!(device_set, channel, "a symbol block did not decode");
                    return;
                };
                counts.1 += 1;
                let publish = counts.1 == 1 || counts.1.is_multiple_of(BLOCK_EVERY);
                if publish {
                    seen.block.set(Some(Arc::new(block.clone())));
                }
                held.take_block(block);
                if publish {
                    seen.refresh();
                }
            }
            _ => {}
        }
    });
}

fn watch(store: Store, device_set: u32, channel: u32) -> impl IntoView {
    let controls = Controls::new();
    let seen = Seen {
        burst: RwSignal::new(None),
        block: RwSignal::new(None),
        stamp: RwSignal::new(0),
    };
    let scope = Rc::new(RefCell::new(Scope::new(chosen_palette())));
    listen(store, device_set, channel, scope.clone(), seen);
    let configure = {
        let scope = scope.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let settings = controls.settings();
            if let Ok(mut held) = scope.try_borrow_mut() {
                held.configure(settings);
                seen.refresh();
            }
        })
    };
    on_cleanup_local(move || drop(configure));
    let symbols = Signal::derive(move || seen.block.get().is_some());
    let shown = Signal::derive(move || controls.view.get().shown(symbols.get()));
    let rows = move || {
        let burst = seen.burst.get();
        let period = burst.as_ref().map_or(0.0, |burst| {
            samples_per_symbol(burst.sample_rate, controls.rate.get() as f32)
        });
        measurements(
            shown.get(),
            burst.as_deref(),
            seen.block.get().as_deref(),
            period,
        )
        .into_iter()
        .map(measure_view)
        .collect::<Vec<_>>()
    };
    let hint = move || waiting(shown.get(), seen.burst.get().is_some(), symbols.get());
    view! {
        column(class = "bb__body") {
            {tab_bar(controls, shown, symbols, seen)}
            {plot(scope, seen, shown, hint)}
            row(class = "bb__measures") {{rows}}
        }
    }
}

fn measure_view(row: Measurement) -> AnyView {
    let pair = AnyView::new(view! {
        row(class = "bb__pair") {
            text(class = "bb__key", class:tip = row.hint.is_some()) {{row.label}}
            text(class = "bb__value") {{row.value}}
        }
    });
    match row.hint {
        Some(hint) => tip(hint.to_owned(), "bb__hinted", pair),
        None => pair,
    }
}

#[must_use]
pub fn has_options(view: View, symbols: bool, decimate: bool) -> bool {
    view == View::Eye || view.decimates(symbols) || view.needs_rate(symbols, decimate)
}

fn tab_bar(
    controls: Controls,
    shown: Signal<View>,
    symbols: Signal<bool>,
    seen: Seen,
) -> impl IntoView {
    let tab = move |view: View| {
        view! {
            control(
                class = "bb__tab",
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                class:on = move || shown.get() == view,
                on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                on:click = move |_| controls.view.set(view)
            ) {
                {view.label()}
            }
        }
    };
    let signal_tabs: Vec<_> = SIGNAL_VIEWS.into_iter().map(tab).collect();
    let gear = NodeRef::new();
    let offered =
        Signal::derive(move || has_options(shown.get(), symbols.get(), controls.decimate.get()));
    view! {
        row(class = "bb__tabs", a11y:label = "Baseband view") {
            row(class = "bb__group") {
                {signal_tabs}
                {move || symbols.get().then(|| {
                    let symbol_tabs: Vec<_> = SYMBOL_VIEWS.into_iter().map(tab).collect();
                    view! {
                        row(class = "bb__group") {
                            box(class = "bb__rule") {}
                            {symbol_tabs}
                        }
                    }
                })}
            }
            spacer() {}
            if move || offered.get() {
                control(
                    node_ref = gear,
                    class = "bb__gear",
                    class:on = move || controls.open.get(),
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:label = "View settings",
                    on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                    on:click:stop = move |_| controls.open.update(|open| *open = !*open)
                ) {
                    {zgui::elements::vector().class("bb__icon").document(GEAR_ICON)}
                }
            }
        }
        {move || (offered.get() && controls.open.get()).then(|| AnyView::new(options(controls, shown, symbols, seen, gear)))}
    }
}

fn option_row(name: &'static str, hint: Option<&'static str>, body: AnyView) -> AnyView {
    let label = AnyView::new(
        view! { text(class = "bb__option-name", class:tip = hint.is_some()) {{name}} },
    );
    let label = match hint {
        Some(hint) => tip(hint.to_owned(), "bb__hinted", label),
        None => label,
    };
    AnyView::new(view! {
        row(class = "bb__option") {
            {label}
            spacer() {}
            {body}
        }
    })
}

fn options(
    controls: Controls,
    shown: Signal<View>,
    symbols: Signal<bool>,
    seen: Seen,
    gear: NodeRef,
) -> impl IntoView {
    let root = NodeRef::new();
    dismissable(root, Some(gear), move || controls.open.set(false));
    let nyquist = Signal::derive(move || {
        seen.burst
            .get()
            .map_or(f64::MAX, |burst| f64::from(burst.sample_rate) / 2.0)
    });
    let eye = move || {
        (shown.get() == View::Eye).then(|| {
            option_row(
                "Trace",
                None,
                AnyView::new(segments(
                    vec![(Eye::I, "I"), (Eye::Q, "Q"), (Eye::Frequency, "freq")],
                    controls.eye.into(),
                    move |chosen| controls.eye.set(chosen),
                )),
            )
        })
    };
    let decimate = move || {
        shown.get().decimates(symbols.get()).then(|| {
            option_row(
                "Symbols only",
                Some(SYMBOLS_ONLY_HINT),
                AnyView::new(check(controls.decimate.into(), move |on| {
                    controls.decimate.set(on);
                })),
            )
        })
    };
    let rate = move || {
        shown
            .get()
            .needs_rate(symbols.get(), controls.decimate.get())
            .then(|| {
                let spec = NumberSpec::new("Symbol rate")
                    .limit(NumberLimit::new(
                        MIN_SYMBOL_RATE,
                        nyquist.get(),
                        SYMBOL_RATE_STEP,
                    ))
                    .unit("Bd")
                    .size("kc-num kc-num--narrow bb__rate");
                let value = Signal::derive(move || Some(controls.rate.get()));
                option_row(
                    "Symbol rate",
                    None,
                    AnyView::new(number_field(value, spec, move |next| {
                        if let Some(next) = next {
                            controls.rate.set(next);
                        }
                    })),
                )
            })
    };
    view! {
        column(
            node_ref = root,
            class = "bb__popover",
            tabindex = Focus::Programmatic,
            a11y:role = Role::Dialog,
            a11y:label = "View settings",
            on:pointer_down:stop = move |_| {}
        ) {
            {eye}
            {decimate}
            {rate}
        }
    }
}

fn plot(
    scope: Rc<RefCell<Scope>>,
    seen: Seen,
    shown: Signal<View>,
    hint: impl Fn() -> Option<&'static str> + Copy + 'static,
) -> impl IntoView {
    let frame = NodeRef::new();
    let size = frame.observe_content_size();
    let css = move || {
        let size = size.get();
        let scale = frame.scale().max(0.1);
        (size.width.0 / scale, size.height.0 / scale)
    };
    let labels = {
        let scope = scope.clone();
        move || {
            seen.stamp.track();
            let (width, height) = css();
            scope
                .try_borrow()
                .map(|held| held.labels(width, height))
                .unwrap_or_default()
                .into_iter()
                .map(label_view)
                .collect::<Vec<_>>()
        }
    };
    let states = Memo::new(move |_| {
        seen.block
            .get()
            .map(|block| states_of(&block))
            .unwrap_or_default()
    });
    let row_height = Signal::derive(move || {
        let count = states.with(Vec::len).max(1) as f32;
        (css().1 - STATE_CAPTION_PX) / count
    });
    view! {
        box(class = "bb__plot", node_ref = frame) {
            {raster_view("bb__surface", scope)}
            box(class = "bb__labels") {{labels}}
            if move || shown.get() == View::States {
                {states_view(states, row_height)}
            }
            if move || hint().is_some() {
                text(class = "bb__hint") {{move || hint().unwrap_or_default()}}
            }
        }
    }
}

fn label_view(label: Label) -> AnyView {
    let class = match label.anchor {
        Anchor::Left => "bb__label",
        Anchor::Right => "bb__label bb__label--right",
        Anchor::Centre => "bb__label bb__label--centre",
    };
    AnyView::new(view! {
        text(
            class = class,
            class:backed = label.backed,
            style:left = Some(format!("{:.1}px", label.x)),
            style:top = Some(format!("{:.1}px", label.y))
        ) {{label.text}}
    })
}

#[derive(Clone, Debug, PartialEq)]
struct StateRow {
    state: SymbolState,
    signed: bool,
}

fn states_of(block: &Block) -> Vec<StateRow> {
    let signed = block.plane != SymbolPlane::Complex;
    symbol_states(block)
        .into_iter()
        .map(|state| StateRow { state, signed })
        .collect()
}

#[must_use]
pub fn state_x(error: f32, signed: bool) -> f32 {
    let unit = if signed {
        (error / STATE_EXTENT + 1.0) / 2.0
    } else {
        error / STATE_EXTENT
    };
    unit.clamp(0.0, 1.0)
}

#[must_use]
pub fn state_marks(signed: bool) -> Vec<(f32, bool)> {
    let marks: &[f32] = if signed {
        &[-1.0, 0.0, 1.0]
    } else {
        &[0.0, 1.0]
    };
    marks
        .iter()
        .map(|mark| (state_x(*mark, signed), *mark == 0.0))
        .collect()
}

#[must_use]
pub fn shows_figures(row_height: f32) -> bool {
    row_height >= FIGURES_MIN_PX
}

fn level(value: f32) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn percent(value: f32, sign: bool) -> String {
    if !value.is_finite() {
        return "\u{2013}".to_owned();
    }
    let shown = (value * 100.0).round();
    format!("{}{shown:.0}%", if sign && shown >= 0.0 { "+" } else { "" })
}

#[must_use]
pub fn numerics(state: &SymbolState) -> String {
    format!(
        "{:>2.0}%  {}  {}",
        state.share * 100.0,
        percent(state.mean, true),
        percent(state.sigma, false)
    )
}

fn at(fraction: f32) -> Option<String> {
    Some(format!("{:.2}%", fraction * 100.0))
}

fn marks_view(signed: bool) -> Vec<AnyView> {
    state_marks(signed)
        .into_iter()
        .map(|(x, ideal)| {
            AnyView::new(view! {
                box(class = if ideal { "bb__ideal" } else { "bb__slice" }, style:left = at(x))
            })
        })
        .collect()
}

fn track_view(state: &SymbolState, signed: bool) -> AnyView {
    if state.count == 0 {
        return AnyView::new(view! {
            box(class = "bb__track") {
                {marks_view(signed)}
                box(class = "bb__guide")
            }
        });
    }
    let spread = state.sigma.is_finite().then(|| {
        let low = state_x(state.mean - state.sigma, signed);
        let high = state_x(state.mean + state.sigma, signed);
        view! { box(class = "bb__spread", style:left = at(low), style:width = at((high - low).max(0.005))) }
    });
    let mean = state_x(state.mean, signed);
    let peak = state_x(state.peak, signed);
    let reach = (mean.min(peak), (peak - mean).abs());
    AnyView::new(view! {
        box(class = "bb__track") {
            {marks_view(signed)}
            box(class = "bb__guide") {}
            {spread}
            box(class = "bb__reach", style:left = at(reach.0), style:width = at(reach.1))
            box(class = "bb__peak", style:left = at(peak))
            box(class = "bb__mean", style:left = at(mean))
        }
    })
}

fn state_view(row: StateRow, figures: bool) -> AnyView {
    let StateRow { state, signed } = row;
    let ideal = if signed {
        level(state.i)
    } else {
        format!("{},{}", level(state.i), level(state.q))
    };
    let readout = if state.count == 0 {
        Some(String::from("never decided"))
    } else {
        figures.then(|| numerics(&state))
    };
    let track = track_view(&state, signed);
    AnyView::new(view! {
        row(class = "bb__state") {
            text(class = "bb__bits") {{state.bits}}
            text(class = "bb__level") {{ideal}}
            {track}
            text(class = "bb__figures") {{readout.unwrap_or_default()}}
        }
    })
}

fn captions_view(signed: bool) -> impl IntoView {
    let marks: Vec<_> = state_marks(signed)
        .into_iter()
        .map(|(x, ideal)| {
            view! { text(class = "bb__caption", style:left = at(x)) {{if ideal { "ideal" } else { "slice" }}} }
        })
        .collect();
    view! {
        row(class = "bb__captions") {
            box(class = "bb__caption-gap")
            box(class = "bb__caption-track") {{marks}}
            text(class = "bb__figures") {"share offset spread"}
        }
    }
}

fn states_view(states: Memo<Vec<StateRow>>, row_height: Signal<f32>) -> impl IntoView {
    let rows = move || {
        let figures = shows_figures(row_height.get());
        states
            .get()
            .into_iter()
            .map(|row| state_view(row, figures))
            .collect::<Vec<_>>()
    };
    let captions = move || {
        states
            .with(|rows| rows.first().map(|row| row.signed))
            .map(|signed| AnyView::new(captions_view(signed)))
    };
    view! {
        column(class = "bb__states") {
            column(class = "bb__state-rows") {{rows}}
            {captions}
        }
    }
}

const SHEET: &str = css!(
    r#"
.bb { position: relative; flex: 1 1 334px; min-height: 0; flex-direction: column; background-color: #08090b; border-bottom-left-radius: 6px; border-bottom-right-radius: 6px; overflow: hidden; }
.bb__empty { flex: 1 1 auto; min-height: 160px; align-items: center; justify-content: center; display: flex; }
.bb__body { flex: 1 1 auto; min-height: 0; flex-direction: column; }
.bb__tabs { flex: 0 0 auto; align-items: stretch; padding: 0 4px; border-bottom: 1px solid var(--line); background-color: var(--panel); }
.bb__group { flex-wrap: wrap; align-items: stretch; min-width: 0; }
.bb__rule { width: 1px; margin: 6px 4px; background-color: var(--line); }
.bb__tab { padding: 4px 7px; font-family: var(--mono); font-size: 10px; color: var(--ink-faint); border-bottom: 2px solid transparent; }
.bb__tab:hover { color: var(--ink); }
.bb__tab.on { color: var(--accent); border-bottom-color: var(--accent); }
.bb__gear { align-self: center; align-items: center; justify-content: center; width: 22px; height: 20px; border-radius: 3px; color: var(--ink-faint); }
.bb__gear:hover { background-color: var(--panel-2); color: var(--ink); }
.bb__gear.on { color: var(--accent); }
.bb__icon { width: 12px; height: 12px; }
.bb__popover { position: absolute; z-index: 30; right: 4px; top: 26px; width: 256px; gap: 8px; padding: 10px 12px; border: 1px solid var(--line); border-radius: 6px; background-color: var(--panel); box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5); }
.bb__option { align-items: center; gap: 8px; min-height: 26px; }
.bb__option-name { font-family: var(--mono); font-size: 10px; letter-spacing: 0.08em; text-transform: uppercase; color: var(--ink-dim); }
.bb__option-name.tip, .bb__key.tip { text-decoration: underline dotted; text-decoration-color: var(--line-strong); cursor: help; }
.bb__rate { width: 104px; }
.bb__plot { position: relative; flex: 1 1 auto; min-height: 120px; background-color: #08090b; }
.bb__surface { position: absolute; left: 0; top: 0; width: 100%; height: 100%; }
.bb__labels { position: absolute; left: 0; top: 0; right: 0; bottom: 0; pointer-events: none; }
.bb__label {
    position: absolute;
    height: 12px;
    margin-top: -6px;
    padding: 0 2px;
    font-family: var(--mono);
    font-size: 10px;
    line-height: 12px;
    white-space: nowrap;
    color: var(--plot-ink-dim);
}
.bb__label.backed { background-color: rgba(8, 9, 11, 0.75); }
.bb__label--right { transform: translateX(-100%); }
.bb__label--centre { transform: translateX(-50%); }
.bb__hint {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    margin-top: -7px;
    text-align: center;
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--plot-ink-dim);
    pointer-events: none;
}
.bb__measures { flex: 0 0 auto; flex-wrap: wrap; gap: 2px 14px; padding: 6px 10px; border-top: 1px solid var(--line); background-color: var(--panel); }
.bb__pair { gap: 6px; align-items: baseline; }
.bb__key { font-family: var(--mono); font-size: 9px; letter-spacing: 0.09em; text-transform: uppercase; color: var(--ink-faint); }
.bb__value { font-family: var(--mono); font-size: 11px; color: var(--ink); }
.bb__states { position: absolute; left: 0; top: 0; right: 0; bottom: 0; padding: 6px 0 0 0; flex-direction: column; background-color: #08090b; font-family: var(--mono); font-size: 10px; color: var(--plot-ink-dim); }
.bb__state-rows { flex: 1 1 auto; min-height: 0; flex-direction: column; }
.bb__state { flex: 1 1 0; min-height: 4px; gap: 8px; align-items: center; padding: 0 8px 0 4px; overflow: hidden; }
.bb__bits { width: 30px; flex: 0 0 auto; }
.bb__level { width: 36px; flex: 0 0 auto; text-align: right; }
.bb__figures { width: 124px; flex: 0 0 auto; white-space: pre; }
.bb__track { position: relative; flex: 1 1 auto; align-self: stretch; }
.bb__guide { position: absolute; left: 0; right: 0; top: 50%; height: 1px; background-color: rgb(50, 50, 50); }
.bb__ideal { position: absolute; top: 0; bottom: 0; width: 1px; background-color: var(--plot-ink-dim); }
.bb__slice { position: absolute; top: 0; bottom: 0; width: 0; border-left: 1px dashed rgb(50, 50, 50); }
.bb__spread { position: absolute; top: 24%; bottom: 24%; max-height: 18px; background-color: rgba(102, 229, 255, 0.25); }
.bb__reach { position: absolute; top: 50%; height: 1px; background-color: rgba(240, 180, 80, 0.65); }
.bb__peak { position: absolute; top: 33%; bottom: 33%; width: 1px; background-color: rgba(240, 180, 80, 0.65); }
.bb__mean { position: absolute; top: 24%; bottom: 24%; width: 2px; margin-left: -1px; background-color: rgb(102, 229, 255); }
.bb__captions { flex: 0 0 16px; height: 16px; gap: 8px; align-items: center; padding: 0 8px 0 4px; }
.bb__caption-gap { width: 74px; flex: 0 0 auto; }
.bb__caption-track { position: relative; flex: 1 1 auto; align-self: stretch; }
.bb__caption { position: absolute; top: 2px; transform: translateX(-50%); }
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_error_lands_between_the_slices() {
        assert!((state_x(0.0, true) - 0.5).abs() < 1e-6);
        assert!((state_x(STATE_EXTENT, true) - 1.0).abs() < 1e-6);
        assert_eq!(state_x(-5.0, true), 0.0);
        assert_eq!(state_x(0.0, false), 0.0);
        assert_eq!(state_x(5.0, false), 1.0);
    }

    #[test]
    fn a_state_reads_its_share_offset_and_spread() {
        let state = SymbolState {
            bits: "10".into(),
            i: 1.0,
            q: 0.0,
            count: 2,
            share: 0.25,
            mean: 0.12,
            sigma: f32::NAN,
            peak: 0.2,
        };
        assert_eq!(numerics(&state), "25%  +12%  \u{2013}");
        let rare = SymbolState {
            share: 0.04,
            ..state
        };
        assert!(numerics(&rare).starts_with(" 4%"));
    }

    #[test]
    fn signed_states_mark_both_slices_around_the_ideal() {
        let marks = state_marks(true);
        assert_eq!(marks.len(), 3);
        assert_eq!(marks.iter().filter(|(_, ideal)| *ideal).count(), 1);
        assert!((marks[1].0 - 0.5).abs() < 1e-6);
        let unsigned = state_marks(false);
        assert_eq!(unsigned[0], (0.0, true));
        assert!((unsigned[1].0 - 1.0 / STATE_EXTENT).abs() < 1e-6);
    }

    #[test]
    fn figures_hide_once_rows_get_too_thin() {
        assert!(shows_figures(12.0));
        assert!(!shows_figures(11.5));
    }

    #[test]
    fn the_gear_shows_only_where_a_view_has_options() {
        assert!(has_options(View::Eye, true, false));
        assert!(has_options(View::Constellation, false, false));
        assert!(has_options(View::Levels, false, false));
        assert!(!has_options(View::Spectrum, false, true));
        assert!(!has_options(View::Quality, true, true));
    }
}
