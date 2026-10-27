use std::collections::{BTreeSet, HashMap};

use sdrmm_wire::{
    AudioRoute, ChannelAddr, Feed, PortType, ResolvedPatch, ServerEvent, StreamKind, StreamParams,
};
use tokio::task::JoinHandle;

use super::{
    MEDIA_ID_BASE, SPECTRUM_ID_BASE, Session, SpectrumLane, alloc_stream_id,
    derived::{spawn_iq_spectrum, spawn_pcm_spectrum},
    flatten_join, spawn_audio, spawn_iq, spawn_spectrum, spawn_symbols, spawn_video, text_event,
};

const NO_NODE: &str = "No such node";
const NO_PORT: &str = "No such port";
const NOT_WIRED: &str = "Not wired";
const NOT_RUNNING: &str = "Not running";
const NO_STREAM_IDS: &str = "Too many streams";
const TOO_MANY: &str = "Too many ports watched";
pub(super) const MAX_WATCHED: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Tapped {
    Spectrum { device_set: u32, stream: u32 },
    Audio(AudioRoute),
    AudioSpectrum(AudioRoute),
    Iq(ChannelAddr),
    BasebandSpectrum(ChannelAddr),
    Symbols(ChannelAddr),
    Video(ChannelAddr),
}

impl Tapped {
    const fn derived(&self) -> bool {
        matches!(self, Self::AudioSpectrum(_) | Self::BasebandSpectrum(_))
    }

