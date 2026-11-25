//! Stream out nodes: each encodes its channel's pictures and sound as an MPEG transport
//! stream, served to players over HTTP or sent to one address over UDP or RTP.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc},
    thread,
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
};
use sdrmm_engine::{Engine, PcmBlock, VideoPacket};
use sdrmm_wire::{
    AUDIO_PORT, AudioRoute, ChannelAddr, NodeBody, PatchGraph, ResolvedPatch, ServerEvent,
    StreamOutNode, VIDEO_PORT,
};
use tokio::{
    runtime::Handle,
    sync::broadcast::{self, error::RecvError},
    task::JoinHandle,
};

use crate::{AppState, resolved::Resolved, rest::AppError};

mod encode;
mod send;
mod stream;

#[cfg(test)]
mod tests;

use encode::Input;
use stream::Stream;

/// How often a node picks up a channel that started late, and how often its status goes out.
const RECHECK: Duration = Duration::from_secs(1);
/// Pictures and sound blocks waiting for the encoder before more are dropped.
const INPUT_QUEUE: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Wanted {
    settings: StreamOutNode,
    video: ChannelAddr,
    audio: Option<AudioRoute>,
}

/// One node's encoder and the wires feeding it.
struct Running {
    wanted: Wanted,
    stream: Arc<Stream>,
    input: mpsc::SyncSender<Input>,
    encoder: thread::JoinHandle<()>,
    video: Option<JoinHandle<()>>,
    audio: Option<JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        for task in [&self.video, &self.audio].into_iter().flatten() {
            task.abort();
        }
    }
}

/// Every running Stream out node, by node id.
#[derive(Default)]
pub(crate) struct StreamHub {
    running: Mutex<HashMap<String, Running>>,
}

impl StreamHub {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Running>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new viewer of `node`'s HTTP stream, or None when it serves none.
    fn watch(&self, node: &str) -> Option<broadcast::Receiver<Bytes>> {
        self.lock()
            .get(node)
            .filter(|running| !running.wanted.settings.transport.sends())
            .map(|running| running.stream.watch())
    }

    fn settle(&self, engine: &Engine, current: &Resolved, runtime: &Handle, sync_fx: &dyn Fn()) {
        let wanted = wanted(&current.graph, &current.patch);
        let mut running = self.lock();
        running.retain(|node, run| {
            !run.encoder.is_finished() && wanted.get(node).is_some_and(|want| *want == run.wanted)
        });
        for (node, want) in wanted {
            let run = running
                .entry(node.clone())
                .or_insert_with(|| Running::start(node, want));
            run.plug(engine, runtime, sync_fx);
        }
    }

    fn report(&self, engine: &Engine) {
        for run in self.lock().values() {
            engine.emit_event(ServerEvent::StreamOutStatus(
                run.stream.status(&run.wanted.settings),
            ));
        }
    }
}

impl Running {
    fn start(node: String, wanted: Wanted) -> Self {
        let stream = Arc::new(Stream::new(node));
        let (input, queue) = mpsc::sync_channel(INPUT_QUEUE);
        let worker = stream.clone();
        let settings = wanted.settings.clone();
        let with_audio = wanted.audio.is_some();
        let encoder = thread::Builder::new()
            .name("stream-out".to_owned())
            .spawn(move || encode::run(&queue, &worker, &settings, with_audio));
        let encoder = encoder.unwrap_or_else(|error| {
            stream.fail(format!("Encoder thread: {error}"));
            thread::spawn(|| {})
        });
        Self {
            wanted,
            stream,
            input,
            encoder,
            video: None,
            audio: None,
        }
    }

    /// Subscribes the wires whose channel runs now and were not subscribed yet.
    fn plug(&mut self, engine: &Engine, runtime: &Handle, sync_fx: &dyn Fn()) {
        if self.video.as_ref().is_none_or(JoinHandle::is_finished) {
            let at = self.wanted.video;
            self.video = match engine.subscribe_video(at.device_set, at.channel) {
                Ok(rx) => {
                    Some(runtime.spawn(forward_video(rx, self.input.clone(), self.stream.clone())))
                }
                Err(error) if error.is_not_found() => None,
                Err(error) => {
                    self.stream.fail(error.to_string());
                    None
                }
            };
        }
        let Some(route) = &self.wanted.audio else {
            return;
        };
        if self.audio.as_ref().is_none_or(JoinHandle::is_finished) {
            if route.fx.is_some() {
                sync_fx();
            }
            self.audio = match engine.subscribe_route_pcm(route) {
                Ok(rx) => {
                    Some(runtime.spawn(forward_audio(rx, self.input.clone(), self.stream.clone())))
                }
                Err(error) if error.is_not_found() => None,
                Err(error) => {
                    self.stream.fail(error.to_string());
                    None
                }
            };
        }
    }
}

