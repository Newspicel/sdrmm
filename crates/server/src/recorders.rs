use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use sdrmm_engine::Engine;
use sdrmm_wire::{
    AUDIO_PORT, AudioRoute, BASEBAND_PORT, IQ_PORT, NodeBody, NodeKind, PatchGraph, ResolvedPatch,
    StateScope, StateSnapshot,
};

use crate::{
    gps::GpsHub,
    problems::ProblemBoard,
    resolved::Resolved,
    rest::{lock_gate, reconcile_recordings},
    store::Store,
};

pub(crate) struct Hooks {
    pub(crate) store: Arc<Store>,
    pub(crate) gate: Arc<Mutex<()>>,
    pub(crate) gps: Arc<GpsHub>,
    pub(crate) problems: Arc<ProblemBoard>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Target {
    Audio(AudioRoute),
    Iq { device_set: u32 },
    Baseband { device_set: u32, channel: u32 },
}

#[derive(Default)]
pub(crate) struct Recorders {
    failures: HashMap<Target, String>,
}

struct Tick<'a> {
    engine: &'a Arc<Engine>,
    hooks: &'a Hooks,
    finished: bool,
    moved_iq: bool,
}

impl Recorders {
    pub(crate) fn reconcile(&mut self, engine: &Arc<Engine>, resolved: &Resolved, hooks: &Hooks) {
        let snapshot = engine.snapshot();
        let mut tick = Tick {
            engine,
            hooks,
            finished: false,
            moved_iq: false,
        };
        let mut wanted_targets = HashSet::new();
        let (graph, patch) = (&resolved.graph, &resolved.patch);
        self.audio(
            &mut tick,
            wanted_audio(graph, patch),
            &snapshot,
            &mut wanted_targets,
        );
        self.baseband(
            &mut tick,
            wanted_baseband(graph, patch),
            &snapshot,
            &mut wanted_targets,
        );
        self.iq(
            &mut tick,
            wanted_iq(graph, patch),
            &snapshot,
            &mut wanted_targets,
        );
        self.failures
            .retain(|target, _| wanted_targets.contains(target));
        tick.finish();
    }

    fn audio(
        &mut self,
        tick: &mut Tick<'_>,
        wanted: HashMap<AudioRoute, bool>,
        snapshot: &StateSnapshot,
        targets: &mut HashSet<Target>,
    ) {
        let current = running_audio(snapshot);
        for route in current.iter().filter(|route| !wanted.contains_key(*route)) {
            match tick.engine.stop_route_recording(route) {
                Ok(_) => tick.finished = true,
                Err(error) => tracing::warn!(?route, %error, "could not stop an audio recording"),
            }
        }
        for (route, &skip_silence) in &wanted {
            if current.contains(route) {
                if let Err(error) = tick.engine.set_route_skip_silence(route, skip_silence) {
                    tracing::warn!(?route, %error, "could not set silence skipping");
                }
                continue;
            }
            let result = tick
                .engine
                .start_route_recording(route, skip_silence)
                .map(|_| ());
            self.outcome(Target::Audio(route.clone()), result);
        }
        targets.extend(wanted.into_keys().map(Target::Audio));
    }

    fn baseband(
        &mut self,
        tick: &mut Tick<'_>,
        wanted: HashSet<(u32, u32)>,
        snapshot: &StateSnapshot,
        targets: &mut HashSet<Target>,
    ) {
        let current = running_baseband(snapshot);
        for &(device_set, channel) in current.difference(&wanted) {
            match tick
                .engine
                .stop_channel_baseband_recording(device_set, channel)
            {
                Ok(_) => tick.finished = true,
                Err(error) => {
                    tracing::warn!(device_set, channel, %error, "could not stop a baseband recording");
                }
            }
        }
        for &(device_set, channel) in wanted.difference(&current) {
            let result = tick
                .engine
                .start_channel_baseband_recording(device_set, channel)
                .map(|_| ());
            self.outcome(
                Target::Baseband {
                    device_set,
                    channel,
                },
                result,
            );
        }
        targets.extend(
            wanted
                .into_iter()
                .map(|(device_set, channel)| Target::Baseband {
                    device_set,
                    channel,
                }),
        );
    }

    fn iq(
        &mut self,
        tick: &mut Tick<'_>,
        wanted: HashMap<u32, u32>,
        snapshot: &StateSnapshot,
        targets: &mut HashSet<Target>,
    ) {
        let current = running_iq(snapshot);
        for (&device_set, &stream) in &current {
            if wanted.get(&device_set) == Some(&stream) {
                continue;
            }
            match tick.engine.stop_recording(device_set) {
                Ok(_) => {
                    tick.finished = true;
                    tick.moved_iq = true;
                }
                Err(error) => tracing::warn!(device_set, %error, "could not stop an IQ recording"),
            }
        }
        for (&device_set, &stream) in &wanted {
            if current.get(&device_set) == Some(&stream) {
                continue;
            }
            let result = tick.engine.start_recording(device_set, stream);
            if result.is_ok() {
                tick.moved_iq = true;
            }
            self.outcome(Target::Iq { device_set }, result);
        }
        targets.extend(
            wanted
                .into_keys()
                .map(|device_set| Target::Iq { device_set }),
        );
    }

