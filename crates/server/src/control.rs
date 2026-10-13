use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use sdrmm_engine::Engine;
use sdrmm_wire::{
    CONTROL_PORT, ChannelAddr, ControlAck, ControlCommand, ControlVerb, DopplerSteer, HuntAction,
    HuntSettings, IQ_PORT, Payload, ResolvedPatch, ScanAction, ScanSettings, SweepJob, SweepStatus,
    WatchedLane,
};
use tokio::sync::watch;

const NOT_RUNNING: &str = "Not running";
const NO_LANE: &str = "Wire the lane it watches";

pub(crate) struct Sweep<'a> {
    pub(crate) job: &'a SweepJob,
    pub(crate) from: &'a str,
    pub(crate) lane: WatchedLane,
}

pub(crate) trait Rig {
    fn tune(&self, at: ChannelAddr, freq_hz: f64) -> Result<(), String>;
    fn steer(&self, at: ChannelAddr, doppler: DopplerSteer) -> Result<(), String>;
    fn sweep(&self, at: ChannelAddr, sweep: &Sweep<'_>) -> Result<Option<SweepStatus>, String>;
    fn halt(&self, at: ChannelAddr, from: &str) -> Result<(), String>;
}

#[derive(Clone, Default)]
struct Held {
    steered: BTreeSet<String>,
    swept: BTreeSet<String>,
}

impl Held {
    fn is_empty(&self) -> bool {
        self.steered.is_empty() && self.swept.is_empty()
    }

