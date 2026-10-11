pub mod detail;
pub mod df;
pub mod heat;
pub mod targets;

use std::{sync::Arc, time::Duration};

use sdrmm_wire::{channel::ChannelParams, ws::ServerEvent};
use zgui::prelude::*;
use zgui::reactive::RenderEffect;

use crate::{
    arrays::Book,
    store::Store,
    ui::{
        kit_audio::titled,
        kit_maps::{
            self, SHEET,
            feed::{Feed, Held, Topic},
            mhz, now_ms, ticker, trail,
            wiring::{channel_type_of, event_sources},
        },
        map::{self, ACCENT, Frame, Geo, MapProps, Overlay, overlay::Image, style},
    },
};

const DRAW_TICK: Duration = Duration::from_millis(500);
const AGE_OUT_EVERY: Duration = Duration::from_secs(15);
const TARGET_FRAME_ZOOM: f64 = 9.0;
const TRAIL_FRAME_ZOOM: f64 = 14.0;
const HEAT_RAMP_CSS: &str =
    "linear-gradient(to right, #000004, #420a68, #932667, #dd513a, #fca50a, #fcffa4)";

#[derive(Clone, Debug, Default, PartialEq)]
struct Wiring {
    kinds: Vec<&'static str>,
    positions: Vec<String>,
    df: df::Sources,
    references: Vec<Geo>,
}

