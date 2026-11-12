use std::{
    collections::HashSet,
    sync::{Arc, atomic},
    time::{Duration, Instant},
};

use axum::{
    Extension,
    extract::{
        State,
        ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use sdrmm_engine::{AudioPacket, Engine, IqBlock, SymbolBlock, VideoPacket};
use sdrmm_wire::{
    API_PROTOCOL, AudioFrame, ClientCommand, IqFrame, PositionFix, ServerEvent, StateScope,
    StreamKind, SymbolFrame, VideoFrame, WS_CLOSE_REVOKED, WS_SUBPROTOCOL, phone::POSE_BURST,
    phone::POSE_RATE_HZ,
};
use tokio::sync::{broadcast, watch};

mod derived;
pub(crate) mod jpeg;
mod outbox;
mod phone;
mod ports;
mod presence;
mod sinks;
pub(crate) mod spectrum;
mod surfaces;
pub(crate) mod webtransport;
use outbox::Outbox;
use phone::{PhoneLink, RateBudget, revoked};
use ports::Ports;
use presence::PeerLink;
use sinks::Sinks;
use surfaces::Surfaces;

use crate::{
    AppState,
    auth::Identity,
    phones::{phone_command, phone_event},
};

const MIN_BINS: usize = 16;
const MAX_BINS: usize = 4096;
const MAX_FPS: u16 = 60;
const MEDIA_ID_BASE: u16 = 0x8000;
const SPECTRUM_ID_BASE: u16 = 0;
const POSE_NEEDS_PHONE: &str = "only a paired phone can publish a pose";
const POSE_TOO_FAST: &str = "pose updates are limited to 20 Hz";
const POSE_FAILED: &str = "could not publish the pose";
const NOT_FOR_PHONES: &str = "Not open to phones";
const LIMIT_ERROR_EVERY: Duration = Duration::from_secs(1);
const CLOSE_FLUSH: Duration = Duration::from_secs(1);
const MAX_INCOMING_BYTES: usize = 1 << 20;
const DRAIN_LIMIT: usize = 128;
pub(crate) const MAX_SOCKETS: u32 = 64;
const TOO_MANY_SOCKETS: &str = "Too many clients";

pub(crate) async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Response {
    if state.clients.load(atomic::Ordering::Relaxed) >= MAX_SOCKETS {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(sdrmm_wire::ApiError {
                error: TOO_MANY_SOCKETS.to_owned(),
                detail: None,
                code: Some(sdrmm_wire::ErrorCode::Unavailable),
            }),
        )
            .into_response();
    }
    ws.protocols([WS_SUBPROTOCOL])
        .max_message_size(MAX_INCOMING_BYTES)
        .max_frame_size(MAX_INCOMING_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, state, identity, String::new()))
}

struct Session {
    engine: Arc<Engine>,
    state: AppState,
    out: Outbox,
    surfaces: Surfaces,
    sinks: Sinks,
    ports: Ports,
    next_spectrum_id: u16,
    next_media_id: u16,
    diagnostics: Option<tokio::task::JoinHandle<()>>,
    identity: Identity,
    pose_budget: RateBudget,
    last_limit_error: Option<Instant>,
    peer: PeerLink,
    path: Option<PathProbe>,
}

impl Session {
    fn new(
        engine: Arc<Engine>,
        state: AppState,
        out: Outbox,
        identity: Identity,
        peer: PeerLink,
    ) -> Self {
        Self {
            engine,
            state,
            out,
            surfaces: Surfaces::default(),
            sinks: Sinks::default(),
            ports: Ports::default(),
            next_spectrum_id: SPECTRUM_ID_BASE,
            next_media_id: MEDIA_ID_BASE,
            diagnostics: None,
            identity,
            pose_budget: RateBudget::new(f64::from(POSE_BURST), f64::from(POSE_RATE_HZ)),
            last_limit_error: None,
            peer,
            path: None,
        }
    }

    fn media_stream_id(&mut self) -> Option<u16> {
        let live: HashSet<u16> = self
            .surfaces
            .live_ids()
            .chain(self.ports.ids(false))
            .collect();
        alloc_stream_id(&mut self.next_media_id, MEDIA_ID_BASE..=u16::MAX, |id| {
            live.contains(&id)
        })
    }

    async fn send_error(&self, message: impl Into<String>) {
        let err = ServerEvent::Error {
            message: message.into(),
        };
        let _ = self.out.send(text_event(&err)).await;
    }

