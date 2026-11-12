use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use sdrmm_engine::{Engine, PcmBlock, PcmPayload, TransmitRequest, TransmitSource};
use sdrmm_wire::{
    AUDIO_PORT, BASEBAND_PORT, IQ_PORT, NodeBody, NodeProblem, ProblemSource, TRANSMIT_PORT,
    TransmitStatus,
};
use tokio::sync::broadcast::error::RecvError;

use crate::{AppState, resolved::Resolved, rest::AppError};

/// A channel's audio carried into an armed transmitter.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Link {
    radio: u32,
    transmitter: String,
    device_set: u32,
    channel: u32,
}

/// The audio wires that key transmitters: one forwarding thread per wired channel.
#[derive(Default)]
pub(crate) struct TransmitHub {
    links: Mutex<HashMap<Link, Arc<AtomicBool>>>,
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
    let Some(feeder) = wired_into(resolved, node, BASEBAND_PORT) else {
        return Err("Wire a modulator, a channel or a lane into it".to_owned());
    };
    let tapped = resolved
        .patch
        .first_feed(node, BASEBAND_PORT)
        .and_then(|feed| feed.tap.as_ref()?.channels().into_iter().next());
    if let Some(addr) = tapped {
        return Ok(TransmitSource::Channel {
            device_set: addr.device_set,
            channel: addr.channel,
        });
    }
    match resolved.graph.node(feeder).map(|found| &found.body) {
        Some(NodeBody::Modulator(modulator)) if modulator.channel_type.is_empty() => {
            Err("Pick the modulator's mode".to_owned())
        }
        Some(NodeBody::Modulator(modulator)) => Ok(TransmitSource::Modulator(modulator.params())),
        _ => Err("The wired channel is not running".to_owned()),
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

/// The armed transmitters a modulator feeds, each with the radio it drives.
fn transmitters_of(engine: &Engine, resolved: &Resolved, modulator: &str) -> Vec<(u32, String)> {
    reached_from(resolved, modulator, BASEBAND_PORT)
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
        Some(NodeBody::Modulator(_)) => {}
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

/// Carries a microphone's audio to every armed transmitter its wires reach. Returns how many
/// took it.
pub(crate) fn speak(state: &AppState, microphone: &str, pcm: &[f32]) -> usize {
    let resolved = state.resolved.current();
    if !matches!(
        resolved.graph.node(microphone).map(|found| &found.body),
        Some(NodeBody::Microphone(_))
    ) {
        return 0;
    }
    reached_from(&resolved, microphone, AUDIO_PORT)
        .flat_map(|modulator| transmitters_of(&state.engine, &resolved, modulator))
        .filter(|(device_set, transmitter)| {
            state
                .engine
                .transmit_audio(*device_set, transmitter, pcm.to_vec())
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
                .patch_transmitter(device_set, &armed.node, now.request.settings)
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
    /// Starts a forwarder for every channel wired into an armed transmitter's modulator and
    /// stops the ones the patch no longer asks for.
    fn relink(&self, state: &AppState, resolved: &Resolved) {
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
        let Some(modulator) = wired_into(resolved, &armed.node, BASEBAND_PORT) else {
            continue;
        };
        for feed in resolved.patch.feeds(modulator, AUDIO_PORT) {
            for addr in feed.tap.iter().flat_map(sdrmm_wire::Tap::channels) {
                links.push(Link {
                    radio,
                    transmitter: armed.node.clone(),
                    device_set: addr.device_set,
                    channel: addr.channel,
                });
            }
        }
    }
    links
}

fn mono(block: &PcmBlock) -> Option<Vec<f32>> {
    let PcmPayload::Samples(samples) = &block.payload else {
        return None;
    };
    let channels = usize::from(block.channels.max(1));
    Some(
        samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect(),
    )
}

fn forward(engine: &Arc<Engine>, link: &Link) -> Result<Arc<AtomicBool>, String> {
    let mut audio = engine
        .subscribe_pcm(link.device_set, link.channel)
        .map_err(|error| error.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
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
                    .transmit_audio(link.radio, &link.transmitter, pcm)
                    .is_err()
                {
                    return;
                }
            }
        })
        .map_err(|error| format!("spawn audio forwarder: {error}"))?;
    Ok(stop)
}