    fn outcome(&mut self, target: Target, result: Result<(), sdrmm_engine::EngineError>) {
        match result {
            Ok(()) => {
                self.failures.remove(&target);
            }
            Err(error) => {
                let error = error.to_string();
                if self.failures.get(&target) != Some(&error) {
                    tracing::warn!(?target, %error, "recorder cannot record");
                    self.failures.insert(target, error);
                }
            }
        }
    }
}

impl Tick<'_> {
    fn finish(self) {
        if self.moved_iq {
            self.hooks.gps.route_for(self.engine, &self.hooks.problems);
        }
        if !self.finished {
            return;
        }
        if let Some(dir) = self.engine.recordings_dir() {
            let _gate = lock_gate(&self.hooks.gate);
            if let Err(error) = reconcile_recordings(dir, &self.hooks.store) {
                tracing::warn!(?error, "could not index a finished recording");
            }
        }
        self.engine.emit_scope(StateScope::Recordings);
    }
}

fn recording(body: &NodeBody) -> bool {
    match body {
        NodeBody::Recorder(recorder) | NodeBody::BasebandRecorder(recorder) => recorder.recording,
        NodeBody::AudioRecorder(recorder) => recorder.recording,
        _ => false,
    }
}

fn skips_silence(body: &NodeBody) -> bool {
    matches!(body, NodeBody::AudioRecorder(recorder) if recorder.skip_silence)
}

fn switched_on(graph: &PatchGraph, kind: NodeKind) -> impl Iterator<Item = &sdrmm_wire::PatchNode> {
    graph
        .nodes
        .iter()
        .filter(move |node| node.body.kind() == kind && recording(&node.body))
}

fn running_audio(snapshot: &StateSnapshot) -> HashSet<AudioRoute> {
    snapshot
        .device_sets
        .iter()
        .flat_map(|set| {
            set.channels.iter().flat_map(move |channel| {
                channel
                    .audio_recordings
                    .iter()
                    .map(move |status| AudioRoute {
                        device_set: set.id,
                        channel: channel.id,
                        fx: status.fx.clone(),
                    })
            })
        })
        .collect()
}

fn wanted_audio(graph: &PatchGraph, patch: &ResolvedPatch) -> HashMap<AudioRoute, bool> {
    let mut wanted = HashMap::new();
    for node in switched_on(graph, NodeKind::AudioRecorder) {
        let skip = skips_silence(&node.body);
        for feed in patch.feeds(&node.id, AUDIO_PORT) {
            for addr in feed.tap.iter().flat_map(sdrmm_wire::Tap::channels) {
                let route = AudioRoute {
                    device_set: addr.device_set,
                    channel: addr.channel,
                    fx: feed.via.clone(),
                };
                wanted
                    .entry(route)
                    .and_modify(|all: &mut bool| *all &= skip)
                    .or_insert(skip);
            }
        }
    }
    wanted
}

fn running_baseband(snapshot: &StateSnapshot) -> HashSet<(u32, u32)> {
    snapshot
        .device_sets
        .iter()
        .flat_map(|set| {
            set.channels
                .iter()
                .filter(|channel| channel.baseband_recording.is_some())
                .map(move |channel| (set.id, channel.id))
        })
        .collect()
}

fn wanted_baseband(graph: &PatchGraph, patch: &ResolvedPatch) -> HashSet<(u32, u32)> {
    switched_on(graph, NodeKind::BasebandRecorder)
        .flat_map(|node| patch.feeds(&node.id, BASEBAND_PORT))
        .flat_map(|feed| feed.tap.iter().flat_map(sdrmm_wire::Tap::channels))
        .map(|addr| (addr.device_set, addr.channel))
        .collect()
}

fn running_iq(snapshot: &StateSnapshot) -> HashMap<u32, u32> {
    snapshot
        .device_sets
        .iter()
        .filter_map(|set| set.recording.as_ref().map(|status| (set.id, status.stream)))
        .collect()
}