    async fn dispatch(&mut self, text: &str) {
        let Ok(command) = serde_json::from_str::<ClientCommand>(text) else {
            self.send_error("invalid command").await;
            return;
        };
        if self.identity.phone().is_some() && !phone_command(&command) {
            self.send_error(NOT_FOR_PHONES).await;
            return;
        }
        match command {
            ClientCommand::SubscribeDiagnostics { enabled } => {
                if let Some(task) = self.diagnostics.take() {
                    task.abort();
                }
                if enabled {
                    self.diagnostics = Some(spawn_diagnostics(
                        self.engine.clone(),
                        self.out.clone(),
                        self.path.clone(),
                    ));
                }
            }
            ClientCommand::SubscribeSurface { node, fit } => {
                self.subscribe_surface(node, fit).await;
            }
            ClientCommand::UnsubscribeSurface { node } => self.unsubscribe_surface(&node).await,
            ClientCommand::SubscribePort { node, port, params } => {
                self.subscribe_port(node, port, params).await;
            }
            ClientCommand::UnsubscribePort { node, port } => {
                self.unsubscribe_port(&node, &port).await;
            }
            ClientCommand::SubscribeSink { node } => self.subscribe_sink(node).await,
            ClientCommand::UnsubscribeSink { node } => self.unsubscribe_sink(&node),
            ClientCommand::PublishPose { fix, error } => self.publish_pose(fix, error).await,
            ClientCommand::Present { author, name } => self.present(&author, &name).await,
            ClientCommand::Point(pointer) => self.point(pointer).await,
        }
    }

    async fn publish_pose(&mut self, fix: Option<PositionFix>, error: Option<String>) {
        let Some(phone) = self.identity.phone().map(str::to_owned) else {
            self.send_error(POSE_NEEDS_PHONE).await;
            return;
        };
        if !self.pose_budget.take() {
            let now = Instant::now();
            if self
                .last_limit_error
                .is_none_or(|last| now.duration_since(last) >= LIMIT_ERROR_EVERY)
            {
                self.last_limit_error = Some(now);
                self.send_error(POSE_TOO_FAST).await;
            }
            return;
        }
        let state = self.state.clone();
        let published =
            tokio::task::spawn_blocking(move || state.gps.publish_pose(&state, &phone, fix, error))
                .await;
        match published {
            Ok(Ok(_)) => {}
            Ok(Err(message)) => self.send_error(message).await,
            Err(error) => {
                tracing::warn!(%error, "publishing a pose stopped");
                self.send_error(POSE_FAILED).await;
            }
        }
    }

    fn abort_streams(self) {
        self.peer.abort();
        if let Some(task) = self.diagnostics {
            task.abort();
        }
        self.surfaces.abort();
        self.sinks.abort();
        self.ports.abort();
    }
}

async fn handle_socket(socket: WebSocket, state: AppState, identity: Identity, fallback: String) {
    let (ws_tx, ws_rx) = socket.split();
    let (out_tx, out_rx) = outbox::channel();
    let link = Link {
        out: out_tx,
        writer: tokio::spawn(write_output(ws_tx, out_rx)),
        commands: Box::pin(socket_commands(ws_rx)),
        path: None,
    };
    serve_session(link, state, identity, fallback).await;
}

fn socket_commands(
    socket: impl futures::Stream<Item = Result<Message, axum::Error>>,
) -> impl futures::Stream<Item = String> {
    socket
        .take_while(|message| {
            std::future::ready(
                matches!(message, Ok(message) if !matches!(message, Message::Close(_))),
            )
        })
        .filter_map(|message| {
            std::future::ready(match message {
                Ok(Message::Text(text)) => Some(text.as_str().to_owned()),
                _ => None,
            })
        })
}

/// One client's connection, whichever transport carries it: what the session writes goes into
/// `out`, `writer` drains that to the wire, and `commands` ends when the client leaves.
struct Link {
    out: Outbox,
    writer: tokio::task::JoinHandle<()>,
    commands: std::pin::Pin<Box<dyn futures::Stream<Item = String> + Send>>,
    path: Option<PathProbe>,
}

/// Reads the network path of a transport that measures it.
type PathProbe = Arc<dyn Fn() -> PathSample + Send + Sync>;

#[derive(Clone, Copy, Default)]
struct PathSample {
    rtt: Duration,
    sent_packets: u64,
    lost_packets: u64,
}

