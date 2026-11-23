use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use sdrmm_decoder::encoder::EncoderParams;
use sdrmm_dsp::audio::{downmix::mono_mix, mix::lane_of, voice_gate::VoiceGate};
use sdrmm_engine::{Engine, PcmBlock, PcmPayload, TransmitRequest, TransmitSource};
use sdrmm_wire::{
    AUDIO_PORT, AudioFrom, AudioProcessing, AudioRoute, BASEBAND_PORT, IQ_PORT, MicrophoneNode,
    NodeBody, NodeProblem, ProblemSource, SendMode, TRANSMIT_PORT, Tap, TransmitStatus,
};
use tokio::sync::{broadcast::error::RecvError, watch};

use crate::{AppState, resolved::Resolved, rest::AppError};

/// A channel's audio carried into an armed transmitter.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Link {
    radio: u32,
    transmitter: String,
    route: AudioRoute,
}

/// The audio wires that key transmitters: one forwarding thread per wired channel.
#[derive(Default)]
pub(crate) struct TransmitHub {
    links: Mutex<HashMap<Link, Arc<AtomicBool>>>,
    gates: Mutex<HashMap<String, VoiceGate>>,
    armed: watch::Sender<u64>,
}

/// The radio a transmitter drives and what the patch wires into it.
#[derive(Debug, PartialEq)]
pub(crate) struct Wired {
    pub(crate) device_set: u32,
    pub(crate) request: TransmitRequest,
}

fn wired_into<'a>(resolved: &'a Resolved, node: &str, port: &str) -> Option<&'a str> {
    resolved
        .patch
        .first_feed(node, port)
        .map(|feed| feed.source.as_str())
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

fn source(resolved: &Resolved, node: &str) -> Result<TransmitSource, String> {
    let lane = resolved
        .patch
        .first_feed(node, IQ_PORT)
        .and_then(|feed| feed.tap.as_ref()?.lane());
    if let Some((device_set, stream)) = lane {
        return Ok(TransmitSource::Lane { device_set, stream });
    }
    if wired_into(resolved, node, IQ_PORT).is_some() {
        return Err("The wired lane is not running".to_owned());
    }
    let Some(feed) = resolved.patch.first_feed(node, BASEBAND_PORT) else {
        return Err("Wire an encoder, a channel or a lane into it".to_owned());
    };
    let encoder = match &feed.tap {
        Some(Tap::Node { node, .. }) => node,
        Some(tap) => {
            let addr = tap
                .channels()
                .into_iter()
                .next()
                .ok_or_else(|| "The wired channel is not running".to_owned())?;
            return Ok(TransmitSource::Channel {
                device_set: addr.device_set,
                channel: addr.channel,
            });
        }
        None => return Err("The wired channel is not running".to_owned()),
    };
    match resolved.graph.node(encoder).map(|found| &found.body) {
        Some(NodeBody::Encoder(encoder)) if encoder.encoder_type.is_empty() => {
            Err("Pick the encoder's mode".to_owned())
        }
        Some(NodeBody::Encoder(encoder)) => Ok(TransmitSource::Encoder(EncoderParams {
            type_id: encoder.encoder_type.clone(),
            settings: encoder.settings.clone(),
        })),
        _ => Err("The wired channel is not running".to_owned()),
    }
}

/// What the encoder feeding `node` runs its audio through.
fn encoder_audio(resolved: &Resolved, node: &str) -> AudioProcessing {
    let feeder = wired_into(resolved, node, BASEBAND_PORT);
    match feeder
        .and_then(|id| resolved.graph.node(id))
        .map(|found| &found.body)
    {
        Some(NodeBody::Encoder(encoder)) => encoder.audio.clone(),
        _ => AudioProcessing::default(),
    }
}