async fn forward_video(
    mut rx: broadcast::Receiver<VideoPacket>,
    input: mpsc::SyncSender<Input>,
    stream: Arc<Stream>,
) {
    loop {
        match rx.recv().await {
            Ok(packet) => {
                if !offer(&input, &stream, Input::Picture(packet.picture)) {
                    return;
                }
            }
            Err(RecvError::Lagged(count)) => stream.dropped(count),
            Err(RecvError::Closed) => return,
        }
    }
}

async fn forward_audio(
    mut rx: broadcast::Receiver<PcmBlock>,
    input: mpsc::SyncSender<Input>,
    stream: Arc<Stream>,
) {
    loop {
        match rx.recv().await {
            Ok(block) => {
                if !offer(&input, &stream, Input::Sound(block)) {
                    return;
                }
            }
            Err(RecvError::Lagged(count)) => stream.dropped(count),
            Err(RecvError::Closed) => return,
        }
    }
}

/// Queues `item` for the encoder, counting it as dropped when the encoder is behind. False
/// once the encoder has stopped.
fn offer(input: &mpsc::SyncSender<Input>, stream: &Stream, item: Input) -> bool {
    match input.try_send(item) {
        Ok(()) => true,
        Err(mpsc::TrySendError::Full(_)) => {
            stream.dropped(1);
            true
        }
        Err(mpsc::TrySendError::Disconnected(_)) => false,
    }
}

fn wanted(graph: &PatchGraph, patch: &ResolvedPatch) -> HashMap<String, Wanted> {
    graph
        .nodes
        .iter()
        .filter_map(|node| {
            let NodeBody::StreamOut(settings) = &node.body else {
                return None;
            };
            let (pictured, video) = first_channel(patch, &node.id, VIDEO_PORT)?;
            let audio = patch
                .feeds(&node.id, AUDIO_PORT)
                .iter()
                .find_map(|feed| feed.audio_routes(graph).into_iter().next())
                .or_else(|| crate::audio_fx::video_sound(graph, &pictured, video));
            let want = Wanted {
                settings: settings.clone(),
                video,
                audio,
            };
            Some((node.id.clone(), want))
        })
        .collect()
}

/// The node feeding `port` and the first channel it taps.
fn first_channel(patch: &ResolvedPatch, node: &str, port: &str) -> Option<(String, ChannelAddr)> {
    patch.feeds(node, port).iter().find_map(|feed| {
        let at = feed.tap.iter().flat_map(sdrmm_wire::Tap::channels).next()?;
        Some((feed.source.clone(), at))
    })
}

pub(crate) fn follow(state: &AppState) {
    let Ok(runtime) = Handle::try_current() else {
        return;
    };
    let handle = runtime.clone();
    crate::resolved::follow(state, "stream out", move |state| {
        settle(state, &handle);
    });
    let state = state.clone();
    runtime.clone().spawn(async move {
        let mut tick = tokio::time::interval(RECHECK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let (state, runtime) = (state.clone(), runtime.clone());
            let step = move || {
                settle(&state, &runtime);
                state.streams.report(&state.engine);
            };
            if tokio::task::spawn_blocking(step).await.is_err() {
                return;
            }
        }
    });
}

fn settle(state: &AppState, runtime: &Handle) {
    let current = state.resolved.current();
    let sync_fx = || crate::audio_fx::sync(&state.engine, &state.resolved);
    state
        .streams
        .settle(&state.engine, &current, runtime, &sync_fx);
}

#[utoipa::path(
    get, path = "/api/streams/{node}",
    params(("node" = String, Path, description = "Stream out node id")),
    responses(
        (status = 200, description = "The node's live MPEG transport stream", content_type = "video/mp2t"),
        (status = 404, description = "The node serves no stream over HTTP", body = sdrmm_wire::ApiError),
    ),
)]
pub(crate) async fn serve(
    State(state): State<AppState>,
    Path(node): Path<String>,
) -> Result<Response, AppError> {
    let rx = state
        .streams
        .watch(&node)
        .ok_or_else(|| AppError::not_found(format!("{node} serves no stream")))?;
    let body = Body::from_stream(futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(bytes) => return Some((Ok::<_, std::convert::Infallible>(bytes), rx)),
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return None,
            }
        }
    }));
    let headers = [
        (header::CONTENT_TYPE, "video/mp2t"),
        (header::CACHE_CONTROL, "no-store"),
    ];
    Ok((headers, body).into_response())
}