fn link_health(before: PathSample, now: PathSample) -> sdrmm_wire::LinkHealth {
    let sent = now.sent_packets.saturating_sub(before.sent_packets);
    let lost = now.lost_packets.saturating_sub(before.lost_packets);
    sdrmm_wire::LinkHealth {
        rtt_ms: now.rtt.as_secs_f32() * 1000.0,
        loss: if sent == 0 {
            0.0
        } else {
            (lost as f32 / sent as f32).min(1.0)
        },
    }
}

async fn serve_session(transport: Link, state: AppState, identity: Identity, fallback: String) {
    let Link {
        out: out_tx,
        mut writer,
        mut commands,
        path,
    } = transport;
    let engine = state.engine.clone();
    let live = state.clients.fetch_add(1, atomic::Ordering::Relaxed) + 1;
    tracing::debug!(clients = live, "client connected");
    engine.emit_scope(StateScope::Clients);
    let link = PhoneLink::join(&state, &identity).await;
    let for_phone = link.is_some();

    let event_rx = engine.subscribe_events();
    let position_rx = state.gps.subscribe();
    greet(&state, &out_tx, &identity).await;

    let events = spawn_events(event_rx, out_tx.clone(), for_phone);
    let positions = spawn_positions(position_rx, out_tx.clone());

    let peer = state.presence.connect();
    let mut session = Session::new(
        engine.clone(),
        state.clone(),
        out_tx.clone(),
        identity,
        PeerLink::new(peer, fallback),
    );
    session.path = path;
    let mut revocation: Option<watch::Receiver<bool>> = link.as_ref().map(PhoneLink::revoked);
    let mut resolved = state.resolved.subscribe();
    let mut writer_done = false;

    let revoked_now = loop {
        let command = tokio::select! {
            _ = &mut writer => {
                writer_done = true;
                break false;
            }
            () = revoked(&mut revocation) => break true,
            Ok(()) = resolved.changed() => {
                session.sync_ports().await;
                continue;
            }
            command = commands.next() => command,
        };
        let Some(command) = command else {
            break false;
        };
        session.dispatch(&command).await;
    };

    if revoked_now {
        let close = Message::Close(Some(CloseFrame {
            code: WS_CLOSE_REVOKED,
            reason: "revoked".into(),
        }));
        let _ = out_tx.send(close).await;
    }
    session.abort_streams();
    events.abort();
    positions.abort();
    drop(out_tx);
    if !writer_done
        && tokio::time::timeout(CLOSE_FLUSH, &mut writer)
            .await
            .is_err()
    {
        writer.abort();
    }
    state.presence.leave(peer);
    if let Some(link) = link {
        link.leave(&state).await;
    }
    let live = state
        .clients
        .fetch_sub(1, atomic::Ordering::Relaxed)
        .saturating_sub(1);
    tracing::debug!(clients = live, "client disconnected");
    engine.emit_scope(StateScope::Clients);
}

async fn greet(state: &AppState, out: &Outbox, identity: &Identity) {
    let hello = ServerEvent::Hello {
        revision: state.engine.snapshot().revision,
        protocol: API_PROTOCOL,
        phone: identity.phone().map(str::to_owned),
    };
    let _ = out.send(text_event(&hello)).await;
    for position in state.gps.snapshot() {
        let _ = out.send(text_event(&position)).await;
    }
    if identity.phone().is_some() {
        return;
    }
    for satellite in state.satellites.snapshot() {
        let _ = out.send(text_event(&satellite)).await;
    }
    for rotator in state.rotators.snapshot() {
        let _ = out.send(text_event(&rotator)).await;
    }
    for schedule in state.schedules.snapshot() {
        let _ = out.send(text_event(&schedule)).await;
    }
}

fn flatten_join<T>(
    joined: Result<Result<T, sdrmm_engine::EngineError>, tokio::task::JoinError>,
) -> Result<T, String> {
    match joined {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(e.to_string()),
        Err(e) => Err(format!("engine task failed: {e}")),
    }
}

fn newest<T: Clone>(rx: &mut broadcast::Receiver<T>, held: &mut T) -> u64 {
    let mut skipped = 0;
    for _ in 0..DRAIN_LIMIT {
        match rx.try_recv() {
            Ok(next) => {
                *held = next;
                skipped += 1;
            }
            Err(broadcast::error::TryRecvError::Lagged(count)) => skipped += count,
            Err(_) => break,
        }
    }
    skipped
}