    fn split_off(&mut self, keep: impl Fn(&str) -> bool) -> Self {
        let (steered, kept) = std::mem::take(&mut self.steered)
            .into_iter()
            .partition(|target| !keep(target));
        self.steered = kept;
        let (swept, kept) = std::mem::take(&mut self.swept)
            .into_iter()
            .partition(|target| !keep(target));
        self.swept = kept;
        Self { steered, swept }
    }
}

#[derive(Default)]
pub(crate) struct ControlBus {
    latest: Mutex<HashMap<String, watch::Sender<Option<ControlCommand>>>>,
    driven: Mutex<HashMap<String, Held>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ControlBus {
    pub(crate) fn follow(&self, target: &str) -> watch::Receiver<Option<ControlCommand>> {
        locked(&self.latest)
            .entry(target.to_owned())
            .or_insert_with(|| watch::channel(None).0)
            .subscribe()
    }

    pub(crate) fn deliver(
        &self,
        rig: &dyn Rig,
        patch: &ResolvedPatch,
        from: &str,
        command: &ControlCommand,
    ) -> Vec<ControlAck> {
        let targets = targets(patch, from);
        self.release_unwired(rig, patch, from, &targets);
        let acks: Vec<ControlAck> = targets
            .iter()
            .filter(|(_, verbs)| command.verb().is_none_or(|verb| verbs.contains(&verb)))
            .map(|(target, _)| self.apply(rig, patch, from, target, command))
            .collect();
        self.remember(from, command, &acks);
        acks
    }

    fn release_all_but(&self, rig: &dyn Rig, patch: &ResolvedPatch, live: &dyn Fn(&str) -> bool) {
        let gone: Vec<(String, Held)> = {
            let mut driven = locked(&self.driven);
            let gone = driven
                .iter()
                .filter(|(from, _)| !live(from))
                .map(|(from, held)| (from.clone(), held.clone()))
                .collect();
            driven.retain(|from, _| live(from));
            gone
        };
        for (from, held) in gone {
            self.let_go(rig, patch, &from, held);
        }
    }

    pub(crate) fn settle(&self, rig: &dyn Rig, patch: &ResolvedPatch) {
        self.release_all_but(rig, patch, &|from: &str| patch.node(from).is_some());
        let controllers: Vec<String> = locked(&self.driven).keys().cloned().collect();
        for from in controllers {
            self.release_unwired(rig, patch, &from, &targets(patch, &from));
        }
    }

    fn release_unwired(
        &self,
        rig: &dyn Rig,
        patch: &ResolvedPatch,
        from: &str,
        targets: &[(String, Vec<ControlVerb>)],
    ) {
        let unwired = {
            let mut driven = locked(&self.driven);
            let Some(held) = driven.get_mut(from) else {
                return;
            };
            let unwired = held.split_off(|held| targets.iter().any(|(target, _)| target == held));
            if held.is_empty() {
                driven.remove(from);
            }
            unwired
        };
        self.let_go(rig, patch, from, unwired);
    }

    fn let_go(&self, rig: &dyn Rig, patch: &ResolvedPatch, from: &str, held: Held) {
        for target in held.steered {
            self.release(rig, patch, &target);
        }
        for target in held.swept {
            if let Some(at) = patch.channel(&target)
                && let Err(error) = rig.halt(at, from)
            {
                tracing::debug!(%error, target, from, "could not stop a sweep");
            }
        }
    }

    fn remember(&self, from: &str, command: &ControlCommand, acks: &[ControlAck]) {
        let mut driven = locked(&self.driven);
        let held = driven.entry(from.to_owned()).or_default();
        let taken = acks
            .iter()
            .filter(|ack| ack.error.is_none())
            .map(|ack| ack.target.clone());
        match command {
            ControlCommand::Release => held.steered.clear(),
            ControlCommand::Doppler(_) | ControlCommand::Point(_) => held.steered.extend(taken),
            ControlCommand::Sweep(job) => match runs(job) {
                Some(true) => held.swept.extend(taken),
                Some(false) => {
                    for target in taken {
                        held.swept.remove(&target);
                    }
                }
                None => {}
            },
            ControlCommand::Tune { .. } => {}
        }
        if held.is_empty() {
            driven.remove(from);
        }
    }

    fn apply(
        &self,
        rig: &dyn Rig,
        patch: &ResolvedPatch,
        from: &str,
        target: &str,
        command: &ControlCommand,
    ) -> ControlAck {
        let applied = match command {
            ControlCommand::Point(_) => {
                self.publish(target, Some(command.clone()));
                Ok(None)
            }
            ControlCommand::Release => {
                self.release(rig, patch, target);
                Ok(None)
            }
            ControlCommand::Tune { freq_hz } => {
                on_channel(patch, target, |at| rig.tune(at, *freq_hz).map(|()| None))
            }
            ControlCommand::Doppler(doppler) => {
                on_channel(patch, target, |at| rig.steer(at, *doppler).map(|()| None))
            }
            ControlCommand::Sweep(job) => on_channel(patch, target, |at| {
                let lane = watched_lane(patch, from).ok_or_else(|| NO_LANE.to_owned())?;
                rig.sweep(at, &Sweep { job, from, lane })
            }),
        };
        let (sweep, error) = match applied {
            Ok(sweep) => (sweep.map(Box::new), None),
            Err(error) => (None, Some(error)),
        };
        ControlAck {
            target: target.to_owned(),
            verb: command.verb(),
            error,
            sweep,
        }
    }

    fn release(&self, rig: &dyn Rig, patch: &ResolvedPatch, target: &str) {
        self.publish(target, None);
        if let Some(at) = patch.channel(target)
            && let Err(error) = rig.steer(at, DopplerSteer::default())
        {
            tracing::debug!(%error, target, "could not release a Doppler correction");
        }
    }

    fn publish(&self, target: &str, command: Option<ControlCommand>) {
        locked(&self.latest)
            .entry(target.to_owned())
            .or_insert_with(|| watch::channel(None).0)
            .send_replace(command);
    }
}

const fn runs(job: &SweepJob) -> Option<bool> {
    match job {
        SweepJob::Scan {
            action: ScanAction::Start,
            ..
        }
        | SweepJob::Hunt {
            action: HuntAction::Start | HuntAction::Sweep,
            ..
        } => Some(true),
        SweepJob::Scan {
            action: ScanAction::Stop,
            ..
        }
        | SweepJob::Hunt {
            action: HuntAction::Stop,
            ..
        } => Some(false),
        SweepJob::Scan {
            action: ScanAction::Skip,
            ..
        }
        | SweepJob::Hunt {
            action: HuntAction::Mark,
            ..
        } => None,
    }
}

pub(crate) fn takes(patch: &ResolvedPatch, from: &str, verb: ControlVerb) -> bool {
    targets(patch, from)
        .iter()
        .any(|(_, verbs)| verbs.contains(&verb))
}

pub(crate) fn targets(patch: &ResolvedPatch, from: &str) -> Vec<(String, Vec<ControlVerb>)> {
    patch
        .reaches(from, CONTROL_PORT)
        .iter()
        .map(|reach| {
            let verbs = patch
                .node(&reach.sink)
                .and_then(|node| node.input(&reach.port))
                .map(|port| match &port.spec.payload {
                    Payload::Control { verbs } => verbs.clone(),
                    _ => Vec::new(),
                })
                .unwrap_or_default();
            (reach.sink.clone(), verbs)
        })
        .collect()
}

fn watched_lane(patch: &ResolvedPatch, from: &str) -> Option<WatchedLane> {
    patch
        .first_feed(from, IQ_PORT)
        .and_then(|feed| feed.tap.as_ref()?.lane())
        .map(|(device_set, stream)| WatchedLane { device_set, stream })
}

fn on_channel(
    patch: &ResolvedPatch,
    target: &str,
    apply: impl FnOnce(ChannelAddr) -> Result<Option<SweepStatus>, String>,
) -> Result<Option<SweepStatus>, String> {
    patch
        .channel(target)
        .ok_or_else(|| NOT_RUNNING.to_owned())
        .and_then(apply)
}

impl Rig for Arc<Engine> {
    fn tune(&self, at: ChannelAddr, freq_hz: f64) -> Result<(), String> {
        self.tune_channel(at.device_set, at.channel, freq_hz)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn steer(&self, at: ChannelAddr, doppler: DopplerSteer) -> Result<(), String> {
        self.steer_channel(at.device_set, at.channel, doppler)
            .map_err(|error| error.to_string())
    }

    fn sweep(&self, at: ChannelAddr, sweep: &Sweep<'_>) -> Result<Option<SweepStatus>, String> {
        let lane = sweep.lane;
        match sweep.job {
            SweepJob::Scan { settings, action } => scan(
                self,
                at,
                scan_settings(settings, sweep.from, at, lane),
                *action,
            )
            .map(|status| Some(SweepStatus::Scan(status))),
            SweepJob::Hunt { params, action } => {
                let settings = HuntSettings {
                    channel: at.channel,
                    lane: Some(lane),
                    node: Some(sweep.from.to_owned()),
                    sweep: *params,
                    ..HuntSettings::for_channel(at.channel)
                };
                hunt(self, at, settings, *action).map(|status| status.map(SweepStatus::Hunt))
            }
        }
        .map_err(|error| error.to_string())
    }

    fn halt(&self, at: ChannelAddr, from: &str) -> Result<(), String> {
        let snapshot = self.snapshot();
        let Some(set) = snapshot
            .device_sets
            .iter()
            .find(|set| set.id == at.device_set)
        else {
            return Ok(());
        };
        let mine = |node: Option<&String>| node.is_some_and(|node| node == from);
        if set.scanners.iter().any(|scanner| {
            scanner.settings.channel == at.channel && mine(scanner.settings.node.as_ref())
        }) {
            self.stop_scan(at.device_set, at.channel)
                .map_err(|error| error.to_string())?;
        }
        if set
            .hunts
            .iter()
            .any(|hunt| hunt.settings.channel == at.channel && mine(hunt.settings.node.as_ref()))
        {
            self.stop_hunt(at.device_set, at.channel)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn scan_settings(
    settings: &ScanSettings,
    from: &str,
    at: ChannelAddr,
    lane: WatchedLane,
) -> ScanSettings {
    ScanSettings {
        channel: at.channel,
        lane: Some(lane),
        node: Some(from.to_owned()),
        ..settings.clone()
    }
}

fn scan(
    engine: &Arc<Engine>,
    at: ChannelAddr,
    settings: ScanSettings,
    action: ScanAction,
) -> Result<sdrmm_wire::ScannerStatus, sdrmm_engine::EngineError> {
    match action {
        ScanAction::Start => engine.start_scan(at.device_set, settings),
        ScanAction::Stop => engine.stop_scan(at.device_set, at.channel),
        ScanAction::Skip => engine.skip_scan(at.device_set, at.channel),
    }
}

fn hunt(
    engine: &Arc<Engine>,
    at: ChannelAddr,
    settings: HuntSettings,
    action: HuntAction,
) -> Result<Option<sdrmm_wire::HuntStatus>, sdrmm_engine::EngineError> {
    match action {
        HuntAction::Start => engine.start_hunt(at.device_set, settings).map(Some),
        HuntAction::Stop => engine.stop_hunt(at.device_set, at.channel).map(Some),
        HuntAction::Sweep => engine
            .sweep_hunt(at.device_set, at.channel, Some(settings))
            .map(Some),
        HuntAction::Mark => engine.hunt_mark(at.device_set, at.channel).map(|_| None),
    }
}

#[cfg(test)]
mod tests;
