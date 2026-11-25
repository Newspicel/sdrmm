//! Encoder nodes: each wired encoder runs in the engine on its own clock, and every voice and
//! channel wired into it reaches it there.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use sdrmm_decoder::encoder::EncoderParams;
use sdrmm_dsp::audio::{downmix::mono_mix, mix::lane_of, voice_gate::VoiceGate};
use sdrmm_engine::{Engine, PcmBlock, PcmPayload, encoders::EncoderSpec};
use sdrmm_wire::{
    AUDIO_PORT, AudioFrom, AudioRoute, BASEBAND_PORT, MicrophoneNode, NodeBody, NodeProblem,
    ProblemSource, SendMode,
};
use tokio::sync::{broadcast::error::RecvError, watch};

use crate::{AppState, resolved::Resolved, rest::AppError};

/// A channel's audio carried into a running encoder.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Link {
    encoder: String,
    route: AudioRoute,
}

/// What keeps encoders fed: one forwarding thread per wired channel, and each microphone's gate.
#[derive(Default)]
pub(crate) struct EncoderLinks {
    links: Mutex<HashMap<Link, Arc<AtomicBool>>>,
    gates: Mutex<HashMap<String, VoiceGate>>,
    changed: watch::Sender<u64>,
}

impl EncoderLinks {
    /// Ticks whenever encoders or transmitters change, so nodes that feed them look again.
    pub(crate) fn subscribe_changes(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    pub(crate) fn tick(&self) {
        self.changed
            .send_modify(|count| *count = count.wrapping_add(1));
    }

    fn passes(&self, microphone: &str, node: &MicrophoneNode, pcm: &[f32]) -> bool {
        if node.send != SendMode::VoiceActivity {
            return true;
        }
        let mut gates = self.gates.lock().unwrap_or_else(PoisonError::into_inner);
        let gate = gates
            .entry(microphone.to_owned())
            .or_insert_with(|| VoiceGate::new(node.threshold_db as f32, node.hang_ms));
        gate.set(node.threshold_db as f32, node.hang_ms);
        gate.feed(pcm)
    }

    fn forget_gates_of_gone_microphones(&self, resolved: &Resolved) {
        let mut gates = self.gates.lock().unwrap_or_else(PoisonError::into_inner);
        gates.retain(|id, _| {
            matches!(
                resolved.graph.node(id).map(|found| &found.body),
                Some(NodeBody::Microphone(node)) if node.send == SendMode::VoiceActivity
            )
        });
    }

    /// Starts a forwarder for every channel wired into a running encoder and stops the ones the
    /// patch no longer asks for.
    fn relink(&self, engine: &Arc<Engine>, resolved: &Resolved) {
        self.forget_gates_of_gone_microphones(resolved);
        let wanted = wanted_links(engine, resolved);
        let mut links = self.links.lock().unwrap_or_else(PoisonError::into_inner);
        links.retain(|link, stop| {
            let keep = wanted.contains(link);
            if !keep {
                stop.store(true, Ordering::Release);
            }
            keep
        });
        for link in wanted {
            if links.contains_key(&link) {
                continue;
            }
            match forward(engine, &link) {
                Ok(stop) => {
                    links.insert(link, stop);
                }
                Err(error) => tracing::warn!(?link, %error, "channel audio not carried"),
            }
        }
    }
}

/// The spec of an encoder node with a mode picked and something on its output.
fn wanted_spec(resolved: &Resolved, node: &str, body: &NodeBody) -> Option<EncoderSpec> {
    let NodeBody::Encoder(encoder) = body else {
        return None;
    };
    if encoder.encoder_type.is_empty() || resolved.patch.reaches(node, BASEBAND_PORT).is_empty() {
        return None;
    }
    Some(EncoderSpec {
        params: EncoderParams {
            type_id: encoder.encoder_type.clone(),
            settings: encoder.settings.clone(),
        },
        audio: encoder.audio.clone(),
    })
}

/// Runs every wired encoder of the patch and stops the rest.
pub(crate) fn settle(state: &AppState) {
    let resolved = state.resolved.current();
    let mut running = HashSet::new();
    let mut problems = Vec::new();
    for node in &resolved.graph.nodes {
        let Some(spec) = wanted_spec(&resolved, &node.id, &node.body) else {
            continue;
        };
        match state.engine.run_encoder(&node.id, &spec) {
            Ok(()) => {
                running.insert(node.id.clone());
            }
            Err(error) => problems.push(NodeProblem {
                node: node.id.clone(),
                source: ProblemSource::Encoder,
                text: error.to_string(),
            }),
        }
    }
    state.engine.retain_encoders(&running);
    state
        .problems
        .replace(&state.engine, ProblemSource::Encoder, &problems);
    state.encoders.relink(&state.engine, &resolved);
    state.encoders.tick();
}

fn reached_from<'a>(
    resolved: &'a Resolved,
    node: &str,
    port: &str,
) -> impl Iterator<Item = &'a str> {
    resolved
        .patch
        .reaches(node, port)
        .iter()
        .map(|reach| reach.sink.as_str())
}