fn alloc_stream_id(
    next: &mut u16,
    range: std::ops::RangeInclusive<u16>,
    in_use: impl Fn(u16) -> bool,
) -> Option<u16> {
    let (first, last) = (*range.start(), *range.end());
    for _ in first..=last {
        let candidate = *next;
        *next = if candidate == last {
            first
        } else {
            candidate + 1
        };
        if !in_use(candidate) {
            return Some(candidate);
        }
    }
    None
}

fn spawn_events(
    mut event_rx: broadcast::Receiver<ServerEvent>,
    out_tx: Outbox,
    for_phone: bool,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(ev) => {
                    if for_phone && !phone_event(&ev) {
                        continue;
                    }
                    if out_tx.send(text_event(&ev)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "event stream lagged; forcing full state refetch");
                    let resync = ServerEvent::StateChanged {
                        scope: StateScope::All,
                    };
                    if out_tx.send(text_event(&resync)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

fn spawn_positions(
    mut position_rx: broadcast::Receiver<ServerEvent>,
    out_tx: Outbox,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match position_rx.recv().await {
                Ok(event) => {
                    if out_tx.send(text_event(&event)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "GPS event stream lagged");
                    let error = ServerEvent::Error {
                        message: "GPS updates were lost; waiting for the next fix".to_owned(),
                    };
                    if out_tx.send(text_event(&error)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

struct FrameThrottle {
    fps: f64,
    next: u64,
    last: u64,
}

impl FrameThrottle {
    fn new(fps: u16) -> Self {
        Self {
            fps: f64::from(fps),
            next: 0,
            last: 0,
        }
    }

    fn admit(&mut self, timestamp: u64, sample_rate: f32) -> bool {
        if timestamp < self.last {
            self.next = timestamp;
        }
        self.last = timestamp;
        if timestamp < self.next {
            return false;
        }
        let period = (f64::from(sample_rate) / self.fps).max(1.0) as u64;
        self.next = self.next.saturating_add(period).max(timestamp);
        true
    }
}

#[derive(Clone, Copy)]
struct SpectrumLane {
    stream_id: u16,
    device_set: u32,
    stream: u32,
}

fn spawn_audio(
    stream_id: u16,
    mut rx: broadcast::Receiver<AudioPacket>,
    out_tx: Outbox,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(packet) => {
                    if packet.created_at.elapsed() > std::time::Duration::from_millis(100)
                        || rx.len() >= 5
                    {
                        out_tx.dropped(1);
                        continue;
                    }
                    let frame = AudioFrame {
                        stream_id,
                        seq: packet.seq,
                        timestamp: packet.timestamp,
                        ch_layout: packet.channels,
                        opus: &packet.opus,
                    }
                    .encode();

                    if out_tx.send_frame(frame).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    out_tx.dropped(count);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    let stopped = ServerEvent::StreamStopped {
                        stream_id,
                        kind: StreamKind::Audio,
                    };
                    let _ = out_tx.send(text_event(&stopped)).await;
                    break;
                }
            }
        }
    })
}

fn spawn_symbols(
    stream_id: u16,
    mut rx: broadcast::Receiver<SymbolBlock>,
    out_tx: Outbox,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(mut block) => {
                    out_tx.dropped(newest(&mut rx, &mut block));
                    let frame = SymbolFrame {
                        stream_id,
                        seq: block.seq,
                        timestamp: block.timestamp,
                        plane: block.plane,
                        symbol_rate: block.symbol_rate,
                        evm: block.evm,
                        mer_db: block.mer_db,
                        margin: block.margin,
                        freq_error_hz: block.freq_error_hz,
                        reference: &block.reference,
                        symbols: &block.symbols,
                    }
                    .encode();

                    if out_tx.send_frame(frame).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    out_tx.dropped(count);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    let stopped = ServerEvent::StreamStopped {
                        stream_id,
                        kind: StreamKind::Symbols,
                    };
                    let _ = out_tx.send(text_event(&stopped)).await;
                    break;
                }
            }
        }
    })
}

/// Packs a block into sixteen bits a sample against its own peak, which the frame carries as
/// its scale: the strongest sample keeps full resolution whatever the level of the block.
fn quantize_iq(samples: &[num_complex::Complex<f32>], out: &mut Vec<i16>) -> f32 {
    let peak = samples.iter().fold(0f32, |peak, sample| {
        peak.max(sample.re.abs()).max(sample.im.abs())
    });
    let scale = if peak.is_finite() && peak > 0.0 {
        peak
    } else {
        1.0
    };
    let gain = f32::from(sdrmm_wire::IQ_FULL_SCALE) / scale;
    out.clear();
    out.extend(samples.iter().flat_map(|sample| {
        [
            (sample.re * gain).round() as i16,
            (sample.im * gain).round() as i16,
        ]
    }));
    scale
}