fn wiring(store: Store, node: &str) -> Wiring {
    let graph = store.graph.get();
    let sources = event_sources(&graph, node);
    let kinds: Vec<String> = sources
        .iter()
        .filter_map(|source| channel_type_of(&graph, source))
        .filter_map(|kind| store.descriptor_of(&kind)?.decoder_kind)
        .collect();
    let params: Vec<ChannelParams> = sources
        .iter()
        .filter_map(|source| store.channel_of(source))
        .map(|channel| channel.settings.params)
        .collect();
    Wiring {
        kinds: targets::map_kinds_of(&kinds),
        positions: trail::positions_of(&graph, node),
        df: df::sources_of(&graph, node),
        references: targets::references(&params),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Drawn {
    overlay: Arc<Overlay>,
    counts: Vec<(&'static str, usize)>,
    trail: usize,
    frame: Option<Arc<Frame>>,
    df: df::Counts,
}

struct Look<'a> {
    selected: Option<&'a str>,
    tracks: bool,
    heat: Option<Image>,
    now: i64,
}

fn draw(held: &Held, book: &Book, wired: &Wiring, look: Look<'_>) -> Drawn {
    let Look {
        selected,
        tracks: show_tracks,
        heat,
        now,
    } = look;
    let tracks: Vec<_> = wired
        .positions
        .iter()
        .filter_map(|node| held.tracks.get(node))
        .collect();
    let from = tracks
        .iter()
        .find_map(|track| track.fix.as_ref())
        .map(|fix| Geo::new(fix.latitude, fix.longitude));
    let mut overlay = Overlay {
        marks: targets::reference_marks(&wired.references),
        images: heat.into_iter().collect(),
        ..Overlay::default()
    };
    overlay.extend(trail::overlay(&tracks));
    let picture = df::picture(&wired.df, held, book, now, from);
    if let Some(picture) = &picture {
        overlay.extend(df::overlay(picture));
    }
    let mut counts = Vec::new();
    let mut target_points = Vec::new();
    for kind in &wired.kinds {
        let stations: Vec<_> = held
            .stations
            .get(kind)
            .map(|stations| stations.values().collect())
            .unwrap_or_default();
        if show_tracks {
            overlay
                .lines
                .extend(targets::track_lines(&stations, held.trails.get(kind), now));
        }
        let (drawn, shown) = targets::overlay(&stations, selected, now);
        target_points.extend(drawn.dots.iter().map(|dot| dot.at));
        overlay.extend(drawn);
        counts.push((*kind, shown));
    }
    let trail_points = trail::points(&tracks);
    let frame = if target_points.is_empty() {
        (!trail_points.is_empty()).then(|| Frame {
            points: trail_points.clone(),
            max_zoom: TRAIL_FRAME_ZOOM,
        })
    } else {
        Some(Frame {
            points: target_points,
            max_zoom: TARGET_FRAME_ZOOM,
        })
    };
    Drawn {
        overlay: Arc::new(overlay),
        counts,
        trail: trail_points.len(),
        frame: frame.map(Arc::new),
        df: df::counts(picture.as_ref()),
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("kit-maps", SHEET);
    install_stylesheet("map-face", FACE_SHEET);
    let feed = Feed::get();
    feed.listen(store);
    let now = ticker(DRAW_TICK);
    {
        let feed = feed.clone();
        kit_maps::every(AGE_OUT_EVERY, move || {
            feed.change(Topic::Stations, |held| {
                held.age_out(targets::TARGET_MAX_AGE_MS, now_ms())
            });
        });
    }
    let wired = {
        let node = node.clone();
        Memo::new(move |_| wiring(store, &node))
    };
    let selected = RwSignal::new(None::<String>);
    let unwiring = RenderEffect::new(move |_| {
        let kinds = wired.with(|wired| wired.kinds.clone());
        if selected.with_untracked(|chosen| chosen_kind_gone(chosen.as_deref(), &kinds)) {
            selected.set(None);
        }
    });
    on_cleanup_local(move || drop(unwiring));
    let heat = RwSignal::new(None::<heat::Held>);
    let refused = RwSignal::new(None::<(String, &'static str)>);
    let crossing = Memo::new(move |_| wired.with(|wired| wired.df.crossings.first().cloned()));
    let fused = Memo::new(move |_| {
        let node = crossing.get()?;
        store
            .arrays
            .with(|book| book.fusion.contains_key(&node))
            .then_some(node)
    });
    store.on_event(move |event| {
        if let ServerEvent::SurfaceRefused { node, reason } = event
            && crossing.with_untracked(|crossing| crossing.as_deref() == Some(node.as_str()))
        {
            refused.set(Some((node.clone(), reason.label())));
        }
    });
    let view_pref = style::map_view();
    let drawn = RwSignal::new(Drawn::default());
    let drawing = {
        let feed = feed.clone();
        RenderEffect::new(move |_| {
            let at = now.get();
            let wiring = wired.get();
            let chosen = selected.get();
            let tracks = view_pref.with(|view| view.tracks);
            let shown_heat = heat_for(heat, crossing);
            let next = store.arrays.with_untracked(|book| {
                draw(
                    &feed.held.borrow(),
                    book,
                    &wiring,
                    Look {
                        selected: chosen.as_deref(),
                        tracks,
                        heat: shown_heat,
                        now: at,
                    },
                )
            });
            drawn.set(next);
        })
    };
    on_cleanup_local(move || drop(drawing));

    let active = {
        let node = node.clone();
        Signal::derive(move || store.selected.get().as_deref() == Some(node.as_str()))
    };
    let props = MapProps {
        overlay: Signal::derive(move || drawn.with(|drawn| drawn.overlay.clone())),
        active,
        frame: Signal::derive(move || drawn.with(|drawn| drawn.frame.clone())),
        on_pick: Some(UnsyncCallback::new(move |hit: Option<String>| {
            selected.set(hit)
        })),
    };
    let heat_shown = Signal::derive(move || heat_for(heat, crossing).is_some());
    let refusal = Signal::derive(move || {
        if heat_shown.get() {
            return None;
        }
        let crossing = crossing.get()?;
        refused.with(|held| {
            held.as_ref()
                .filter(|(node, _)| *node == crossing)
                .map(|(_, reason)| *reason)
        })
    });
    let chrome = AnyView::new(view! {
        {move || {
            if let Some(node) = fused.get() {
                heat::feed(store, node, heat);
            }
            AnyView::new(())
        }}
        {legend(drawn, wired, heat_shown, refusal)}
        {detail(feed, selected, wired)}
    });
    view! {
        column(class = "geo map-face") {
            {map::map(store, props, chrome)}
        }
    }
}

fn heat_for(heat: RwSignal<Option<heat::Held>>, crossing: Memo<Option<String>>) -> Option<Image> {
    let wanted = crossing.get()?;
    heat.with(|held| {
        held.as_ref()
            .filter(|held| held.node == wanted)
            .map(|held| held.image.clone())
    })
}

#[must_use]
pub fn chosen_kind_gone(selected: Option<&str>, kinds: &[&'static str]) -> bool {
    selected
        .and_then(|key| key.split_once('/'))
        .is_some_and(|(kind, _)| !kinds.contains(&kind))
}

fn legend(
    drawn: RwSignal<Drawn>,
    wired: Memo<Wiring>,
    heat: Signal<bool>,
    refusal: Signal<Option<&'static str>>,
) -> impl IntoView {
    let rows = move || {
        let counts = drawn.with(|drawn| drawn.counts.clone());
        let trail = wired.with(|wired| !wired.positions.is_empty());
        let df_rows = drawn.with(|drawn| df::legend_rows(&drawn.df));
        let heat = heat.get();
        if counts.is_empty() && !trail && df_rows.is_empty() && !heat {
            return None;
        }
        let rows: Vec<AnyView> = counts
            .into_iter()
            .map(|(kind, _)| {
                let (title, colour) = targets::style(kind);
                let count = move || {
                    drawn.with(|drawn| {
                        drawn
                            .counts
                            .iter()
                            .find(|(held, _)| *held == kind)
                            .map_or(0, |(_, count)| *count)
                            .to_string()
                    })
                };
                AnyView::new(trail::legend_row(colour, title, count))
            })
            .collect();
        let trail_row = trail.then(|| {
            AnyView::new(trail::legend_row(ACCENT, "GPS trail", move || {
                drawn.with(|drawn| drawn.trail.to_string())
            }))
        });
        let heat_row = heat.then(|| {
            AnyView::new(view! {
                row(class = "geo__legend-row") {
                    box(class = "map-face__heat", style:background-image = Some(HEAT_RAMP_CSS.to_owned()))
                    text(class = "geo__legend-name") {"Heat"}
                }
            })
        });
        Some(view! {
            column(class = "geo__legend-box") {
                {rows}
                {trail_row}
                {df_rows}
                {heat_row}
            }
        })
    };
    let badge = move || {
        refusal.get().map(|reason| {
            titled(
                reason,
                AnyView::new(view! { text(class = "map-face__badge") {"no heat"} }),
            )
        })
    };
    view! {
        column(class = "geo__legend") {
            {rows}
            {badge}
        }
    }
}

fn detail(
    feed: std::rc::Rc<Feed>,
    selected: RwSignal<Option<String>>,
    wired: Memo<Wiring>,
) -> impl IntoView {
    move || {
        let key = selected.get()?;
        let (kind, id) = key.split_once('/')?;
        if !wired.with(|wired| wired.kinds.contains(&kind)) {
            return None;
        }
        let shown = feed.with(Topic::Stations, |held| {
            held.stations
                .get(kind)
                .and_then(|stations| stations.get(id))
                .map(detail::detail)
        });
        let Some(shown) = shown else {
            selected.set(None);
            return None;
        };
        let (_, colour) = targets::style(shown.kind);
        let rows: Vec<AnyView> = shown
            .rows
            .into_iter()
            .map(|(name, value)| {
                AnyView::new(view! {
                    row(class = "map-face__row") {
                        text(class = "map-face__name") {{name}}
                        text(class = "map-face__value") {{value}}
                    }
                })
            })
            .collect();
        let foot = format!(
            "{} · last seen {}",
            mhz(shown.freq_hz),
            kit_maps::utc_clock(shown.last_seen)
        );
        Some(view! {
            column(class = "map-face__card", on:pointer_down:stop = |_| {}) {
                row(class = "map-face__head") {
                    text(class = "map-face__title", style:color = Some(format!("#{colour:06x}"))) {{shown.label}}
                    spacer()
                    control(
                        class = "map-face__close",
                        a11y:label = "Clear target selection",
                        on:click:stop = move |_| selected.set(None)
                    ) {"x"}
                }
                column(class = "map-face__rows") {{rows}}
                text(class = "map-face__foot") {{foot}}
            }
        })
    }
}

pub const FACE_SHEET: &str = css!(
    r#"
.map-face { min-height: 300px; }
.map-face__heat { width: 16px; height: 8px; border-radius: 2px; opacity: 0.75; }
.map-face__badge {
    padding: 1px 6px;
    border: 1px solid var(--line);
    border-radius: 4px;
    background-color: color-mix(in oklab, var(--bg) 85%, transparent);
    color: var(--danger);
    pointer-events: auto;
}
.map-face__card {
    position: absolute;
    right: 8px;
    bottom: 20px;
    width: 240px;
    border: 1px solid var(--line);
    border-radius: 5px;
    background-color: var(--panel);
    cursor: default;
    pointer-events: auto;
}
.map-face__head { align-items: center; gap: 6px; padding: 3px 8px; border-bottom: 1px solid var(--line); }
.map-face__title { font-family: var(--mono); font-size: 13px; }
.map-face__close { color: var(--ink-dim); font-family: var(--mono); font-size: 11px; padding: 0 4px; }
.map-face__close:hover { color: var(--ink); }
.map-face__rows { padding: 4px 8px; gap: 1px; }
.map-face__row { justify-content: space-between; gap: 10px; }
.map-face__name { font-size: 11px; color: var(--ink-faint); }
.map-face__value { font-family: var(--mono); font-size: 11px; color: var(--ink); }
.map-face__foot { padding: 3px 8px; border-top: 1px solid var(--line); font-family: var(--mono); font-size: 10px; color: var(--ink-dim); }
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_goes_when_its_kind_is_unwired() {
        assert!(chosen_kind_gone(Some("ais/211"), &["adsb"]));
        assert!(!chosen_kind_gone(Some("adsb/3c6444"), &["adsb"]));
        assert!(!chosen_kind_gone(None, &[]));
    }
}
