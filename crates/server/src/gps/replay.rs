use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use sdrmm_recorder::RecordedTrack;
use sdrmm_wire::{NodeBody, PlaybackStatus, PositionFix};
use tokio::sync::watch;

use super::{GpsHub, RouteState, limit_error};
use crate::{AppState, resolved::Resolved};

const REPLAY_EVERY: Duration = Duration::from_millis(200);
const NO_POSITIONS: &str = "no positions recorded";
const NONE_HERE: &str = "no position recorded here";

/// A recording on a Device node whose position output is wired, played back in step with its IQ.
pub(super) struct Replay {
    stem: String,
    track: Result<RecordedTrack, String>,
    shown: Option<Spot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spot {
    Broken,
    Empty,
    Stopped,
    At(Option<usize>),
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Replay {
    fn load(state: &AppState, stem: &str) -> Self {
        let track = match state.engine.recordings_dir() {
            Some(dir) => RecordedTrack::read(&dir.join(stem))
                .map_err(|error| limit_error(format!("recorded positions: {error}"))),
            None => Err("recording disabled".to_owned()),
        };
        Self {
            stem: stem.to_owned(),
            track,
            shown: None,
        }
    }

    fn spot(&self, playback: Option<PlaybackStatus>) -> Spot {
        match (&self.track, playback) {
            (Err(_), _) => Spot::Broken,
            (Ok(track), _) if track.is_empty() => Spot::Empty,
            (Ok(_), None) => Spot::Stopped,
            (Ok(track), Some(status)) => Spot::At(track.index_at(status.position_samples)),
        }
    }

    /// What to publish when playback has crossed into another recorded point, if it has.
    fn advance(
        &mut self,
        playback: Option<PlaybackStatus>,
    ) -> Option<(Option<PositionFix>, Option<String>)> {
        let spot = self.spot(playback);
        if self.shown == Some(spot) {
            return None;
        }
        self.shown = Some(spot);
        Some(match (spot, &self.track) {
            (Spot::Broken, Err(error)) => (None, Some(error.clone())),
            (Spot::Empty, _) => (None, Some(NO_POSITIONS.to_owned())),
            (Spot::At(Some(index)), Ok(track)) => {
                let now = crate::store::rfc3339(jiff::Timestamp::now());
                match track.fix(index, &now) {
                    Some(fix) => (Some(fix), unreadable(track)),
                    None => (None, Some(NONE_HERE.to_owned())),
                }
            }
            (Spot::At(None), _) => (None, Some(NONE_HERE.to_owned())),
            _ => (None, None),
        })
    }
}

fn unreadable(track: &RecordedTrack) -> Option<String> {
    match track.unreadable() {
        0 => None,
        count => Some(format!("{count} recorded positions unreadable")),
    }
}

/// Recording Device nodes whose position output feeds something, keyed by node, with the stem
/// each plays.
fn wanted(resolved: &Resolved) -> HashMap<String, String> {
    resolved
        .graph
        .nodes
        .iter()
        .filter(|node| !resolved.positions.targets(&node.id).is_empty())
        .filter_map(|node| match &node.body {
            NodeBody::Device(device) => device
                .device
                .as_ref()
                .filter(|pinned| pinned.is_recording())
                .and_then(|pinned| pinned.key.clone())
                .map(|stem| (node.id.clone(), stem)),
            _ => None,
        })
        .collect()
}

impl GpsHub {
    /// Loads the track of every newly wired recording and returns the nodes that replay one.
    pub(super) fn reconcile_replays(
        &self,
        state: &AppState,
        resolved: &Resolved,
    ) -> HashSet<String> {
        let wanted = wanted(resolved);
        let mut replays = locked(&self.replays);
        replays.retain(|node, replay| wanted.get(node) == Some(&replay.stem));
        for (node, stem) in &wanted {
            replays
                .entry(node.clone())
                .or_insert_with(|| Replay::load(state, stem));
        }
        replays.keys().cloned().collect()
    }

    pub(crate) fn spawn_replay(self: &Arc<Self>, state: &AppState) {
        let hub = Arc::downgrade(self);
        let route = RouteState::from(state);
        let resolved = state.resolved.subscribe();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::warn!("no runtime in context: recorded positions will not play back");
            return;
        };
        handle.spawn(async move {
            let mut tick = tokio::time::interval(REPLAY_EVERY);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let Some(hub) = hub.upgrade() else {
                    break;
                };
                hub.step_replays(&route, &resolved);
            }
        });
    }

    fn step_replays(&self, route: &RouteState, resolved: &watch::Receiver<Arc<Resolved>>) {
        let current = resolved.borrow().clone();
        let due: Vec<_> = locked(&self.replays)
            .iter_mut()
            .filter_map(|(node, replay)| {
                let playback = current
                    .patch
                    .device_set(node)
                    .and_then(|set| route.engine.playback(set));
                replay
                    .advance(playback)
                    .map(|(fix, error)| (node.clone(), fix, error))
            })
            .collect();
        for (node, fix, error) in due {
            self.publish_routed(route.clone(), &node, fix, error);
        }
    }
}