fn spawn_iq(
    stream_id: u16,
    mut rx: broadcast::Receiver<IqBlock>,
    out_tx: Outbox,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut samples: Vec<i16> = Vec::new();
        loop {
            match rx.recv().await {
                Ok(mut block) => {
                    out_tx.dropped(newest(&mut rx, &mut block));
                    let scale = quantize_iq(&block.samples, &mut samples);
                    let frame = IqFrame {
                        stream_id,
                        seq: block.seq,
                        timestamp: block.timestamp,
                        sample_rate: block.sample_rate,
                        center_hz: block.center_hz,
                        scale,
                        samples: &samples,
                    }
                    .encode();

                    if out_tx.send_frame(frame).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    out_tx.dropped(count);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    let stopped = ServerEvent::StreamStopped {
                        stream_id,
                        kind: StreamKind::Iq,
                    };
                    let _ = out_tx.send(text_event(&stopped)).await;
                    break;
                }
            }
        }
    })
}

async fn video_frame(
    stream_id: u16,
    packet: &VideoPacket,
    pictures: &Arc<jpeg::Memo>,
) -> Option<Result<Vec<u8>, sdrmm_wire::FrameError>> {
    let (memo, picture) = (pictures.clone(), packet.picture.clone());
    let jpeg = tokio::task::spawn_blocking(move || memo.encoded(&picture))
        .await
        .ok()??;
    Some(
        VideoFrame {
            stream_id,
            seq: packet.seq,
            timestamp: packet.timestamp,
            width: packet.picture.width,
            height: packet.picture.height,
            channels: if packet.picture.rgb.is_empty() {
                sdrmm_wire::VIDEO_GRAY
            } else {
                sdrmm_wire::VIDEO_RGB
            },
            jpeg: &jpeg,
        }
        .encode(),
    )
}

fn spawn_video(
    stream_id: u16,
    mut rx: broadcast::Receiver<VideoPacket>,
    out_tx: Outbox,
    pictures: Arc<jpeg::Memo>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(mut packet) => {
                    out_tx.dropped(newest(&mut rx, &mut packet));
                    let Some(frame) = video_frame(stream_id, &packet, &pictures).await else {
                        tracing::warn!(stream_id, "a picture could not be encoded");
                        out_tx.dropped(1);
                        continue;
                    };
                    if out_tx.send_frame(frame).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    out_tx.dropped(count);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    let stopped = ServerEvent::StreamStopped {
                        stream_id,
                        kind: StreamKind::Video,
                    };
                    let _ = out_tx.send(text_event(&stopped)).await;
                    break;
                }
            }
        }
    })
}

fn text_event(ev: &ServerEvent) -> Message {
    Message::Text(encode_event(ev))
}

pub(crate) fn encode_event(ev: &ServerEvent) -> Utf8Bytes {
    match serde_json::to_string(ev) {
        Ok(json) => json.into(),
        Err(_) => r#"{"type":"Error","data":{"message":"event serialization failed"}}"#.into(),
    }
}

#[cfg(test)]
mod tests;

fn spawn_diagnostics(
    engine: Arc<Engine>,
    out: Outbox,
    path: Option<PathProbe>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut before = path.as_ref().map(|probe| probe());
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let engine = engine.clone();
            let Ok(queues) = tokio::task::spawn_blocking(move || engine.pipeline_health()).await
            else {
                break;
            };
            let now = path.as_ref().map(|probe| probe());
            let link = before
                .zip(now)
                .map(|(before, now)| link_health(before, now));
            before = now;
            let event = ServerEvent::PipelineHealth {
                queues,
                websocket: out.health(),
                link,
            };
            if out.send(text_event(&event)).await.is_err() {
                break;
            }
        }
    })
}

async fn write_output<S: futures::Sink<Message> + Unpin>(mut sink: S, mut output: outbox::Output) {
    while let Some(message) = output.recv().await {
        if !matches!(
            tokio::time::timeout(outbox::WRITE_TIMEOUT, sink.send(message)).await,
            Ok(Ok(()))
        ) {
            break;
        }
    }
}