pub(crate) fn wired(resolved: &Resolved, node: &str) -> Result<Wired, String> {
    let Some(NodeBody::Transmit(body)) = resolved.graph.node(node).map(|found| &found.body) else {
        return Err(format!("{node} is not a transmitter"));
    };
    let radio = reached_from(resolved, node, TRANSMIT_PORT)
        .next()
        .ok_or_else(|| "Wire it to a radio's transmit input".to_owned())?;
    let device_set = resolved
        .patch
        .device_set(radio)
        .ok_or_else(|| "That radio is not open".to_owned())?;
    Ok(Wired {
        device_set,
        request: TransmitRequest {
            node: node.to_owned(),
            source: source(resolved, node)?,
            settings: body.settings.clone(),
            audio: encoder_audio(resolved, node),
        },
    })
}

pub(crate) fn arm(state: &AppState, node: &str) -> Result<TransmitStatus, AppError> {
    let resolved = state.resolved.current();
    let wired = wired(&resolved, node).map_err(AppError::bad_request)?;
    disarm_elsewhere(&state.engine, node, wired.device_set);
    let status = state
        .engine
        .arm_transmitter(wired.device_set, &wired.request)?;
    state.transmit.relink(state, &resolved);
    Ok(status)
}

fn disarm_elsewhere(engine: &Engine, node: &str, keep: u32) {
    for (device_set, request) in engine.transmitters() {
        if request.node == node
            && device_set != keep
            && let Err(error) = engine.disarm_transmitter(device_set, node)
        {
            tracing::debug!(%error, node, "a moved transmitter was already off");
        }
    }
}

fn armed_on(engine: &Engine, node: &str) -> Option<u32> {
    engine
        .transmitters()
        .into_iter()
        .find(|(_, request)| request.node == node)
        .map(|(device_set, _)| device_set)
}

pub(crate) fn disarm(state: &AppState, node: &str) -> Result<(), AppError> {
    let device_set =
        armed_on(&state.engine, node).ok_or_else(|| AppError::bad_request("Not armed"))?;
    state.engine.disarm_transmitter(device_set, node)?;
    state.transmit.relink(state, &state.resolved.current());
    Ok(())
}

/// The armed transmitters an encoder feeds, each with the radio it drives.
fn transmitters_of(engine: &Engine, resolved: &Resolved, encoder: &str) -> Vec<(u32, String)> {
    reached_from(resolved, encoder, BASEBAND_PORT)
        .filter_map(|transmitter| Some((armed_on(engine, transmitter)?, transmitter.to_owned())))
        .collect()
}

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
    let transmitters = transmitters_of(&state.engine, &resolved, node);
    if transmitters.is_empty() {
        return Err(AppError::bad_request("Arm the transmitter it is wired to"));
    }
    for (device_set, transmitter) in transmitters {
        state
            .engine
            .transmit_message(device_set, &transmitter, message)?;
    }
    Ok(())
}

/// Carries a microphone's 48 kHz audio to the armed transmitters its wires reach, if its send
/// mode lets the frame through. Returns how many took it.
pub(crate) fn speak(state: &AppState, microphone: &str, pcm: &[f32]) -> usize {
    let resolved = state.resolved.current();
    let Some(NodeBody::Microphone(node)) = resolved.graph.node(microphone).map(|found| &found.body)
    else {
        return 0;
    };
    if !state.transmit.passes(microphone, node, pcm) {
        return 0;
    }
    state.engine.speak_voice(microphone, pcm);
    speak_as(state, microphone, lane_of(microphone), pcm)
}

/// Whether `source`'s audio out reaches an armed transmitter through an encoder.
pub(crate) fn reaches_armed(engine: &Engine, resolved: &Resolved, source: &str) -> bool {
    reached_from(resolved, source, AUDIO_PORT)
        .any(|encoder| !transmitters_of(engine, resolved, encoder).is_empty())
}

/// Carries audio from any node whose audio out reaches armed transmitters through an encoder,
/// on its own mixing lane. Returns how many took it.
pub(crate) fn speak_as(state: &AppState, source: &str, lane: u64, pcm: &[f32]) -> usize {
    let resolved = state.resolved.current();
    reached_from(&resolved, source, AUDIO_PORT)
        .flat_map(|encoder| transmitters_of(&state.engine, &resolved, encoder))
        .filter(|(device_set, transmitter)| {
            state
                .engine
                .transmit_audio(*device_set, transmitter, lane, pcm.to_vec())
                .is_ok()
        })
        .count()
}