    const fn kind(&self) -> StreamKind {
        match self {
            Self::Spectrum { .. } | Self::AudioSpectrum(_) | Self::BasebandSpectrum(_) => {
                StreamKind::Spectrum
            }
            Self::Audio(_) => StreamKind::Audio,
            Self::Iq(_) => StreamKind::Iq,
            Self::Symbols(_) => StreamKind::Symbols,
            Self::Video(_) => StreamKind::Video,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Upstream {
    pub(super) source: String,
    pub(super) via: Vec<String>,
    pub(super) index: u32,
}

impl Upstream {
    fn of(source: &str) -> Self {
        Self {
            source: source.to_owned(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Wanted {
    pub(super) tapped: Tapped,
    pub(super) from: Upstream,
}

struct Live {
    id: u16,
    from: Upstream,
    task: JoinHandle<()>,
}

struct PortSub {
    params: StreamParams,
    streams: HashMap<Tapped, Live>,
    refused: Option<String>,
}

#[derive(Default)]
pub(super) struct Ports {
    subs: HashMap<(String, String), PortSub>,
}

impl Ports {
    pub(super) fn ids(&self, spectrum: bool) -> impl Iterator<Item = u16> + '_ {
        self.subs
            .values()
            .flat_map(|sub| sub.streams.iter())
            .filter(move |(tapped, _)| (tapped.kind() == StreamKind::Spectrum) == spectrum)
            .map(|(_, live)| live.id)
    }

    #[cfg(test)]
    pub(super) fn occupy(&mut self, ids: impl Iterator<Item = u16>) {
        let streams = ids
            .map(|id| {
                let tapped = Tapped::Video(ChannelAddr {
                    device_set: u32::from(id),
                    channel: 0,
                });
                let live = Live {
                    id,
                    from: Upstream::default(),
                    task: tokio::spawn(async {}),
                };
                (tapped, live)
            })
            .collect();
        self.subs.insert(
            (String::new(), String::new()),
            PortSub {
                params: StreamParams::default(),
                streams,
                refused: None,
            },
        );
    }

    pub(super) fn abort(self) {
        for sub in self.subs.into_values() {
            for live in sub.streams.into_values() {
                live.task.abort();
            }
        }
    }
}

fn stream_started(
    key: &(String, String),
    stream_id: u16,
    kind: StreamKind,
    from: &Upstream,
) -> ServerEvent {
    ServerEvent::PortStream {
        stream_id,
        node: key.0.clone(),
        port: key.1.clone(),
        source: from.source.clone(),
        via: from.via.clone(),
        index: from.index,
        kind,
    }
}

pub(super) fn wanted(patch: &ResolvedPatch, node: &str, port: &str) -> Result<Vec<Wanted>, String> {
    let found = patch.node(node).ok_or(NO_NODE)?;
    if let Some(input) = found.input(port) {
        let wanted: Vec<Wanted> = input.feeds.iter().flat_map(of_feed).collect();
        if wanted.is_empty() {
            let reason = if input.feeds.is_empty() {
                NOT_WIRED
            } else {
                NOT_RUNNING
            };
            return Err(reason.to_owned());
        }
        return Ok(wanted);
    }
    let output = found.output(port).ok_or(NO_PORT)?;
    let own = own_output(patch, node, port, output.spec.port_type);
    if own.is_empty() {
        return Err(NOT_RUNNING.to_owned());
    }
    Ok(own)
}

fn of_feed(feed: &Feed) -> Vec<Wanted> {
    let Some(tap) = &feed.tap else {
        return Vec::new();
    };
    let upstream = |index: u32| Upstream {
        source: feed.source.clone(),
        via: feed.via.clone(),
        index,
    };
    if let Some((device_set, stream)) = tap.lane() {
        return vec![Wanted {
            tapped: Tapped::Spectrum { device_set, stream },
            from: upstream(0),
        }];
    }
    (0..)
        .zip(tap.channels())
        .flat_map(|(index, addr)| {
            let from = upstream(index);
            channel_streams(feed.payload.port_type(), addr, &feed.via)
                .into_iter()
                .map(move |tapped| Wanted {
                    tapped,
                    from: from.clone(),
                })
        })
        .collect()
}

fn channel_streams(port_type: PortType, addr: ChannelAddr, via: &[String]) -> Vec<Tapped> {
    match port_type {
        PortType::Audio => {
            let route = AudioRoute {
                device_set: addr.device_set,
                channel: addr.channel,
                fx: via.to_vec(),
            };
            vec![Tapped::Audio(route.clone()), Tapped::AudioSpectrum(route)]
        }
        PortType::Baseband => vec![
            Tapped::Iq(addr),
            Tapped::Symbols(addr),
            Tapped::BasebandSpectrum(addr),
        ],
        PortType::Video => vec![Tapped::Video(addr)],
        _ => Vec::new(),
    }
}

fn own_output(patch: &ResolvedPatch, node: &str, port: &str, port_type: PortType) -> Vec<Wanted> {
    let tapped = match port_type {
        PortType::Iq => patch.device_set(node).map(|device_set| {
            vec![Tapped::Spectrum {
                device_set,
                stream: patch
                    .node(node)
                    .and_then(|held| held.output(port))
                    .and_then(|output| output.spec.stream)
                    .unwrap_or_default(),
            }]
        }),
        _ => patch
            .channel(node)
            .map(|addr| channel_streams(port_type, addr, &[])),
    };
    tapped
        .unwrap_or_default()
        .into_iter()
        .map(|tapped| Wanted {
            tapped,
            from: Upstream::of(node),
        })
        .collect()
}

impl Session {
    pub(super) async fn subscribe_port(
        &mut self,
        node: String,
        port: String,
        params: StreamParams,
    ) {
        let key = (node.clone(), port.clone());
        let retuned = match self.ports.subs.get_mut(&key) {
            Some(sub) => std::mem::replace(&mut sub.params, params) != params,
            None => {
                if self.ports.subs.len() >= MAX_WATCHED {
                    self.send_error(TOO_MANY).await;
                    return;
                }
                self.ports.subs.insert(
                    key.clone(),
                    PortSub {
                        params,
                        streams: HashMap::new(),
                        refused: None,
                    },
                );
                false
            }
        };
        let held = self.live_ids(&key);
        if retuned {
            self.drop_spectrum(&key).await;
        }
        self.sync_port(&node, &port).await;
        if self.live_ids(&key) == held {
            self.announce(&key).await;
        }
    }

    fn live_ids(&self, key: &(String, String)) -> BTreeSet<u16> {
        self.ports
            .subs
            .get(key)
            .map(|sub| sub.streams.values().map(|live| live.id).collect())
            .unwrap_or_default()
    }

    async fn announce(&self, key: &(String, String)) {
        let Some(sub) = self.ports.subs.get(key) else {
            return;
        };
        for (tapped, live) in &sub.streams {
            let started = stream_started(key, live.id, tapped.kind(), &live.from);
            let _ = self.out.send(text_event(&started)).await;
        }
    }

    async fn drop_spectrum(&mut self, key: &(String, String)) {
        let Some(sub) = self.ports.subs.get_mut(key) else {
            return;
        };
        let stale: Vec<(Tapped, Live)> = sub
            .streams
            .extract_if(|tapped, _| tapped.kind() == StreamKind::Spectrum)
            .collect();
        for (tapped, live) in stale {
            self.stop_stream(&tapped, live).await;
        }
    }

    pub(super) async fn unsubscribe_port(&mut self, node: &str, port: &str) {
        let Some(sub) = self.ports.subs.remove(&(node.to_owned(), port.to_owned())) else {
            return;
        };
        for (tapped, live) in sub.streams {
            self.stop_stream(&tapped, live).await;
        }
    }

    pub(super) async fn sync_ports(&mut self) {
        let keys: Vec<(String, String)> = self.ports.subs.keys().cloned().collect();
        for (node, port) in keys {
            self.sync_port(&node, &port).await;
        }
    }

    async fn sync_port(&mut self, node: &str, port: &str) {
        let key = (node.to_owned(), port.to_owned());
        let resolved = self.state.resolved.current();
        let wanted = wanted(&resolved.patch, node, port);
        let Some(sub) = self.ports.subs.get_mut(&key) else {
            return;
        };
        let derived = sub.params.spectrum;
        let keep: Vec<Wanted> = wanted
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|want| derived || !want.tapped.derived())
            .collect();
        let gone: Vec<(Tapped, Live)> = sub
            .streams
            .extract_if(|tapped, live| {
                live.task.is_finished()
                    || !keep
                        .iter()
                        .any(|want| want.tapped == *tapped && want.from == live.from)
            })
            .collect();
        for (tapped, live) in gone {
            self.stop_stream(&tapped, live).await;
        }
        let mut refusal = wanted.err();
        for want in keep {
            if let Err(reason) = self.start_stream(&key, want).await {
                refusal = Some(reason);
            }
        }
        let vanished = refusal.as_deref() == Some(NO_NODE);
        self.refuse(&key, refusal).await;
        if vanished {
            self.ports.subs.remove(&key);
        }
    }

    async fn refuse(&mut self, key: &(String, String), refusal: Option<String>) {
        let Some(sub) = self.ports.subs.get_mut(key) else {
            return;
        };
        if sub.refused == refusal {
            return;
        }
        sub.refused.clone_from(&refusal);
        if let Some(reason) = refusal {
            let refused = ServerEvent::PortRefused {
                node: key.0.clone(),
                port: key.1.clone(),
                reason,
            };
            let _ = self.out.send(text_event(&refused)).await;
        }
    }

    async fn start_stream(&mut self, key: &(String, String), want: Wanted) -> Result<(), String> {
        let Some(sub) = self.ports.subs.get(key) else {
            return Ok(());
        };
        if sub.streams.contains_key(&want.tapped) {
            return Ok(());
        }
        let params = sub.params;
        let kind = want.tapped.kind();
        let id = self.port_stream_id(kind).ok_or(NO_STREAM_IDS)?;
        let task = self.spawn_tapped(id, &want.tapped, params).await?;
        let started = stream_started(key, id, kind, &want.from);
        let _ = self.out.send(text_event(&started)).await;
        if let Some(sub) = self.ports.subs.get_mut(key) {
            sub.streams.insert(
                want.tapped,
                Live {
                    id,
                    from: want.from,
                    task,
                },
            );
        } else {
            task.abort();
        }
        Ok(())
    }

    async fn stop_stream(&self, tapped: &Tapped, live: Live) {
        let ended = live.task.is_finished();
        live.task.abort();
        if ended {
            return;
        }
        let stopped = ServerEvent::StreamStopped {
            stream_id: live.id,
            kind: tapped.kind(),
        };
        let _ = self.out.send(text_event(&stopped)).await;
    }

    fn port_stream_id(&mut self, kind: StreamKind) -> Option<u16> {
        if kind == StreamKind::Spectrum {
            let live: Vec<u16> = self.ports.ids(true).collect();
            return alloc_stream_id(
                &mut self.next_spectrum_id,
                SPECTRUM_ID_BASE..=MEDIA_ID_BASE - 1,
                |id| live.contains(&id),
            );
        }
        self.media_stream_id()
    }

    async fn spawn_tapped(
        &self,
        id: u16,
        tapped: &Tapped,
        params: StreamParams,
    ) -> Result<JoinHandle<()>, String> {
        let engine = self.engine.clone();
        let out = self.out.clone();
        match tapped.clone() {
            Tapped::Spectrum { device_set, stream } => {
                let rx = flatten_join(
                    tokio::task::spawn_blocking(move || {
                        engine.subscribe_spectrum(device_set, stream)
                    })
                    .await,
                )?;
                let lane = SpectrumLane {
                    stream_id: id,
                    device_set,
                    stream,
                };
                Ok(spawn_spectrum(
                    lane,
                    params.fps,
                    params.bins,
                    rx,
                    out,
                    self.engine.clone(),
                ))
            }
            Tapped::Audio(route) => {
                let resolved = self.state.resolved.clone();
                let rx = flatten_join(
                    tokio::task::spawn_blocking(move || {
                        if !route.fx.is_empty() {
                            crate::audio_fx::sync(&engine, &resolved);
                        }
                        engine.subscribe_route_audio(&route)
                    })
                    .await,
                )?;
                Ok(spawn_audio(id, rx, out))
            }
            Tapped::Iq(at) => {
                let rx = flatten_join(
                    tokio::task::spawn_blocking(move || {
                        engine.subscribe_iq(at.device_set, at.channel)
                    })
                    .await,
                )?;
                Ok(spawn_iq(id, rx, out))
            }
            Tapped::BasebandSpectrum(at) => {
                let rx = flatten_join(
                    tokio::task::spawn_blocking(move || {
                        engine.subscribe_iq(at.device_set, at.channel)
                    })
                    .await,
                )?;
                Ok(spawn_iq_spectrum(id, params.fps, params.bins, rx, out))
            }
            Tapped::AudioSpectrum(route) => {
                let resolved = self.state.resolved.clone();
                let rx = flatten_join(
                    tokio::task::spawn_blocking(move || {
                        if !route.fx.is_empty() {
                            crate::audio_fx::sync(&engine, &resolved);
                        }
                        engine.subscribe_route_pcm(&route)
                    })
                    .await,
                )?;
                Ok(spawn_pcm_spectrum(id, params.fps, params.bins, rx, out))
            }
            Tapped::Symbols(at) => {
                let rx = flatten_join(
                    tokio::task::spawn_blocking(move || {
                        engine.subscribe_symbols(at.device_set, at.channel)
                    })
                    .await,
                )?;
                Ok(spawn_symbols(id, rx, out))
            }
            Tapped::Video(at) => {
                let rx = flatten_join(
                    tokio::task::spawn_blocking(move || {
                        engine.subscribe_video(at.device_set, at.channel)
                    })
                    .await,
                )?;
                Ok(spawn_video(id, rx, out))
            }
        }
    }
}
