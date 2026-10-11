pub mod survey;

use std::sync::Arc;

use sdrmm_wire::{
    patch::{NodeBody, SignalMapNode},
    survey::{SurveyAction, SurveyCell, SurveyGrid, SurveyRequest},
    ws::ServerEvent,
};
use zgui::prelude::*;
use zgui::reactive::RenderEffect;

use self::survey::{
    SIGNAL_MAX_DBFS, SIGNAL_MIN_DBFS, SurveyState, SurveyView, can_start, followed, retuned_since,
    seeded,
};
use crate::{
    binding,
    head::{self, NodeHead},
    store::Store,
    ui::{
        faces::channel::{lane_center_hz, lane_rate_hz},
        kit_audio::{self, Stat, Tone, si, stats, titled},
        kit_channel::{self, radio::offset_stepper},
        kit_maps::{
            SHEET, armed_clear, encode, entry,
            feed::{Feed, Topic},
            readout, trail,
        },
        map::{self, ACCENT, Frame, MapProps, Overlay},
    },
};

const MAX_BANDWIDTH_HZ: u64 = 100_000_000;
const MAX_OFFSET_HZ: f64 = 1e12;
const SURVEY_FRAME_ZOOM: f64 = 14.0;
const LEVEL_HINT: &str =
    "Relative receiver level. Keep gain and antenna fixed when comparing locations";
const DROPPED_HINT: &str = "Oldest cells dropped to stay within the limit";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Lane {
    center_hz: Option<f64>,
    span_hz: Option<f64>,
}

#[derive(Clone, Copy)]
struct Survey {
    store: Store,
    state: RwSignal<SurveyState>,
    pending: RwSignal<bool>,
    syncing: RwSignal<bool>,
    error: RwSignal<Option<String>>,
}

fn settings_of(store: Store, node: &str) -> SignalMapNode {
    store
        .graph
        .get()
        .node(node)
        .and_then(|found| match &found.body {
            NodeBody::SignalMap(settings) => Some(*settings),
            _ => None,
        })
        .unwrap_or_default()
}

fn write(store: Store, node: String, settings: SignalMapNode) {
    store.edit_graph(move |graph| {
        if let Some(found) = graph.nodes.iter_mut().find(|found| found.id == node)
            && let NodeBody::SignalMap(held) = &mut found.body
        {
            *held = settings;
        }
    });
}

fn lane_of(store: Store, node: &str) -> Lane {
    let graph = store.graph.get();
    let Some((device, stream)) = binding::iq_source_of(&graph, node) else {
        return Lane::default();
    };
    let Some(set) = store.device_set_of(&device).and_then(|id| store.set_of(id)) else {
        return Lane::default();
    };
    Lane {
        center_hz: lane_center_hz(&set, stream),
        span_hz: lane_rate_hz(&set, stream),
    }
}

fn limit(span_hz: Option<f64>, bandwidth_hz: u64) -> f64 {
    span_hz.map_or(MAX_OFFSET_HZ, |span| {
        survey::offset_limit_hz(span, bandwidth_hz as f64).min(MAX_OFFSET_HZ)
    })
}

fn path_of(node: &str) -> String {
    format!("/api/survey/{}", encode(node))
}

impl Survey {
    fn seed(self, node: String) {
        if self.syncing.get_untracked() {
            return;
        }
        self.syncing.set(true);
        zgui::task::spawn_local(async move {
            let answer = self.store.api().get::<SurveyGrid>(&path_of(&node)).await;
            self.syncing.try_set(false);
            match answer {
                Ok(grid) => {
                    self.state.try_update(|state| *state = seeded(state, grid));
                }
                Err(error) => {
                    self.error.try_set(Some(format!("Survey: {error}")));
                }
            }
        });
    }

    fn control(self, node: String, action: SurveyAction) {
        if self.pending.get_untracked() {
            return;
        }
        self.pending.set(true);
        zgui::task::spawn_local(async move {
            let answer = self
                .store
                .api()
                .post::<_, SurveyGrid>(&path_of(&node), &SurveyRequest { action })
                .await;
            self.pending.try_set(false);
            match answer {
                Ok(grid) => {
                    self.error.try_set(None);
                    self.state.try_update(|state| *state = seeded(state, grid));
                }
                Err(error) => {
                    self.error.try_set(Some(format!("Survey: {error}")));
                    self.store.fail("Survey", &error);
                }
            }
        });
    }

