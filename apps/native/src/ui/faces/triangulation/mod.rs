pub mod heat;
pub mod logic;

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use jiff::Timestamp;
use sdrmm_wire::{
    frame::{FrameKind, FusionGridFrame},
    fusion::{DfFusionState, FusionDecay, NavMode, TriangulationParams},
    limits::FUSION_LIMITS,
    ws::ServerEvent,
};
use zgui::{prelude::*, reactive::RenderEffect, vocab::SharedString};

use super::{processor, scope::colormap::Colormap};
use crate::{
    store::Store,
    ui::{
        kit_raster::{Bounds, button, number, raster_view},
        widgets::{check, row_field},
    },
};
use heat::{HeatScene, Marks, owned};
use logic::{
    NAV_OPTIONS, NO_BEARINGS, NO_SOURCES, decay_choice, decay_option_title, decay_options,
    decay_with, fade_title, fusion_sources, half_life_of, nav_title, readout_rows, settings_mut,
    settings_of, station_cells, stats, subtitle,
};

const AGE_TICK: Duration = Duration::from_secs(1);
const STATION_HEADS: [&str; 5] = ["Station", "Last", "\u{b1}", "Align", "Age"];
const ALIGN_TITLE: &str = "Heading offset learned from crossing bearings";

type Fusion = Memo<Option<DfFusionState>>;
type Settings = Memo<Option<TriangulationParams>>;

fn path(node: &str) -> String {
    format!("/api/fusion/{node}")
}

fn seed(store: Store, node: String) {
    zgui::task::spawn_local(async move {
        match store.api().get::<DfFusionState>(&path(&node)).await {
            Ok(state) => store
                .arrays
                .update(|book| Arc::make_mut(book).seed_fusion(&node, state)),
            Err(error) => store.flag_refusal(
                &node,
                format!("Cannot load the bearings: {error}"),
                crate::ui::faces::channel::alert::RefusalSource::Action,
            ),
        }
    });
}

fn clear(store: Store, node: String, pending: RwSignal<bool>) {
    pending.set(true);
    zgui::task::spawn_local(async move {
        let done = store.api().delete(&path(&node)).await;
        pending.try_set(false);
        match done {
            Ok(()) => store
                .arrays
                .update(|book| Arc::make_mut(book).clear_fusion(&node)),
            Err(error) => store.fail("Cannot clear the bearings", &error),
        }
    });
}

pub fn face(store: Store, node: String) -> impl IntoView {
    let titled = node.clone();
    install_stylesheet("triangulation-face", SHEET);
    install_stylesheet("processor-face", processor::SHEET);
    seed(store, node.clone());
    let fusion: Fusion = {
        let node = node.clone();
        Memo::new(move |_| store.arrays.with(|book| book.fusion.get(&node).cloned()))
    };
    let sources = {
        let node = node.clone();
        Memo::new(move |_| store.graph.with(|graph| fusion_sources(graph, &node)))
    };
    let head = Signal::derive(move || {
        let reporting = fusion.with(|held| held.as_ref().map_or(0, |held| held.stations.len()));
        subtitle(reporting, sources.get())
    });
    let settings = processor::settings(store, node.clone(), settings_of);
    let plot = heat_box(store, node.clone(), fusion, sources);
    let edits = controls(store, node.clone(), settings, fusion);
    let foot = footer(store, node, fusion);
    processor::head(store, titled, head);
    view! {
        column(class = "face tri") {
            {plot}
            {edits}
            {readouts(fusion)}
            {stations(fusion)}
            {foot}
        }
    }
}

fn marks_of(fusion: Option<&DfFusionState>) -> Marks {
    fusion.map_or_else(Marks::default, |fusion| Marks {
        estimate: fusion.estimate,
        emitters: fusion.emitters.clone(),
        stations: fusion.stations.clone(),
    })
}