fn wanted_iq(graph: &PatchGraph, patch: &ResolvedPatch) -> HashMap<u32, u32> {
    let mut wanted = HashMap::new();
    for node in switched_on(graph, NodeKind::Recorder) {
        for (device_set, stream) in patch
            .feeds(&node.id, IQ_PORT)
            .iter()
            .filter_map(|feed| feed.tap.as_ref()?.lane())
        {
            wanted.entry(device_set).or_insert(stream);
        }
    }
    wanted
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        AudioRecorderNode, ChannelNode, DeviceNode, DeviceRef, PatchEdge, PatchNode, PortRef,
        Position, RecorderNode,
    };

    use super::*;

    fn node(id: &str, body: NodeBody) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn wire(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
        PatchEdge {
            from: PortRef {
                node: from.0.to_owned(),
                port: from.1.to_owned(),
            },
            to: PortRef {
                node: to.0.to_owned(),
                port: to.1.to_owned(),
            },
        }
    }

    fn audio(from: &str, to: &str) -> PatchEdge {
        wire((from, "audio"), (to, "audio"))
    }

    fn channel(id: &str) -> PatchNode {
        node(
            id,
            NodeBody::Channel(ChannelNode {
                channel_type: "nfm".to_owned(),
                tuning_locked: false,
            }),
        )
    }

    fn on(recording: bool) -> RecorderNode {
        RecorderNode { recording }
    }

    fn audio_on(recording: bool) -> AudioRecorderNode {
        AudioRecorderNode {
            recording,
            skip_silence: false,
        }
    }

    fn resolved(graph: Option<PatchGraph>, engine: &Engine) -> Resolved {
        graph.map_or_else(Resolved::default, |graph| Resolved {
            patch: crate::workspace::resolve(&graph, &engine.snapshot()),
            graph: Arc::new(graph),
            ..Resolved::default()
        })
    }

    fn wanted(graph: &PatchGraph, engine: &Engine) -> HashMap<AudioRoute, bool> {
        let resolved = resolved(Some(graph.clone()), engine);
        wanted_audio(&resolved.graph, &resolved.patch)
    }

    fn patch(recording: bool) -> PatchGraph {
        PatchGraph {
            nodes: vec![
                node(
                    "dev",
                    NodeBody::Device(DeviceNode {
                        device: Some(DeviceRef {
                            backend: "virtual".to_owned(),
                            serial: None,
                            key: None,
                        }),
                        ..DeviceNode::default()
                    }),
                ),
                channel("ch"),
                node("iq", NodeBody::Recorder(on(recording))),
                node("base", NodeBody::BasebandRecorder(on(recording))),
                node("audio", NodeBody::AudioRecorder(audio_on(recording))),
            ],
            edges: vec![
                wire(("dev", "iq"), ("ch", "iq")),
                wire(("dev", "iq"), ("iq", "iq")),
                wire(("ch", "baseband"), ("base", "baseband")),
                audio("ch", "audio"),
            ],
        }
    }

    struct Running {
        iq: bool,
        baseband: bool,
        audio: usize,
    }

    fn running(engine: &Engine, ch: u32) -> Running {
        let snapshot = engine.snapshot();
        let set = &snapshot.device_sets[0];
        let channel = set
            .channels
            .iter()
            .find(|channel| channel.id == ch)
            .expect("channel");
        Running {
            iq: set.recording.is_some(),
            baseband: channel.baseband_recording.is_some(),
            audio: channel.audio_recordings.len(),
        }
    }

    fn radio(dir: &tempfile::TempDir) -> (Arc<Engine>, u32, u32) {
        let mut registry = sdrmm_device::DeviceRegistry::new();
        registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
        let engine = Engine::with_registry(registry, Some(dir.path().to_path_buf()));
        let ds = engine.create_device_set("virtual:band").expect("radio");
        let settings = sdrmm_wire::ChannelSettings::default_for("nfm").expect("nfm");
        let ch = engine
            .add_channel_for(ds, 0, settings, Some("ch"))
            .expect("channel");
        (engine, ds, ch)
    }

    #[test]
    fn silence_is_skipped_only_when_every_recorder_on_a_route_asks() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let (engine, ds, ch) = radio(&dir);
        let route = AudioRoute::channel(ds, ch);
        let mut graph = patch(true);
        graph.nodes.push(node(
            "quiet",
            NodeBody::AudioRecorder(AudioRecorderNode {
                recording: true,
                skip_silence: true,
            }),
        ));
        graph.edges.push(audio("ch", "quiet"));
        assert_eq!(wanted(&graph, &engine).get(&route), Some(&false));
        graph.nodes.retain(|found| found.id != "audio");
        assert_eq!(wanted(&graph, &engine).get(&route), Some(&true));
        engine.remove_device_set(ds).expect("close");
    }

    #[test]
    fn the_switches_on_the_patch_start_and_stop_every_kind_of_recording() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let (engine, ds, ch) = radio(&dir);
        let store = Arc::new(Store::open(None).expect("store"));
        let hooks = Hooks {
            store,
            gate: Arc::new(Mutex::new(())),
            gps: Arc::new(GpsHub::default()),
            problems: Arc::default(),
        };
        let mut recorders = Recorders::default();

        recorders.reconcile(&engine, &resolved(Some(patch(true)), &engine), &hooks);
        let started = running(&engine, ch);
        assert!(started.iq && started.baseband && started.audio == 1);
        recorders.reconcile(&engine, &resolved(Some(patch(true)), &engine), &hooks);
        assert_eq!(
            running(&engine, ch).audio,
            1,
            "a second tick doubled a file"
        );

        recorders.reconcile(&engine, &resolved(Some(patch(false)), &engine), &hooks);
        let stopped = running(&engine, ch);
        assert!(!stopped.iq && !stopped.baseband && stopped.audio == 0);

        recorders.reconcile(&engine, &resolved(Some(patch(true)), &engine), &hooks);
        recorders.reconcile(&engine, &resolved(None, &engine), &hooks);
        let closed = running(&engine, ch);
        assert!(!closed.iq && !closed.baseband && closed.audio == 0);
        engine.remove_device_set(ds).expect("close");
    }
}
