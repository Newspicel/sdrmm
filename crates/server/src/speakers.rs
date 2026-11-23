use std::{
    collections::{HashMap, hash_map::Entry},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use sdrmm_audio_out::Output;
use sdrmm_engine::{Engine, PcmBlock};
use sdrmm_wire::{
    AUDIO_PORT, AudioRoute, NodeBody, NodeProblem, PatchGraph, ProblemSource, ResolvedPatch,
};
use tokio::{runtime::Handle, sync::broadcast, task::JoinHandle};

use crate::{AppState, resolved::Resolved};

mod feed;

/// How often a speaker picks up a channel that started late, or a card that came back.
const RECHECK: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq)]
struct Wanted {
    device: Option<String>,
    /// Each wire's linear gain.
    routes: HashMap<AudioRoute, f32>,
}

/// One wire feeding the card, with a gain the fader turns while it plays.
struct Wire {
    task: JoinHandle<()>,
    gain: Arc<AtomicU32>,
}

struct Playing {
    device: Option<String>,
    output: Output,
    wires: HashMap<AudioRoute, Wire>,
}

impl Playing {
    fn open(device: Option<String>) -> Result<Self, String> {
        let output = Output::open(device.as_deref()).map_err(|error| error.to_string())?;
        Ok(Self {
            device,
            output,
            wires: HashMap::new(),
        })
    }

    fn unplug(&mut self, keep: impl Fn(&AudioRoute) -> bool) {
        self.wires.retain(|route, wire| {
            let kept = keep(route) && !wire.task.is_finished();
            if !kept {
                wire.task.abort();
            }
            kept
        });
    }
}

impl Drop for Playing {
    fn drop(&mut self) {
        self.unplug(|_| false);
    }
}

/// Speakers that play on the server's own sound card.
#[derive(Default)]
pub(crate) struct Speakers {
    playing: HashMap<String, Playing>,
}

pub(crate) fn follow(state: &AppState) {
    let Ok(runtime) = Handle::try_current() else {
        return;
    };
    let speakers = Arc::new(Mutex::new(Speakers::default()));
    let held = speakers.clone();
    let handle = runtime.clone();
    crate::resolved::follow(state, "speakers", move |state| {
        lock(&held).reconcile(state, &handle);
    });
    let state = state.clone();
    runtime.clone().spawn(async move {
        let mut tick = tokio::time::interval(RECHECK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let (state, speakers, runtime) = (state.clone(), speakers.clone(), runtime.clone());
            let step = move || lock(&speakers).reconcile(&state, &runtime);
            if tokio::task::spawn_blocking(step).await.is_err() {
                return;
            }
        }
    });
}

fn lock(speakers: &Mutex<Speakers>) -> std::sync::MutexGuard<'_, Speakers> {
    speakers.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Speakers {
    fn reconcile(&mut self, state: &AppState, runtime: &Handle) {
        let current = state.resolved.current();
        let sync_fx = || crate::audio_fx::sync(&state.engine, &state.resolved);
        let problems = self.settle(&state.engine, &current, &sync_fx, runtime);
        state
            .problems
            .replace(&state.engine, ProblemSource::Speaker, &problems);
    }

    fn settle(
        &mut self,
        engine: &Engine,
        current: &Resolved,
        sync_fx: &dyn Fn(),
        runtime: &Handle,
    ) -> Vec<NodeProblem> {
        let wanted = wanted(&current.graph, &current.patch);
        let mut problems = Vec::new();
        self.playing.retain(|node, playing| {
            let fault = playing.output.fault();
            if let Some(text) = fault.clone() {
                problems.push(problem(node, text));
            }
            fault.is_none()
                && wanted
                    .get(node)
                    .is_some_and(|want| want.device == playing.device)
        });
        for (node, want) in wanted {
            if problems.iter().any(|problem| problem.node == node) {
                continue;
            }
            if let Err(text) = self.play(engine, &node, want, sync_fx, runtime) {
                problems.push(problem(&node, text));
            }
        }
        problems
    }

    fn play(
        &mut self,
        engine: &Engine,
        node: &str,
        want: Wanted,
        sync_fx: &dyn Fn(),
        runtime: &Handle,
    ) -> Result<(), String> {
        let playing = match self.playing.entry(node.to_owned()) {
            Entry::Occupied(held) => held.into_mut(),
            Entry::Vacant(free) => free.insert(Playing::open(want.device)?),
        };
        playing.unplug(|route| want.routes.contains_key(route));
        for (route, gain) in want.routes {
            if let Some(wire) = playing.wires.get(&route) {
                wire.gain.store(gain.to_bits(), Ordering::Relaxed);
                continue;
            }
            let Some(pcm) = subscribe(engine, &route, sync_fx)? else {
                continue;
            };
            let voice = playing.output.voice().map_err(|error| error.to_string())?;
            let gain = Arc::new(AtomicU32::new(gain.to_bits()));
            let task = runtime.spawn(feed::run(pcm, voice, playing.output.rate(), gain.clone()));
            playing.wires.insert(route, Wire { task, gain });
        }
        Ok(())
    }
}

/// The channel's audio, or None while the channel is not running yet.
fn subscribe(
    engine: &Engine,
    route: &AudioRoute,
    sync_fx: &dyn Fn(),
) -> Result<Option<broadcast::Receiver<PcmBlock>>, String> {
    if !route.fx.is_empty() {
        sync_fx();
    }
    match engine.subscribe_route_pcm(route) {
        Ok(pcm) => Ok(Some(pcm)),
        Err(error) if error.is_not_found() => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn problem(node: &str, text: String) -> NodeProblem {
    NodeProblem {
        node: node.to_owned(),
        source: ProblemSource::Speaker,
        text,
    }
}

fn wanted(graph: &PatchGraph, patch: &ResolvedPatch) -> HashMap<String, Wanted> {
    graph
        .nodes
        .iter()
        .filter_map(|node| {
            let NodeBody::Speaker(speaker) = &node.body else {
                return None;
            };
            let device = speaker.server_output()?.map(str::to_owned);
            let routes: HashMap<AudioRoute, f32> = patch
                .feeds(&node.id, AUDIO_PORT)
                .iter()
                .flat_map(|feed| {
                    let fx = graph.audio_fx(&feed.source);
                    let gain = speaker.gain(&feed.source);
                    feed.tap
                        .iter()
                        .flat_map(sdrmm_wire::Tap::channels)
                        .map(move |addr| {
                            let route = AudioRoute {
                                device_set: addr.device_set,
                                channel: addr.channel,
                                fx: fx.clone(),
                            };
                            (route, gain)
                        })
                })
                .collect();
            (!routes.is_empty()).then(|| (node.id.clone(), Wanted { device, routes }))
        })
        .collect()
}

#[cfg(test)]
mod tests;