fn heat_box(store: Store, node: String, fusion: Fusion, sources: Memo<usize>) -> impl IntoView {
    let known = Memo::new(move |_| fusion.with(Option::is_some));
    let shown = RwSignal::new(false);
    let refused = RwSignal::new(None::<&'static str>);
    let hint = move || {
        let waiting = !shown.get() && refused.get().is_none();
        let text = if sources.get() == 0 {
            NO_SOURCES
        } else {
            NO_BEARINGS
        };
        waiting.then(|| view! { text(class = "kit-note tri__over") {{text}} })
    };
    let refusal = move || {
        refused.get().map(|reason| {
            view! {
                text(
                    class = "kit-alert tri__over",
                    a11y:description = SharedString::from("The server refused this plot")
                ) {{reason}}
            }
        })
    };
    view! {
        box(class = "tri__heat", a11y:label = "Bearing heat") {
            {move || known.get().then(|| AnyView::new(plot(store, node.clone(), fusion, shown, refused)))}
            {hint}
            {refusal}
        }
    }
}

fn plot(
    store: Store,
    node: String,
    fusion: Fusion,
    shown: RwSignal<bool>,
    refused: RwSignal<Option<&'static str>>,
) -> impl IntoView {
    let scene = Rc::new(RefCell::new(HeatScene::new(Colormap::Inferno)));
    let marking = {
        let scene = scene.clone();
        RenderEffect::new(move |_| {
            let marks = fusion.with(|held| marks_of(held.as_ref()));
            if let Ok(mut held) = scene.try_borrow_mut() {
                held.mark(marks);
            }
        })
    };
    on_cleanup_local(move || {
        drop(marking);
        shown.try_set(false);
    });
    listen(store, node, scene.clone(), shown, refused);
    raster_view("tri__plot", scene)
}

fn listen(
    store: Store,
    node: String,
    scene: Rc<RefCell<HeatScene>>,
    shown: RwSignal<bool>,
    refused: RwSignal<Option<&'static str>>,
) {
    let ours = node.clone();
    store.on_event(move |event| {
        if let ServerEvent::SurfaceRefused { node: from, reason } = event
            && *from == ours
        {
            refused.set(Some(reason.label()));
        }
    });
    let named = node.clone();
    processor::surface(store, node, FrameKind::FusionGrid, None, move |bytes| {
        match FusionGridFrame::decode(bytes) {
            Ok(frame) => {
                if let Ok(mut held) = scene.try_borrow_mut() {
                    held.show(owned(&frame));
                    shown.set(true);
                }
            }
            Err(error) => {
                tracing::warn!(node = %named, %error, "a fusion grid frame did not decode")
            }
        }
    });
}

fn titled<T>(
    options: Vec<(T, &'static str, String)>,
    chosen: Signal<T>,
    on_pick: impl Fn(T) + Clone + 'static,
) -> impl IntoView
where
    T: Clone + PartialEq + Send + Sync + 'static,
{
    let items: Vec<_> = options
        .into_iter()
        .map(|(value, label, title)| {
            let on_pick = on_pick.clone();
            let picked = value.clone();
            view! {
                control(
                    class = "seg__item",
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:description = SharedString::from(title),
                    class:on = move || chosen.get() == value,
                    on:click:stop = move |_| on_pick(picked.clone())
                ) {
                    {label}
                }
            }
        })
        .collect();
    view! { row(class = "seg") {{items}} }
}

fn described(title: impl Fn() -> String + 'static, body: impl IntoView + 'static) -> impl IntoView {
    view! { box(a11y:description = move || SharedString::from(title())) {{body}} }
}

fn read<T>(
    settings: Settings,
    pick: impl Fn(&TriangulationParams) -> T + Send + Sync + 'static,
) -> Signal<T>
where
    T: Default + Clone + Send + Sync + 'static,
{
    Signal::derive(move || settings.with(|held| held.as_ref().map(&pick).unwrap_or_default()))
}

fn controls(store: Store, node: String, settings: Settings, fusion: Fusion) -> impl IntoView {
    let write = move |change: Box<dyn FnOnce(&mut TriangulationParams)>| {
        processor::edit(store, &node, settings_mut, change);
    };
    let fading = fade(settings, fusion, write.clone());
    let guiding = guide(settings, write.clone());
    let sizes = numbers(settings, write.clone());
    let align = {
        let write = write.clone();
        move |on: bool| write(Box::new(move |params| params.align = on))
    };
    let emitters = move |count: f64| {
        write(Box::new(move |params| params.max_emitters = count as u8));
    };
    let limits = FUSION_LIMITS.emitters;
    view! {
        column(class = "params") {
            {fading}
            {guiding}
            {sizes}
            {described(
                || "Learn each station's heading offset from crossing bearings".to_owned(),
                row_field("Align", check(read(settings, |params| params.align), align)),
            )}
            {described(
                || "Most transmitters to find at once".to_owned(),
                row_field("Emitters", number(
                    "Emitters",
                    read(settings, |params| f64::from(params.max_emitters)),
                    Bounds::whole(f64::from(limits.min), f64::from(limits.max)),
                    emitters,
                )),
            )}
        }
    }
}

fn fade(
    settings: Settings,
    fusion: Fusion,
    write: impl Fn(Box<dyn FnOnce(&mut TriangulationParams)>) + Clone + 'static,
) -> impl IntoView {
    let decay = read(settings, |params| params.decay);
    let options = decay_options()
        .into_iter()
        .map(|(value, label)| (value, label, decay_option_title(value)))
        .collect();
    let pick = {
        let write = write.clone();
        move |picked: FusionDecay| {
            write(Box::new(move |params| {
                params.decay = decay_with(picked, params.decay);
            }));
        }
    };
    let chosen = Signal::derive(move || decay_choice(decay.get()));
    let custom = Memo::new(move |_| half_life_of(decay.get()).is_some());
    let half_life = move || {
        let write = write.clone();
        custom.get().then(|| {
            let seconds = Signal::derive(move || half_life_of(decay.get()).map_or(0.0, f64::from));
            let limits = FUSION_LIMITS.half_life_s;
            AnyView::new(described(
                || "Old bearings count half after this".to_owned(),
                row_field(
                    "Half life s",
                    number(
                        "Half life",
                        seconds,
                        Bounds::whole(f64::from(limits.min), f64::from(limits.max)),
                        move |value| {
                            write(Box::new(move |params| {
                                params.decay = FusionDecay::HalfLife {
                                    seconds: value as u32,
                                };
                            }));
                        },
                    ),
                ),
            ))
        })
    };
    view! {
        column(class = "params") {
            {described(move || fusion.with(|held| fade_title(held.as_ref())), row_field("Fade", titled(options, chosen, pick)))}
            {half_life}
        }
    }
}

fn guide(
    settings: Settings,
    write: impl Fn(Box<dyn FnOnce(&mut TriangulationParams)>) + Clone + 'static,
) -> impl IntoView {
    let options = NAV_OPTIONS
        .into_iter()
        .map(|(value, label)| {
            (
                value,
                label,
                nav_title(value).unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let pick = move |nav: NavMode| write(Box::new(move |params| params.nav = nav));
    described(
        || "Where to send a wired vehicle".to_owned(),
        row_field(
            "Guide",
            titled(options, read(settings, |params| params.nav), pick),
        ),
    )
}

fn numbers(
    settings: Settings,
    write: impl Fn(Box<dyn FnOnce(&mut TriangulationParams)>) + Clone + 'static,
) -> impl IntoView {
    let limits = FUSION_LIMITS;
    let extent = {
        let write = write.clone();
        move |km: f64| write(Box::new(move |params| params.extent_km = km))
    };
    let probe = {
        let write = write.clone();
        move |km: f64| write(Box::new(move |params| params.probe_km = km))
    };
    let confidence = move |value: f64| {
        write(Box::new(move |params| params.min_confidence = value as f32));
    };
    view! {
        column(class = "params") {
        {described(|| "Half width of the search grid".to_owned(), row_field("Extent km", number(
            "Extent",
            read(settings, |params| params.extent_km),
            Bounds::new(limits.extent_km.min, limits.extent_km.max),
            extent,
        )))}
        {described(|| "How far across a single bearing to drive".to_owned(), row_field("Probe km", number(
            "Probe",
            read(settings, |params| params.probe_km),
            Bounds::new(limits.probe_km.min, limits.probe_km.max),
            probe,
        )))}
        {described(|| "Bearings below this are refused".to_owned(), row_field("Min conf", number(
            "Min conf",
            read(settings, |params| f64::from(params.min_confidence)),
            Bounds::new(f64::from(limits.min_confidence.min), f64::from(limits.min_confidence.max)),
            confidence,
        )))}
        }
    }
}

fn readouts(fusion: Fusion) -> impl IntoView {
    move || {
        fusion
            .with(|held| readout_rows(held.as_ref()))
            .into_iter()
            .map(|(label, value, title)| {
                view! {
                    row(class = "tri__read", a11y:description = SharedString::from(title.unwrap_or_default())) {
                        text(class = "tri__read_label") {{label}}
                        text(class = "tri__read_value") {{value}}
                    }
                }
            })
            .collect::<Vec<_>>()
    }
}

fn stations(fusion: Fusion) -> impl IntoView {
    let now = RwSignal::new(Timestamp::now());
    let tick = set_interval(AGE_TICK, move || now.set(Timestamp::now()));
    on_cleanup_local(move || drop(tick));
    move || {
        let list = fusion.with(|held| {
            held.as_ref()
                .map(|held| held.stations.clone())
                .unwrap_or_default()
        });
        if list.is_empty() {
            return None;
        }
        let at = now.get();
        let heads: Vec<_> = STATION_HEADS
            .into_iter()
            .map(|head| view! { text(class = "tri__cell") {{head}} })
            .collect();
        let rows: Vec<_> = list
            .iter()
            .map(|station| AnyView::new(station_row(station_cells(station, at))))
            .collect();
        Some(view! {
            column(class = "tri__table", a11y:label = "Stations") {
                row(class = "tri__row head") {{heads}}
                {rows}
            }
        })
    }
}

fn station_row(cells: [String; 5]) -> impl IntoView {
    let [id, last, sigma, align, age] = cells;
    let title = SharedString::from(id.clone());
    view! {
        row(class = "tri__row") {
            text(class = "tri__cell tri__id", a11y:description = title) {{id}}
            text(class = "tri__cell") {{last}}
            text(class = "tri__cell") {{sigma}}
            text(class = "tri__cell", a11y:description = SharedString::from(ALIGN_TITLE)) {{align}}
            text(class = "tri__cell") {{age}}
        }
    }
}

fn footer(store: Store, node: String, fusion: Fusion) -> impl IntoView {
    let pending = RwSignal::new(false);
    let shown = move || {
        fusion
            .with(|held| stats(held.as_ref()))
            .into_iter()
            .map(|stat| {
                view! {
                    row(class = "pf__stat danger", a11y:description = SharedString::from(stat.title)) {
                        text(class = "pf__stat_label") {{stat.label}}
                        text(class = "pf__stat_value") {{processor::count(stat.count)}}
                    }
                }
            })
            .collect::<Vec<_>>()
    };
    view! {
        row(class = "tri__foot") {
            row(class = "tri__stats") {{shown}}
            spacer() {}
            box(a11y:description = SharedString::from("Throw away every bearing and start again")) {
                {button("Clear", pending.into(), move || clear(store, node.clone(), pending))}
            }
        }
    }
}

const SHEET: &str = css!(
    r#"
.tri__heat { position: relative; height: 180px; flex: 0 0 auto; background-color: #08090b; }
.tri__plot { position: absolute; left: 0; top: 0; right: 0; bottom: 0; }
.tri__over { position: absolute; left: 0; right: 0; top: 80px; text-align: center; }
.tri__read { gap: 10px; align-items: baseline; }
.tri__read_label {
    flex: 0 0 auto;
    width: 84px;
    font-family: var(--mono);
    font-size: 9px;
    letter-spacing: 0.09em;
    text-transform: uppercase;
    color: var(--ink-faint);
}
.tri__read_value { flex: 1 1 auto; min-width: 0; font-family: var(--mono); font-size: 11px; color: var(--ink); }
.tri__table { border-top: 1px solid var(--line); font-family: var(--mono); font-size: 10px; }
.tri__row { gap: 0; }
.tri__row.head { color: var(--ink-faint); }
.tri__cell { flex: 1 1 0; min-width: 0; padding: 2px 4px; color: var(--ink); }
.tri__row.head .tri__cell { color: var(--ink-faint); }
.tri__id { flex: 2 1 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.tri__foot { align-items: center; gap: 6px; padding-top: 6px; border-top: 1px solid var(--line); }
.tri__stats { flex-wrap: wrap; gap: 6px; }
"#
);
