//! Transmitters: each one sends what its wire carries, a lane, a channel or a running encoder.

use sdrmm_engine::{Engine, TransmitRequest, TransmitSource};
use sdrmm_wire::{
    BASEBAND_PORT, IQ_PORT, NodeBody, NodeProblem, ProblemSource, TRANSMIT_PORT, Tap,
    TransmitStatus,
};

use crate::{AppState, resolved::Resolved, rest::AppError};

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
    match &feed.tap {
        Some(Tap::Node { node, .. }) => encoded(resolved, node),
        Some(tap) => tap
            .channels()
            .into_iter()
            .next()
            .map(|addr| TransmitSource::Channel {
                device_set: addr.device_set,
                channel: addr.channel,
            })
            .ok_or_else(|| "The wired channel is not running".to_owned()),
        None => Err("The wired channel is not running".to_owned()),
    }
}

fn encoded(resolved: &Resolved, encoder: &str) -> Result<TransmitSource, String> {
    match resolved.graph.node(encoder).map(|found| &found.body) {
        Some(NodeBody::Encoder(body)) if body.encoder_type.is_empty() => {
            Err("Pick the encoder's mode".to_owned())
        }
        Some(NodeBody::Encoder(body)) => Ok(TransmitSource::Encoded {
            node: encoder.to_owned(),
            type_id: body.encoder_type.clone(),
        }),
        _ => Err("The wired encoder is gone".to_owned()),
    }
}

pub(crate) fn wired(resolved: &Resolved, node: &str) -> Result<Wired, String> {
    let Some(NodeBody::Transmit(body)) = resolved.graph.node(node).map(|found| &found.body) else {
        return Err(format!("{node} is not a transmitter"));
    };
    let radio = resolved
        .patch
        .reaches(node, TRANSMIT_PORT)
        .first()
        .map(|reach| reach.sink.as_str())
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
    crate::encoders::settle(state);
    let resolved = state.resolved.current();
    let wired = wired(&resolved, node).map_err(AppError::bad_request)?;
    disarm_elsewhere(&state.engine, node, wired.device_set);
    let status = state
        .engine
        .arm_transmitter(wired.device_set, &wired.request)?;
    state.encoders.tick();
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
    state.encoders.tick();
    Ok(())
}

/// Runs the patch's encoders, then brings every armed transmitter back in line: one whose
/// wiring is gone comes off the air, one whose source changed is armed again and one whose
/// settings moved follows.
pub(crate) fn reconcile(state: &AppState) {
    crate::encoders::settle(state);
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
                node: armed.node,
                source: ProblemSource::Transmit,
                text: error.to_string(),
            });
        }
    }
    state
        .problems
        .replace(&state.engine, ProblemSource::Transmit, &problems);
    state.encoders.tick();
}