    fn listen(self, node: String) {
        let ours = node.clone();
        self.store.on_event(move |event| match event {
            ServerEvent::Hello { .. } => {
                self.state.set(SurveyState::default());
                self.seed(ours.clone());
            }
            ServerEvent::SurveyUpdate { node: from, update } if *from == ours => {
                self.state.update(|state| *state = followed(state, update));
            }
            _ => {}
        });
        let resync = RenderEffect::new(move |_| {
            if self.state.with(|state| state.behind) {
                self.seed(node.clone());
            }
        });
        on_cleanup_local(move || drop(resync));
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("kit-maps", SHEET);
    kit_audio::install();
    kit_channel::install();
    let feed = Feed::get();
    feed.listen(store);
    let settings = {
        let node = node.clone();
        Memo::new(move |_| settings_of(store, &node))
    };
    let lane = {
        let node = node.clone();
        Memo::new(move |_| lane_of(store, &node))
    };
    let radio_wired = {
        let node = node.clone();
        Memo::new(move |_| binding::iq_source_of(&store.graph.get(), &node).is_some())
    };
    let positions = {
        let node = node.clone();
        Memo::new(move |_| trail::positions_of(&store.graph.get(), &node))
    };
    let positioned = {
        let feed = feed.clone();
        Signal::derive_local(move || {
            let first = positions.with(|nodes| nodes.first().cloned());
            feed.with(Topic::Positions, |held| {
                first
                    .and_then(|node| held.tracks.get(&node))
                    .is_some_and(|track| track.fix.is_some())
            })
        })
    };
    let survey = Survey {
        store,
        state: RwSignal::new(SurveyState::default()),
        pending: RwSignal::new(false),
        syncing: RwSignal::new(false),
        error: RwSignal::new(None),
    };
    survey.listen(node.clone());
    survey.seed(node.clone());

    let view_of = Signal::derive_local(move || {
        let wired = radio_wired.get();
        survey.state.with(|state| {
            let target_hz = state.target_hz.filter(|_| wired);
            SurveyView {
                radio_wired: wired,
                position_wired: positions.with(|nodes| !nodes.is_empty()),
                positioned: positioned.get(),
                target_hz,
                level_dbfs: state.level_dbfs.filter(|_| wired),
                recording: state.recording,
                retuned: retuned_since(&state.cells, target_hz),
            }
        })
    });
    head::show(store, node.clone(), move || {
        NodeHead::said(survey::status(&view_of.get()))
    });

    let map_view = surveyed_map(store, node.clone(), survey, positions);
    view! {
        column(class = "geo survey") {
            {map_view}
            {chips(store, node.clone(), settings, lane, survey)}
            {readouts(survey, view_of)}
            {kit_audio::alert(move || survey.error.get())}
            {foot(node, settings, survey, view_of)}
        }
    }
}

fn surveyed_map(
    store: Store,
    node: String,
    survey: Survey,
    positions: Memo<Vec<String>>,
) -> impl IntoView {
    let feed = Feed::get();
    let drawn = {
        let feed = feed.clone();
        Signal::derive_local(move || {
            let nodes = positions.get();
            let (mut overlay, trail_points) = feed.with(Topic::Positions, |held| {
                let tracks: Vec<_> = nodes
                    .iter()
                    .filter_map(|node| held.tracks.get(node))
                    .collect();
                (trail::overlay(&tracks), trail::points(&tracks))
            });
            let cells = survey.state.with(|state| state.cells.clone());
            overlay.extend(survey::overlay(&cells));
            let frame = if cells.is_empty() {
                trail_points
            } else {
                cells
                    .iter()
                    .map(|cell| map::Geo::new(cell.latitude, cell.longitude))
                    .collect()
            };
            (Arc::new(overlay), frame)
        })
    };
    let overlay = RwSignal::new(Arc::new(Overlay::default()));
    let frame = RwSignal::new(None::<Arc<Frame>>);
    let publishing = RenderEffect::new(move |_| {
        let (next, points) = drawn.get();
        overlay.set(next);
        frame.set((!points.is_empty()).then(|| {
            Arc::new(Frame {
                points,
                max_zoom: SURVEY_FRAME_ZOOM,
            })
        }));
    });
    on_cleanup_local(move || drop(publishing));
    let active = Signal::derive(move || store.selected.get().as_deref() == Some(node.as_str()));
    let props = MapProps {
        overlay: overlay.into(),
        active,
        frame: frame.into(),
        on_pick: None,
    };
    let chrome = AnyView::new(legend(survey, positions));
    map::map(store, props, chrome)
}

fn offset_chip(
    store: Store,
    node: String,
    settings: Memo<SignalMapNode>,
    lane: Memo<Lane>,
    held: Signal<bool>,
) -> impl IntoView {
    move || {
        if held.get() {
            let shown = AnyView::new(view! {
                row(class = "geo__pair") {
                    text(class = "geo__label") {"Offset"}
                    text(class = "geo__num") {{move || signed_hz(settings.get().offset_hz as f64)}}
                }
            });
            return titled("Clear the survey to change the offset", shown);
        }
        let node = node.clone();
        let stepper = offset_stepper(
            Signal::derive(move || settings.get().offset_hz as f64),
            Signal::derive(move || Some(limit(lane.get().span_hz, settings.get().bandwidth_hz))),
            Signal::derive(move || lane.get().center_hz),
            move |offset| {
                let current = settings.get_untracked();
                let offset_hz = offset.round() as i64;
                if offset_hz != current.offset_hz {
                    write(
                        store,
                        node.clone(),
                        SignalMapNode {
                            offset_hz,
                            ..current
                        },
                    );
                }
            },
        );
        titled("Offset from the radio centre", AnyView::new(stepper))
    }
}

fn signed_hz(hz: f64) -> String {
    let sign = if hz < 0.0 { "\u{2212}" } else { "+" };
    format!("{sign}{}", si(hz.abs(), "Hz"))
}

fn width_field(
    store: Store,
    node: String,
    settings: Memo<SignalMapNode>,
    lane: Memo<Lane>,
    held: Signal<bool>,
) -> AnyView {
    let field = entry(
        "Width kHz",
        Signal::derive(move || format!("{}", settings.get().bandwidth_hz as f64 / 1e3)),
        held,
        move |text| {
            let current = settings.get_untracked();
            let Some(hz) = parse_width(&text) else {
                store.say("Width must be 0.001 to 100000 kHz");
                return;
            };
            if hz != current.bandwidth_hz {
                let bound = limit(lane.get_untracked().span_hz, hz);
                let offset_hz = (current.offset_hz as f64).clamp(-bound, bound) as i64;
                write(
                    store,
                    node.clone(),
                    SignalMapNode {
                        offset_hz,
                        bandwidth_hz: hz,
                    },
                );
            }
        },
    );
    let tip = if held.get_untracked() {
        "Clear the survey to change the width"
    } else {
        "Survey bandwidth"
    };
    titled(tip, AnyView::new(field))
}

fn chips(
    store: Store,
    node: String,
    settings: Memo<SignalMapNode>,
    lane: Memo<Lane>,
    survey: Survey,
) -> impl IntoView {
    let held = Signal::derive(move || survey.state.with(|state| !state.cells.is_empty()));
    let held_now = Memo::new(move |_| held.get());
    let width_node = node.clone();
    view! {
        row(class = "geo__chips", on:pointer_down:stop = |_| {}) {
            {offset_chip(store, node, settings, lane, held)}
            {move || {
                held_now.track();
                width_field(store, width_node.clone(), settings, lane, held)
            }}
        }
    }
}

#[must_use]
pub fn parse_width(text: &str) -> Option<u64> {
    let khz = text.trim().replace(',', ".").parse::<f64>().ok()?;
    let hz = (khz * 1e3).round();
    (hz >= 1.0 && hz <= MAX_BANDWIDTH_HZ as f64).then_some(hz as u64)
}

fn readouts(survey: Survey, view_of: Signal<SurveyView, LocalStorage>) -> impl IntoView {
    let cells = readout("Cells", None, move || {
        survey.state.with(|state| state.cells.len()).to_string()
    });
    let at = readout("At", None, move || {
        view_of
            .get()
            .target_hz
            .map_or_else(|| String::from("- Hz"), |hz| si(hz, "Hz"))
    });
    let level = readout("Level", Some(LEVEL_HINT), move || {
        view_of.get().level_dbfs.map_or_else(
            || String::from("- dBFS"),
            |level| format!("{level:.1} dBFS"),
        )
    });
    view! {
        row(class = "geo__readouts") {
            {cells}
            {at}
            {level}
        }
    }
}

fn foot(
    node: String,
    settings: Memo<SignalMapNode>,
    survey: Survey,
    view_of: Signal<SurveyView, LocalStorage>,
) -> impl IntoView {
    let held = Signal::derive(move || survey.state.with(|state| !state.cells.is_empty()));
    let recording = Signal::derive(move || survey.state.with(|state| state.recording));
    let startable = Signal::derive_local(move || {
        !survey.pending.get() && (recording.get() || can_start(&view_of.get()))
    });
    let clearable = Signal::derive(move || held.get() && !survey.pending.get());
    let dropped = move || {
        let dropped = survey.state.with(|state| state.dropped);
        (dropped > 0)
            .then(|| Stat::new("Dropped", dropped.to_string(), DROPPED_HINT).tone(Tone::Warn))
            .into_iter()
            .collect()
    };
    let clear_node = node.clone();
    view! {
        row(class = "geo__foot", on:pointer_down:stop = |_| {}) {
            {stats(dropped)}
            control(
                class = "btn",
                state:disabled = move || !held.get(),
                on:click:stop = move |_| {
                    let cells = survey.state.with_untracked(|state| state.cells.clone());
                    export(survey.store, settings.get_untracked(), &cells);
                }
            ) {"Export CSV"}
            {armed_clear(clearable, move || survey.control(clear_node.clone(), SurveyAction::Clear))}
            control(
                class = "btn",
                class:danger = recording,
                class:primary = move || !recording.get(),
                state:disabled = move || !startable.get(),
                on:click:stop = move |_| {
                    let action = if recording.get_untracked() {
                        SurveyAction::Stop
                    } else {
                        SurveyAction::Start
                    };
                    survey.control(node.clone(), action);
                }
            ) {
                {move || if recording.get() { "Pause" } else { "Start survey" }}
            }
        }
    }
}

fn export(store: Store, settings: SignalMapNode, cells: &[SurveyCell]) {
    let frequency = cells
        .first()
        .map_or(0, |cell| cell.frequency_hz.round() as i64);
    let name = format!(
        "signal-survey-{frequency}-hz-{}-hz-wide.csv",
        settings.bandwidth_hz
    );
    let Some(folder) = dirs::download_dir().or_else(dirs::home_dir) else {
        store.say("No folder to save the survey in");
        return;
    };
    let path = folder.join(name);
    match std::fs::write(
        &path,
        survey::csv(cells, settings.offset_hz, settings.bandwidth_hz),
    ) {
        Ok(()) => store.note(format!("Saved {}", path.display())),
        Err(error) => store.say(format!("Cannot save the survey: {error}")),
    }
}

fn legend(survey: Survey, positions: Memo<Vec<String>>) -> impl IntoView {
    let feed = Feed::get();
    let trail_count = move || {
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
    view! {
        column(class = "geo__legend") {
            column(class = "geo__legend-box") {
                if move || !positions.get().is_empty() {
                    {trail::legend_row(ACCENT, "GPS trail", trail_count.clone())}
                }
                row(class = "geo__legend-row") {
                    text(class = "geo__legend-name") {"Signal cells"}
                    text(class = "geo__legend-count") {{move || survey.state.with(|state| state.cells.len()).to_string()}}
                }
                box(class = "geo__ramp", style:background-image = Some(survey::RAMP_CSS.to_string()))
                row(class = "geo__ends") {
                    text {{format!("{SIGNAL_MIN_DBFS} dBFS")}}
                    text {{format!("{SIGNAL_MAX_DBFS} dBFS")}}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_width_is_whole_hertz_inside_the_limits() {
        assert_eq!(parse_width("12.5"), Some(12_500));
        assert_eq!(parse_width(" 12,5 "), Some(12_500));
        assert_eq!(parse_width("0"), None);
        assert_eq!(parse_width("200000"), None);
        assert_eq!(parse_width("wide"), None);
    }

    #[test]
    fn an_unknown_span_does_not_bound_the_offset_and_a_known_one_keeps_the_width_inside() {
        assert_eq!(limit(None, 12_500), MAX_OFFSET_HZ);
        assert_eq!(limit(Some(1_000_000.0), 12_500), 493_750.0);
        assert_eq!(path_of("signal map"), "/api/survey/signal%20map");
        assert_eq!(signed_hz(-25_000.0).chars().next(), Some('\u{2212}'));
    }
}
