pub mod ionosonde;
pub mod layers;
pub mod model;
pub mod sondes;

use std::{cell::RefCell, collections::HashMap, sync::Arc, time::Duration};

use sdrmm_wire::{
    DecoderLogResponse,
    decode::DecodedRecord,
    patch::NodeBody,
    propagation::{IonosondeStation, PropagationNode},
    ws::ServerEvent,
};
use zgui::prelude::*;
use zgui::reactive::{ArcRwSignal, RenderEffect};

use self::{
    ionosonde::{Agreement, Comparison, Forecast},
    layers::Layer,
    model::{Cell, Observation, Options, Summary},
};
use crate::{
    store::Store,
    ui::{
        kit_audio::titled,
        kit_maps::{
            SHEET, armed_clear, encode,
            feed::{Feed, Topic},
            iso_of, mhz, now_ms, readout, ticker, trail,
            wiring::{channel_type_of, event_sources},
        },
        map::{self, ACCENT, Frame, Geo, MapProps, Overlay},
        widgets::{check, pick, segments},
    },
};

const REDRAW: Duration = Duration::from_secs(2);
const MUF_HINT: &str = "Measured MUF is a floor: the highest frequency decoded over each path, projected onto a 3000 km hop";
const HISTORY_LIMIT: usize = 2_000;
const TABLE_ROWS: usize = 12;
const RECEIVER_ZOOM: f64 = 3.0;

const HALF_LIVES: [(u32, &str); 7] = [
    (5, "5 min"),
    (15, "15 min"),
    (30, "30 min"),
    (60, "1 h"),
    (120, "2 h"),
    (360, "6 h"),
    (720, "12 h"),
];

const HEIGHTS: [(u32, &str); 5] = [
    (110, "110 km (E)"),
    (250, "250 km (F2 low)"),
    (300, "300 km (F2)"),
    (350, "350 km (F2 high)"),
    (400, "400 km"),
];

#[derive(Clone, Debug, Default, PartialEq)]
struct Session {
    observations: Vec<Observation>,
    cleared_at: i64,
}