/// Brings every armed transmitter back in line with the patch: one whose wiring is gone comes
/// off the air, one whose source changed is armed again and one whose settings moved follows.
pub(crate) fn reconcile(state: &AppState) {
    let resolved = state.resolved.current();
    let mut problems = Vec::new();
    for (device_set, armed) in state.engine.transmitters() {
        let followed = match wired(&resolved, &armed.node) {
            Ok(now) if now.device_set == device_set && now.request == armed => Ok(()),
            Ok(now) if now.device_set == device_set && now.request.source == armed.source => state
                .engine
                .patch_transmitter(
                    device_set,
                    &armed.node,
                    now.request.settings,
                    now.request.audio,
                )
                .map(drop),
            Ok(now) if now.device_set == device_set => state
                .engine
                .arm_transmitter(device_set, &now.request)
                .map(drop),
            _ => state.engine.disarm_transmitter(device_set, &armed.node),
        };
        if let Err(error) = followed {
            problems.push(NodeProblem {
                node: armed.node.clone(),
                source: ProblemSource::Transmit,
                text: error.to_string(),
            });
        }
    }
    state
        .problems
        .replace(&state.engine, ProblemSource::Transmit, &problems);
    state.transmit.relink(state, &resolved);
}

impl TransmitHub {
    /// Ticks whenever transmitters were armed, disarmed or relinked, so nodes that key them
    /// look again at once.
    pub(crate) fn subscribe_armed(&self) -> watch::Receiver<u64> {
        self.armed.subscribe()
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

    /// Starts a forwarder for every channel wired into an armed transmitter's encoder and
    /// stops the ones the patch no longer asks for.
    fn relink(&self, state: &AppState, resolved: &Resolved) {
        self.armed
            .send_modify(|count| *count = count.wrapping_add(1));
        self.forget_gates_of_gone_microphones(resolved);
        let wanted = wanted_links(&state.engine, resolved);
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
            match forward(&state.engine, &link) {
                Ok(stop) => {
                    links.insert(link, stop);
                }
                Err(error) => tracing::warn!(?link, %error, "channel audio not carried"),
            }
        }
    }
}

fn wanted_links(engine: &Engine, resolved: &Resolved) -> Vec<Link> {
    let mut links = Vec::new();
    for (radio, armed) in engine.transmitters() {
        let Some(encoder) = wired_into(resolved, &armed.node, BASEBAND_PORT) else {
            continue;
        };
        let channels = resolved
            .patch
            .feeds(encoder, AUDIO_PORT)
            .iter()
            .flat_map(|feed| feed.audio_routes(&resolved.graph))
            .filter(|route| route.channel_addr().is_some());
        for route in channels {
            links.push(Link {
                radio,
                transmitter: armed.node.clone(),
                route,
            });
        }
    }
    links
}

fn mono(block: &PcmBlock) -> Option<Vec<f32>> {
    let PcmPayload::Samples(samples) = &block.payload else {
        return None;
    };
    Some(mono_mix(samples, usize::from(block.channels)))
}

/// The mixing lane of one channel's audio, the same for every node that carries it.
pub(crate) fn route_lane(route: &AudioRoute) -> u64 {
    let from = match &route.from {
        AudioFrom::Channel {
            device_set,
            channel,
        } => format!("{device_set}/{channel}"),
        AudioFrom::Voice { node, .. } => format!("voice/{node}"),
    };
    lane_of(&format!("{from}/{}", route.fx.join(",")))
}

fn forward(engine: &Arc<Engine>, link: &Link) -> Result<Arc<AtomicBool>, String> {
    let mut audio = engine
        .subscribe_route_pcm(&link.route)
        .map_err(|error| error.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let lane = route_lane(&link.route);
    let (engine, link, halted) = (engine.clone(), link.clone(), stop.clone());
    std::thread::Builder::new()
        .name("sdrmm-tx-audio".to_owned())
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
                if engine
                    .transmit_audio(link.radio, &link.transmitter, lane, pcm)
                    .is_err()
                {
                    return;
                }
            }
        })
        .map_err(|error| format!("spawn audio forwarder: {error}"))?;
    Ok(stop)
}
