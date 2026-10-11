pub mod text;

use std::time::Duration;

use sdrmm_wire::{
    patch::NodeBody,
    satellite::{SatelliteCatalogResponse, SatelliteNode, SatelliteStatus, TransmittersResponse},
};
use zgui::reactive::RenderEffect;
use zgui::{
    prelude::*,
    view::{TimeoutHandle, Timers},
};
use zgui_ui::prelude::*;

use crate::{
    head::{self, NodeHead},
    store::Store,
    ui::{
        kit_audio::{self, Stat, Tone, titled},
        kit_channel::{self, tuning_lock},
        kit_maps::{
            SHEET,
            feed::{Feed, Topic},
            mhz, ticker,
        },
        kit_sources::{self, DialSpec, dial::Reach, frequency_dial, search::searchable_pick},
        widgets::row_field,
    },
};

const REACH: Reach = Reach {
    min: text::RANGE_HZ.0,
    max: text::RANGE_HZ.1,
};

const SEARCH_DELAY: Duration = Duration::from_millis(350);
const SHOWN_RESULTS: usize = 8;

fn settings_of(store: Store, node: &str) -> SatelliteNode {
    store
        .graph
        .get()
        .node(node)
        .and_then(|found| match &found.body {
            NodeBody::Satellite(settings) => Some(settings.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn edit(store: Store, node: &str, change: impl FnOnce(&mut SatelliteNode) + 'static) {
    let node = node.to_owned();
    store.edit_graph(move |graph| {
        if let Some(found) = graph.nodes.iter_mut().find(|found| found.id == node)
            && let NodeBody::Satellite(held) = &mut found.body
        {
            change(held);
        }
    });
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("kit-maps", SHEET);
    install_stylesheet("satellite", FACE_SHEET);
    let feed = Feed::get();
    feed.listen(store);
    let settings = {
        let node = node.clone();
        Memo::new(move |_| settings_of(store, &node))
    };
    let status = {
        let node = node.clone();
        Signal::derive_local(move || {
            feed.with(Topic::Satellites, |held| {
                held.satellites.get(&node).cloned()
            })
        })
    };
    let elements = Memo::new(move |_| settings.get().tle.is_some());
    let picking = node.clone();
    view! {
        column(class = "face satellite", on:pointer_down:stop = |_| {}) {
            if move || elements.get() {
                {tracked(store, node.clone(), settings, status)}
            } else {
                {picker(store, picking.clone())}
            }
        }
    }
}

fn picker(store: Store, node: String) -> impl IntoView {
    let draft = RwSignal::new_local(String::new());
    let results = RwSignal::new(None::<Result<Vec<(String, String, String)>, String>>);
    let waiting = StoredValue::new_local(None::<TimeoutHandle>);
    let timers = Timers::current();
    let searching = RenderEffect::new(move |_| {
        let query = draft.get().trim().to_owned();
        if text::pasted_elements(&query).is_some() {
            waiting.set_value(None);
            results.set(None);
            return;
        }
        let Some(timers) = timers.clone() else {
            return;
        };
        let handle = timers.set_timeout(SEARCH_DELAY, move || {
            zgui::task::spawn_local(async move {
                let path = format!("/api/satellites?q={}", encode(&query));
                let answer = store.api().get::<SatelliteCatalogResponse>(&path).await;
                results.set(Some(answer.map_err(|error| error.to_string()).map(
                    |found| {
                        found
                            .satellites
                            .into_iter()
                            .take(SHOWN_RESULTS)
                            .map(|satellite| (satellite.name, satellite.catalog, satellite.tle))
                            .collect()
                    },
                )));
            });
        });
        waiting.set_value(Some(handle));
    });
    on_cleanup_local(move || drop(searching));
    let choose = move |tle: String| {
        edit(store, &node, move |held| held.tle = Some(tle));
    };
    let found = {
        let choose = choose.clone();
        move || {
            if let Some(tle) = text::pasted_elements(&draft.get()) {
                let choose = choose.clone();
                return AnyView::new(view! {
                    control(class = "btn", on:click:stop = move |_| choose(tle.clone())) {"Use these elements"}
                });
            }
            match results.get() {
                None => AnyView::new(()),
                Some(Err(error)) => AnyView::new(view! { text(class = "geo__bad") {{error}} }),
                Some(Ok(found)) if found.is_empty() => {
                    AnyView::new(view! { text(class = "hint") {"Nothing found"} })
                }
                Some(Ok(found)) => {
                    let rows: Vec<AnyView> = found
                        .into_iter()
                        .map(|(name, catalog, tle)| {
                            let choose = choose.clone();
                            AnyView::new(view! {
                                control(class = "satellite__hit", on:click:stop = move |_| choose(tle.clone())) {
                                    text {{name}}
                                    spacer()
                                    text(class = "legend") {{catalog}}
                                }
                            })
                        })
                        .collect();
                    AnyView::new(view! { column(class = "satellite__hits") {{rows}} })
                }
            }
        }
    };
    let search = titled(
        "A name, a NORAD number, or pasted element lines",
        AnyView::new(view! {
            Input(
                class = "native-input",
                value = draft,
                label = "Search satellites",
                placeholder = "ISS, 25544 or element lines"
            )
        }),
    );
    view! {
        column(class = "entry satellite__pick") {
            {search}
            box(class = "satellite__found") {{found}}
        }
    }
}

#[must_use]
pub fn elements_stat(age_days: Option<f64>) -> Option<Stat> {
    let days = age_days?;
    let shown = Stat::new(
        "Elements",
        format!("{days:.1} d"),
        "Age of the orbital elements. Older ones drift; refresh them",
    );
    Some(if days > text::STALE_ELEMENTS_DAYS {
        shown.tone(Tone::Warn)
    } else {
        shown
    })
}

fn downlink(store: Store, node: String, settings: Memo<SatelliteNode>) -> impl IntoView {
    let held = Signal::derive(move || settings.get().transmitter.is_some());
    let locked = Signal::derive(move || settings.get().tuning_locked);
    let frozen = Signal::derive(move || held.get() || locked.get());
    let hz = Signal::derive(move || settings.get().downlink_hz.unwrap_or(0.0));
    let tune = {
        let node = node.clone();
        move |hz: f64| {
            edit(store, &node, move |held| {
                held.downlink_hz = Some(hz);
                held.uplink_hz = None;
                held.transmitter = None;
            });
        }
    };
    let selected = {
        let node = node.clone();
        Signal::derive(move || store.selected.get().as_deref() == Some(node.as_str()))
    };
    let spec = DialSpec {
        hz,
        reach: Signal::stored(REACH),
        disabled: frozen,
        wheel: selected,
    };
    let dial = {
        move || match settings.get().downlink_hz {
            None => AnyView::new(view! { text(class = "hint") {"No downlink"} }),
            Some(_) => AnyView::new(frequency_dial(spec, tune.clone())),
        }
    };
    let hold = Signal::derive(move || held.get().then(|| text::HELD_BY_SIGNAL.to_owned()));
    let lock = move |on: bool| edit(store, &node, move |held| held.tuning_locked = on);
    titled(
        "Downlink before Doppler",
        AnyView::new(view! {
            row(class = "satellite__downlink kit-dial-row") {
                {dial}
                spacer() {}
                {tuning_lock(frozen, "Downlink locked", "Lock downlink", hold, lock)}
            }
        }),
    )
}

fn tracked(
    store: Store,
    node: String,
    settings: Memo<SatelliteNode>,
    status: Signal<Option<SatelliteStatus>, LocalStorage>,
) -> impl IntoView {
    kit_audio::install();
    kit_channel::install();
    kit_sources::install();
    head::show(store, node.clone(), move || {
        status
            .get()
            .and_then(|status| status.name.or(status.catalog))
            .map_or_else(NodeHead::default, NodeHead::said)
    });
    let catalog = Signal::derive_local(move || status.get().and_then(|status| status.catalog));
    let refresh_node = node.clone();
    let change_node = node.clone();
    let age = Signal::derive_local(move || status.get().and_then(|status| status.tle_age_days));
    let refreshing = RwSignal::new(false);
    let failed = RwSignal::new(None::<String>);
    let refresh_hint = move || {
        failed
            .get()
            .unwrap_or_else(|| String::from("Fetch the newest elements"))
    };
    view! {
        {downlink(store, node.clone(), settings)}
        {signals(store, node, settings, catalog)}
        {readout(status)}
        {move || status.get().and_then(|status| status.error).map(|error| view! { text(class = "geo__bad") {{error}} })}
        row(class = "face__foot rec-foot") {
            {kit_audio::stats(move || elements_stat(age.get()).into_iter().collect())}
            {move || {
                let refresh_node = refresh_node.clone();
                catalog.get().is_some().then(|| titled(refresh_hint(), AnyView::new(view! {
                    control(
                        class = "btn",
                        state:disabled = move || refreshing.get(),
                        on:click:stop = move |_| {
                            if !refreshing.get_untracked() {
                                refresh(store, refresh_node.clone(), catalog.get_untracked(), (refreshing, failed));
                            }
                        }
                    ) {"Refresh elements"}
                })))
            }}
            control(
                class = "btn",
                on:click:stop = move |_| edit(store, &change_node, |held| *held = SatelliteNode::default())
            ) {"Change satellite"}
        }
    }
}

fn refresh(
    store: Store,
    node: String,
    catalog: Option<String>,
    (busy, failed): (RwSignal<bool>, RwSignal<Option<String>>),
) {
    let Some(catalog) = catalog else {
        return;
    };
    busy.set(true);
    zgui::task::spawn_local(async move {
        let path = format!("/api/satellites?q={}", encode(&catalog));
        let answer = store.api().get::<SatelliteCatalogResponse>(&path).await;
        busy.try_set(false);
        match answer {
            Ok(found) => {
                failed.try_set(None);
                match found
                    .satellites
                    .into_iter()
                    .find(|satellite| satellite.catalog == catalog)
                {
                    Some(fresh) => edit(store, &node, move |held| held.tle = Some(fresh.tle)),
                    None => store.say(format!("{catalog} is not in the catalogue")),
                }
            }
            Err(error) => {
                failed.try_set(Some(error.to_string()));
                store.say(format!("cannot refresh the elements: {error}"));
            }
        }
    });
}

fn signals(
    store: Store,
    node: String,
    settings: Memo<SatelliteNode>,
    catalog: Signal<Option<String>, LocalStorage>,
) -> impl IntoView {
    let listed = RwSignal::new(None::<TransmittersResponse>);
    let loading = RenderEffect::new(move |_| {
        let Some(catalog) = catalog.get() else {
            listed.set(None);
            return;
        };
        zgui::task::spawn_local(async move {
            let path = format!("/api/satellites/{}/transmitters", encode(&catalog));
            match store.api().get::<TransmittersResponse>(&path).await {
                Ok(found) => listed.set(Some(found)),
                Err(error) => store.say(format!("cannot list the transmitters: {error}")),
            }
        });
    });
    on_cleanup_local(move || drop(loading));
    move || {
        let chosen = settings.get().transmitter;
        let found = listed.get()?;
        let shown = text::shown_signals(&found.transmitters, chosen.as_deref());
        if shown.is_empty() {
            return None;
        }
        let title = text::signal_title(Some(found.source.as_str()));
        let mut options = vec![(String::new(), String::from("Own frequency"))];
        options.extend(
            shown
                .iter()
                .map(|signal| (signal.id.clone(), text::transmitter_label(signal))),
        );
        let transmitters: Vec<_> = shown.into_iter().cloned().collect();
        let node = node.clone();
        let choice = searchable_pick(
            "Signal",
            options,
            Signal::derive(move || Some(settings.get().transmitter.unwrap_or_default())),
            move |id: String| {
                let picked = transmitters.iter().find(|signal| signal.id == id).cloned();
                edit(store, &node, move |held| match picked {
                    Some(signal) => {
                        held.transmitter = Some(signal.id);
                        held.downlink_hz = signal.downlink_hz;
                        held.uplink_hz = signal.uplink_hz;
                    }
                    None => held.transmitter = None,
                });
            },
        );
        Some(titled(title, AnyView::new(row_field("Signal", choice))))
    }
}

fn readout(status: Signal<Option<SatelliteStatus>, LocalStorage>) -> impl IntoView {
    let now = ticker(Duration::from_secs(1));
    move || {
        let status = status.get()?;
        let mut rows: Vec<AnyView> = Vec::new();
        if let Some(look) = status.look {
            let text = format!(
                "{:.0}° {}, {:.1}° up",
                look.azimuth_deg,
                text::compass(look.azimuth_deg),
                look.elevation_deg
            );
            let visible = status.visible;
            rows.push(AnyView::new(row_field(
                "Look",
                view! { text(class = "mono", class:on = visible) {{text}} },
            )));
            rows.push(AnyView::new(row_field(
                "Range",
                format!("{:.0} km", look.range_km),
            )));
        }
        if let Some(shift) = status.doppler_hz {
            let rate = status
                .doppler_rate_hz_s
                .map_or_else(String::new, |rate| format!(", {}/s", text::doppler(rate)));
            rows.push(titled(
                "Added to every wired decoder",
                AnyView::new(row_field(
                    "Doppler",
                    format!("{}{rate}", text::doppler(shift)),
                )),
            ));
        }
        if let Some(uplink) = status.uplink_hz {
            rows.push(titled(
                "The uplink corrected so the satellite hears it on frequency",
                AnyView::new(row_field("Send on", mhz(uplink))),
            ));
        }
        if status.look.is_some() {
            let pass = status.next_pass;
            rows.push(AnyView::new(row_field(
                "Next pass",
                view! { text(class = "mono") {{move || text::pass_line(pass.as_ref(), now.get() / 1_000)}} },
            )));
        }
        (!rows.is_empty()).then(|| view! { column(class = "satellite__readout") {{rows}} })
    }
}

const FACE_SHEET: &str = css!(
    r#"
.satellite { gap: 8px; }
.satellite__pick { gap: 6px; }
.satellite__hits { gap: 1px; }
.satellite__hit {
    align-items: center;
    padding: 3px 6px;
    border-radius: 4px;
    font-size: 12px;
    color: var(--ink-dim);
    display: flex;
}
.satellite__hit:hover { background-color: var(--panel-3); color: var(--ink); }
.satellite__downlink { align-items: center; gap: 6px; }
.satellite__readout { gap: 2px; }
.satellite__readout .on { color: var(--accent); }
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_age_warns_once_the_elements_go_stale() {
        assert_eq!(elements_stat(None), None);
        let fresh = elements_stat(Some(2.04)).expect("a stat");
        assert_eq!(
            (fresh.value.as_deref(), fresh.tone),
            (Some("2.0 d"), Tone::Plain)
        );
        let stale = elements_stat(Some(9.0)).expect("a stat");
        assert_eq!(stale.tone, Tone::Warn);
    }
}