thread_local! {
    static SESSIONS: RefCell<HashMap<String, Session>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Input {
    node: String,
    device_set: u32,
    channel: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Computed {
    overlay: Arc<Overlay>,
    summary: Summary,
    cells: Vec<Cell>,
    comparisons: Vec<Comparison>,
    agreement: Agreement,
    overhead: Option<Forecast>,
    live: usize,
}

fn settings_of(store: Store, node: &str) -> PropagationNode {
    store
        .graph
        .get()
        .node(node)
        .and_then(|found| match &found.body {
            NodeBody::Propagation(settings) => Some(*settings),
            _ => None,
        })
        .unwrap_or_default()
}

fn update(store: Store, node: String, change: impl FnOnce(&mut PropagationNode) + 'static) {
    store.edit_graph(move |graph| {
        if let Some(found) = graph.nodes.iter_mut().find(|found| found.id == node)
            && let NodeBody::Propagation(held) = &mut found.body
        {
            change(held);
        }
    });
}

fn inputs_of(store: Store, node: &str) -> Vec<Input> {
    let graph = store.graph.get();
    event_sources(&graph, node)
        .into_iter()
        .filter(|source| {
            channel_type_of(&graph, source)
                .and_then(|kind| store.descriptor_of(&kind)?.decoder_kind)
                .is_some_and(|kind| model::is_propagation_kind(&kind))
        })
        .filter_map(|source| {
            let channel = store.channel_of(&source)?;
            Some(Input {
                device_set: store.device_set_of(&source)?,
                channel: channel.id,
                node: source,
            })
        })
        .collect()
}

fn history_path(kind: &str, inputs: &[Input], since: i64) -> String {
    let nodes: Vec<&str> = inputs.iter().map(|input| input.node.as_str()).collect();
    let sources: Vec<String> = inputs
        .iter()
        .map(|input| format!("{}:{}", input.device_set, input.channel))
        .collect();
    format!(
        "/api/decoderlog?kind={kind}&nodes={}&sources={}&since={}&limit={HISTORY_LIMIT}",
        encode(&nodes.join(",")),
        encode(&sources.join(",")),
        encode(&iso_of(since)),
    )
}

fn observations_of(
    records: &[DecodedRecord],
    wanted: &[Input],
    receiver: Geo,
    height_km: f64,
) -> Vec<Observation> {
    records
        .iter()
        .filter(|record| {
            wanted.iter().any(|input| {
                input.device_set == record.device_set && input.channel == record.channel
            })
        })
        .filter_map(|record| model::observation_of(record, receiver, height_km))
        .collect()
}

fn compute(
    held: &[Observation],
    cleared_at: i64,
    settings: PropagationNode,
    receiver: Option<Geo>,
    sondes: &[IonosondeStation],
    layer: Layer,
    now: i64,
) -> Computed {
    let options = Options {
        half_life_minutes: f64::from(settings.half_life_minutes),
        now_ms: now,
    };
    let observations = model::live(held, options, cleared_at);
    let cells = model::cells(&observations, options);
    let paths = match (settings.show_paths, receiver) {
        (true, Some(receiver)) => model::paths(&observations, receiver, options),
        _ => Vec::new(),
    };
    let sondes: &[IonosondeStation] = if settings.compare_forecast {
        sondes
    } else {
        &[]
    };
    let comparisons = ionosonde::compare(&cells, sondes);
    Computed {
        overlay: Arc::new(layers::overlay(&cells, &paths, sondes, layer)),
        summary: model::summary(&observations),
        agreement: ionosonde::agreement(&comparisons),
        overhead: receiver
            .and_then(|at| ionosonde::forecast_at(sondes, at, ionosonde::FORECAST_RADIUS_KM)),
        comparisons,
        cells,
        live: observations.len(),
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("kit-maps", SHEET);
    let feed = Feed::get();
    feed.listen(store);
    let settings = {
        let node = node.clone();
        Memo::new(move |_| settings_of(store, &node))
    };
    let inputs = {
        let node = node.clone();
        Memo::new(move |_| inputs_of(store, &node))
    };
    let positions = {
        let node = node.clone();
        Memo::new(move |_| trail::positions_of(&store.graph.get(), &node))
    };
    let receiver = {
        let feed = feed.clone();
        Signal::derive_local(move || {
            let nodes = positions.get();
            feed.with(Topic::Positions, |held| {
                nodes
                    .iter()
                    .find_map(|node| model::receiver_of(held.tracks.get(node)?.fix.as_ref()))
            })
        })
    };
    let session = RwSignal::new(
        SESSIONS.with(|sessions| sessions.borrow().get(&node).cloned().unwrap_or_default()),
    );
    let keeping = {
        let node = node.clone();
        RenderEffect::new(move |_| {
            let held = session.get();
            SESSIONS.with(|sessions| sessions.borrow_mut().insert(node.clone(), held));
        })
    };
    on_cleanup_local(move || drop(keeping));
    let layer = RwSignal::new(Layer::Activity);
    let now = ticker(REDRAW);
    let seed = RwSignal::new(Vec::<DecodedRecord>::new());
    let compare = Memo::new(move |_| settings.get().compare_forecast);
    let report = sondes::answer();

    listen(store, inputs, receiver, settings, session);
    load_history(store, inputs, seed);
    sondes::follow(store, compare);

    let computed = RwSignal::new(Computed::default());
    let computing = RenderEffect::new(move |_| {
        let at = now.get();
        let chosen = settings.get();
        let here = receiver.get();
        let height = f64::from(chosen.reflection_height_km);
        let seeded = match here {
            Some(receiver) => {
                seed.with(|records| observations_of(records, &inputs.get(), receiver, height))
            }
            None => Vec::new(),
        };
        let (held, cleared_at) = session.with(|session| {
            (
                model::merge(&seeded, &session.observations, model::OBSERVATION_CAPACITY)
                    .unwrap_or(seeded),
                session.cleared_at,
            )
        });
        let sondes = report.with(|report: &sondes::Answer| {
            report
                .as_ref()
                .and_then(|report| report.as_ref().ok())
                .map(|report| report.stations.clone())
                .unwrap_or_default()
        });
        computed.set(compute(
            &held,
            cleared_at,
            chosen,
            here,
            &sondes,
            layer.get(),
            at,
        ));
    });
    on_cleanup_local(move || drop(computing));

    let active = {
        let node = node.clone();
        Signal::derive(move || store.selected.get().as_deref() == Some(node.as_str()))
    };
    let overlay = {
        let feed = feed.clone();
        Signal::derive_local(move || {
            let nodes = positions.get();
            let mut drawn = feed.with(Topic::Positions, |held| {
                let tracks: Vec<_> = nodes
                    .iter()
                    .filter_map(|node| held.tracks.get(node))
                    .collect();
                trail::overlay(&tracks)
            });
            drawn.extend((*computed.with(|computed| computed.overlay.clone())).clone());
            Arc::new(drawn)
        })
    };
    let shown = RwSignal::new(Arc::new(Overlay::default()));
    let frame = RwSignal::new(None::<Arc<Frame>>);
    let publishing = RenderEffect::new(move |_| {
        shown.set(overlay.get());
        let at = receiver.get();
        frame.set(at.map(|at| {
            Arc::new(Frame {
                points: vec![at],
                max_zoom: RECEIVER_ZOOM,
            })
        }));
    });
    on_cleanup_local(move || drop(publishing));
    let props = MapProps {
        overlay: shown.into(),
        active,
        frame: frame.into(),
        on_pick: None,
    };
    let chrome = AnyView::new(legend(positions));
    view! {
        column(class = "geo propagation") {
            {map::map(store, props, chrome)}
            {controls(store, node.clone(), settings, layer)}
            {readouts(settings, sondes::answer(), computed)}
            {fault(compare, sondes::answer())}
            {table(settings, computed)}
            {foot(session, computed)}
        }
    }
}

fn legend(positions: Memo<Vec<String>>) -> impl IntoView {
    let feed = Feed::get();
    let count = move || {
        let nodes = positions.get();
        feed.with(Topic::Positions, |held| {
            nodes
                .iter()
                .filter_map(|node| held.tracks.get(node))
                .map(|track| track.history.len())
                .sum::<usize>()
                .to_string()
        })
    };
    move || {
        (!positions.get().is_empty()).then(|| {
            let count = count.clone();
            view! {
                column(class = "geo__legend") {
                    column(class = "geo__legend-box") {
                        {trail::legend_row(ACCENT, "GPS trail", count)}
                    }
                }
            }
        })
    }
}

fn listen(
    store: Store,
    inputs: Memo<Vec<Input>>,
    receiver: Signal<Option<Geo>, LocalStorage>,
    settings: Memo<PropagationNode>,
    session: RwSignal<Session>,
) {
    store.on_event(move |event| {
        let records: &[DecodedRecord] = match event {
            ServerEvent::Decoded(record) => std::slice::from_ref(record.as_ref()),
            ServerEvent::DecodedBacklog { records } => records,
            _ => return,
        };
        let Some(here) = receiver.get_untracked() else {
            return;
        };
        let height = f64::from(settings.get_untracked().reflection_height_km);
        let fresh = observations_of(records, &inputs.get_untracked(), here, height);
        if fresh.is_empty() {
            return;
        }
        let merged = session.with_untracked(|session| {
            model::merge(&session.observations, &fresh, model::OBSERVATION_CAPACITY)
        });
        if let Some(merged) = merged {
            session.update(|session| session.observations = merged);
        }
    });
}

fn load_history(store: Store, inputs: Memo<Vec<Input>>, seed: RwSignal<Vec<DecodedRecord>>) {
    let since = now_ms() - model::HISTORY_WINDOW_MS;
    let loading = RenderEffect::new(move |_| {
        let wanted = inputs.get();
        if wanted.is_empty() {
            seed.set(Vec::new());
            return;
        }
        zgui::task::spawn_local(async move {
            let mut records = Vec::new();
            for kind in model::PROPAGATION_KINDS {
                match store
                    .api()
                    .get::<DecoderLogResponse>(&history_path(kind, &wanted, since))
                    .await
                {
                    Ok(response) => {
                        records.extend(response.entries.into_iter().map(|entry| DecodedRecord {
                            origin: entry.origin,
                            device_set: entry.device_set,
                            channel: entry.channel,
                            at: entry.at,
                            freq_hz: entry.freq_hz,
                            event: entry.event,
                            sinks: Vec::new(),
                        }))
                    }
                    Err(error) => store.say(format!("cannot read the {kind} history: {error}")),
                }
            }
            seed.set(records);
        });
    });
    on_cleanup_local(move || drop(loading));
}

fn controls(
    store: Store,
    node: String,
    settings: Memo<PropagationNode>,
    layer: RwSignal<Layer>,
) -> impl IntoView {
    let half_node = node.clone();
    let height_node = node.clone();
    let paths_node = node.clone();
    let sondes_node = node;
    let half_life = pick(
        HALF_LIVES
            .iter()
            .map(|(value, name)| (*value, (*name).to_owned()))
            .collect(),
        Signal::derive(move || Some(settings.get().half_life_minutes)),
        move |minutes| {
            update(store, half_node.clone(), move |held| {
                held.half_life_minutes = minutes
            })
        },
    );
    let height = pick(
        HEIGHTS
            .iter()
            .map(|(value, name)| (*value, (*name).to_owned()))
            .collect(),
        Signal::derive(move || Some(settings.get().reflection_height_km)),
        move |km| {
            update(store, height_node.clone(), move |held| {
                held.reflection_height_km = km
            })
        },
    );
    let layers = segments(
        vec![(Layer::Activity, "Activity"), (Layer::Muf, "MUF")],
        layer.into(),
        move |chosen| layer.set(chosen),
    );
    let paths = check(
        Signal::derive(move || settings.get().show_paths),
        move |on| {
            update(store, paths_node.clone(), move |held| held.show_paths = on);
        },
    );
    let sondes = check(
        Signal::derive(move || settings.get().compare_forecast),
        move |on| {
            update(store, sondes_node.clone(), move |held| {
                held.compare_forecast = on
            });
        },
    );
    view! {
        row(class = "geo__chips", on:pointer_down:stop = |_| {}) {
            {titled("Decay half-life", chip("Half-life", AnyView::new(half_life)))}
            {titled("Reflecting layer height", chip("Height", AnyView::new(height)))}
            {titled("Map layer", chip("Map", AnyView::new(layers)))}
            {titled("Draw the path to every station heard", toggle("Paths", AnyView::new(paths)))}
            {titled("Compare against the ionosonde network", toggle("Ionosondes", AnyView::new(sondes)))}
        }
    }
}

fn chip(name: &'static str, body: AnyView) -> AnyView {
    AnyView::new(view! {
        column(class = "geo__entry") {
            text(class = "geo__label") {{name}}
            {body}
        }
    })
}

fn toggle(name: &'static str, body: AnyView) -> AnyView {
    AnyView::new(view! {
        row(class = "geo__check") {
            {body}
            text(class = "geo__label") {{name}}
        }
    })
}

fn foot(session: RwSignal<Session>, computed: RwSignal<Computed>) -> impl IntoView {
    let clearable = Signal::derive(move || computed.with(|computed| computed.live > 0));
    view! {
        row(class = "geo__foot", on:pointer_down:stop = |_| {}) {
            {armed_clear(clearable, move || session.update(|session| {
                session.observations.clear();
                session.cleared_at = now_ms();
            }))}
        }
    }
}

fn readouts(
    settings: Memo<PropagationNode>,
    report: ArcRwSignal<sondes::Answer>,
    computed: RwSignal<Computed>,
) -> impl IntoView {
    let read = move |pick: fn(&Computed) -> String| move || computed.with(pick);
    let base = vec![
        readout("Decodes", None, read(|c| c.summary.decodes.to_string())),
        readout("Grids", None, read(|c| c.summary.grids.to_string())),
        readout("Cells", None, read(|c| c.cells.len().to_string())),
        readout(
            "Highest",
            Some("Highest frequency heard"),
            read(|c| mhz(c.summary.best_freq_hz)),
        ),
        readout(
            "MUF(3000)",
            Some(MUF_HINT),
            read(|c| muf_floor(c.summary.best_muf3000_mhz)),
        ),
        readout(
            "Farthest",
            None,
            read(|c| format!("{:.0} km", c.summary.farthest_km)),
        ),
    ];
    let forecast = move || {
        settings
            .get()
            .compare_forecast
            .then(|| forecast_readouts(&report, computed))
    };
    view! {
        row(class = "geo__readouts") {
            {base}
            {forecast}
        }
    }
}

fn forecast_readouts(
    report: &ArcRwSignal<sondes::Answer>,
    computed: RwSignal<Computed>,
) -> Vec<AnyView> {
    let (sites, source) = report.with(|answer| {
        let report = answer.as_ref().and_then(|answer| answer.as_ref().ok());
        (
            report.map_or(0, |report| report.stations.len()),
            report.map(|report| report.source.clone()),
        )
    });
    let overhead = move || {
        computed.with(|c| {
            c.overhead.as_ref().map_or_else(
                || String::from("-"),
                |forecast| format!("{:.1} MHz", forecast.muf3000_mhz),
            )
        })
    };
    let mut out = vec![
        readout(
            "Forecast",
            Some("The ionosonde MUF(3000) over your location"),
            overhead,
        ),
        titled(
            sites_hint(source.as_deref()),
            readout("Sites", None, move || sites_text(sites)),
        ),
    ];
    if sites > 0 {
        out.push(readout(
            "Above",
            Some("Compared cells that sit above the forecast"),
            move || computed.with(|c| format!("{}/{}", c.agreement.above, c.agreement.cells)),
        ));
        out.push(readout(
            "Median \u{0394}",
            Some("Median of measured minus forecast MUF"),
            move || computed.with(|c| format!("{:+.1} MHz", c.agreement.median_delta_mhz)),
        ));
    }
    out
}

#[must_use]
pub fn muf_floor(muf: Option<f64>) -> String {
    muf.map_or_else(|| String::from("-"), |muf| format!("\u{2265} {muf:.1} MHz"))
}

#[must_use]
pub fn sites_text(sites: usize) -> String {
    if sites == 0 {
        String::from("waiting")
    } else {
        sites.to_string()
    }
}

#[must_use]
pub fn sites_hint(source: Option<&str>) -> String {
    format!(
        "Sounding sites, from {}",
        source.unwrap_or("the ionosonde network")
    )
}

#[must_use]
pub fn feed_failure(answer: &sondes::Answer) -> Option<String> {
    match answer.as_ref()? {
        Err(error) => Some(error.clone()),
        Ok(report) => report.error.clone(),
    }
}

fn fault(compare: Memo<bool>, report: ArcRwSignal<sondes::Answer>) -> impl IntoView {
    move || {
        let detail = report.with(feed_failure).filter(|_| compare.get())?;
        Some(view! {
            column(class = "geo__fault") {
                text(class = "geo__fault-head") {"Ionosonde feed failed"}
                text(class = "geo__fault-detail") {{detail}}
            }
        })
    }
}

fn cell(text: String) -> AnyView {
    AnyView::new(view! { text(class = "geo__cell") {{text}} })
}

fn table(settings: Memo<PropagationNode>, computed: RwSignal<Computed>) -> impl IntoView {
    move || {
        let compare = settings.get().compare_forecast;
        let rows: Vec<Vec<AnyView>> = computed.with(|computed| {
            if compare {
                let mut sorted = computed.comparisons.clone();
                sorted.sort_by(|a, b| b.cell.weight.total_cmp(&a.cell.weight));
                sorted
                    .into_iter()
                    .take(TABLE_ROWS)
                    .map(|row| {
                        let colour = format!("#{:06x}", layers::muf_colour(row.measured_muf3000_mhz));
                        vec![
                            cell(row.cell.key.clone()),
                            cell(row.cell.decodes.to_string()),
                            cell(mhz(row.cell.best_freq_hz)),
                            AnyView::new(view! {
                                text(class = "geo__cell", style:color = Some(colour)) {{format!("\u{2265} {:.1}", row.measured_muf3000_mhz)}}
                            }),
                            cell(format!("{:.1}", row.forecast.muf3000_mhz)),
                            cell(format!("{:+.1}", row.delta_mhz)),
                        ]
                    })
                    .collect()
            } else {
                computed
                    .cells
                    .iter()
                    .take(TABLE_ROWS)
                    .map(|found| {
                        vec![
                            cell(found.key.clone()),
                            cell(found.decodes.to_string()),
                            cell(mhz(found.best_freq_hz)),
                            cell(found.measured_muf3000_mhz.map_or_else(|| String::from("-"), |muf| format!("\u{2265} {muf:.1}"))),
                        ]
                    })
                    .collect()
            }
        });
        if rows.is_empty() {
            return AnyView::new(view! { text(class = "geo__note") {"No reflection points yet."} });
        }
        let mut head = vec!["Midpoint", "Decodes", "Highest", "Measured MUF"];
        if compare {
            head.extend(["Forecast", "\u{0394}"]);
        }
        let head: Vec<AnyView> = head.into_iter().map(|name| cell(name.to_owned())).collect();
        let body: Vec<AnyView> = rows
            .into_iter()
            .map(|cells| AnyView::new(view! { row(class = "geo__row") {{cells}} }))
            .collect();
        AnyView::new(view! {
            column(class = "geo__table") {
                row(class = "geo__row head") {{head}}
                {body}
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_query_names_every_wired_decoder() {
        let inputs = [
            Input {
                node: "ft8 a".to_owned(),
                device_set: 1,
                channel: 2,
            },
            Input {
                node: "wspr".to_owned(),
                device_set: 1,
                channel: 3,
            },
        ];
        let path = history_path("ft8", &inputs, 0);
        assert!(path.starts_with(
            "/api/decoderlog?kind=ft8&nodes=ft8%20a%2Cwspr&sources=1%3A2%2C1%3A3&since="
        ));
        assert!(path.ends_with("&limit=2000"));
    }

    #[test]
    fn the_forecast_readouts_say_waiting_and_name_their_source() {
        assert_eq!(sites_text(0), "waiting");
        assert_eq!(sites_text(12), "12");
        assert_eq!(
            sites_hint(None),
            "Sounding sites, from the ionosonde network"
        );
        assert_eq!(sites_hint(Some("GIRO")), "Sounding sites, from GIRO");
        assert_eq!(muf_floor(None), "-");
        assert_eq!(muf_floor(Some(21.04)), "\u{2265} 21.0 MHz");
    }

    #[test]
    fn a_feed_failure_is_either_a_dead_request_or_a_reported_error() {
        assert_eq!(feed_failure(&None), None);
        assert_eq!(
            feed_failure(&Some(Err("timeout".to_owned()))).as_deref(),
            Some("timeout")
        );
        let report = sdrmm_wire::propagation::IonosondeReport {
            source: "GIRO".to_owned(),
            url: String::new(),
            fetched_at: None,
            stations: Vec::new(),
            error: Some("HTTP 503".to_owned()),
        };
        assert_eq!(feed_failure(&Some(Ok(report))).as_deref(), Some("HTTP 503"));
    }

    #[test]
    fn an_empty_session_computes_an_empty_picture() {
        let computed = compute(
            &[],
            0,
            PropagationNode::default(),
            None,
            &[],
            Layer::Activity,
            0,
        );
        assert_eq!(computed.live, 0);
        assert!(computed.cells.is_empty());
        assert!(computed.overhead.is_none());
    }
}