/// Carries a microphone's 48 kHz audio to the voices and encoders its wires reach, if its send
/// mode lets the frame through. Returns how many encoders took it.
pub(crate) fn speak(state: &AppState, microphone: &str, pcm: &[f32]) -> usize {
    let resolved = state.resolved.current();
    let Some(NodeBody::Microphone(node)) = resolved.graph.node(microphone).map(|found| &found.body)
    else {
        return 0;
    };
    if !state.encoders.passes(microphone, node, pcm) {
        return 0;
    }
    state.engine.speak_voice(microphone, pcm);
    speak_as(state, microphone, lane_of(microphone), pcm)
}

/// Whether `source`'s audio out reaches an encoder that runs.
pub(crate) fn feeds_encoder(engine: &Engine, resolved: &Resolved, source: &str) -> bool {
    reached_from(resolved, source, AUDIO_PORT).any(|sink| engine.runs_encoder(sink))
}

/// Carries a voice to every running encoder its audio out reaches, on its own mixing lane.
/// Returns how many took it.
pub(crate) fn speak_as(state: &AppState, source: &str, lane: u64, pcm: &[f32]) -> usize {
    let resolved = state.resolved.current();
    reached_from(&resolved, source, AUDIO_PORT)
        .filter(|encoder| {
            state
                .engine
                .encoder_audio(encoder, lane, pcm.to_vec())
                .is_ok()
        })
        .count()
}

/// Queues a message on an encoder node.
pub(crate) fn send(
    state: &AppState,
    node: &str,
    message: &serde_json::Value,
) -> Result<(), AppError> {
    let resolved = state.resolved.current();
    match resolved.graph.node(node).map(|found| &found.body) {
        Some(NodeBody::Encoder(_)) => {}
        Some(_) => {
            return Err(AppError::bad_request(format!(
                "{node} takes no such action"
            )));
        }
        None => return Err(AppError::not_found(format!("No node {node}"))),
    }
    Ok(state.engine.encoder_message(node, message)?)
}

fn wanted_links(engine: &Engine, resolved: &Resolved) -> Vec<Link> {
    resolved
        .graph
        .nodes
        .iter()
        .filter(|node| engine.runs_encoder(&node.id))
        .flat_map(|node| {
            resolved
                .patch
                .feeds(&node.id, AUDIO_PORT)
                .iter()
                .flat_map(|feed| feed.audio_routes(&resolved.graph))
                .filter(|route| route.channel_addr().is_some())
                .map(|route| Link {
                    encoder: node.id.clone(),
                    route,
                })
        })
        .collect()
}

fn mono(block: &PcmBlock) -> Option<Vec<f32>> {
    let PcmPayload::Samples(samples) = &block.payload else {
        return None;
    };
    Some(mono_mix(samples, usize::from(block.channels)))
}

/// The mixing lane of one wire's audio, the same for every node that carries it.
pub(crate) fn route_lane(route: &AudioRoute) -> u64 {
    let from = match &route.from {
        AudioFrom::Channel {
            device_set,
            channel,
        } => format!("{device_set}/{channel}"),
        AudioFrom::Voice { node, .. } => format!("voice/{node}"),
    };
    lane_of(&format!(
        "{from}/{}",
        route.fx.as_deref().unwrap_or_default()
    ))
}

fn forward(engine: &Arc<Engine>, link: &Link) -> Result<Arc<AtomicBool>, String> {
    let mut audio = engine
        .subscribe_route_pcm(&link.route)
        .map_err(|error| error.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let lane = route_lane(&link.route);
    let (engine, link, halted) = (engine.clone(), link.clone(), stop.clone());
    std::thread::Builder::new()
        .name("sdrmm-encoder-audio".to_owned())
        .spawn(move || {
            while !halted.load(Ordering::Acquire) {
                let block = match audio.blocking_recv() {
                    Ok(block) => block,
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                };
                let Some(pcm) = mono(&block) else {
                    continue;
                };
                if engine.encoder_audio(&link.encoder, lane, pcm).is_err() {
                    return;
                }
            }
        })
        .map_err(|error| format!("spawn audio forwarder: {error}"))?;
    Ok(stop)
}
