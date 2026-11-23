use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use sdrmm_dsp::radar::bistatic::{Geodetic, look};
use sdrmm_wire::{
    AimParams, AimStatus, ControlAck, ControlCommand, DecodedRecord, LatLon, NodeBody,
    POSITION_PORT, PatchGraph, PositionFix, SatelliteLook, ServerEvent, SkyTrack,
    position::{WAITING_FOR_FIX, WIRE_A_POSITION},
    present::Presentation,
};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::{AppState, decoded::Decoded, resolved::Resolved};

#[cfg(test)]
mod tests;

const TICK: Duration = Duration::from_secs(1);
const METRES_PER_KM: f64 = 1_000.0;
const NO_TARGET: &str = "Waiting for a target";

#[derive(Clone, Debug, PartialEq)]
struct Target {
    entity: Option<String>,
    at: LatLon,
    altitude_m: Option<f64>,
    seen: Instant,
}

#[derive(Clone, Debug, PartialEq)]
struct Wiring {
    node: String,
    settings: AimParams,
    position: Option<String>,
}

#[derive(Default)]
pub(crate) struct AimHub {
    targets: Mutex<HashMap<String, Target>>,
    statuses: Mutex<HashMap<String, AimStatus>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl AimHub {
    pub(crate) fn snapshot(&self) -> Vec<ServerEvent> {
        locked(&self.statuses)
            .values()
            .map(|status| ServerEvent::AimUpdate {
                status: Box::new(status.clone()),
            })
            .collect()
    }

    fn observe(&self, graph: &PatchGraph, record: &DecodedRecord, now: Instant) {
        let presented;
        let view = if let Some(view) = &record.view {
            view
        } else {
            presented = record.event.present();
            &presented
        };
        let mut targets = locked(&self.targets);
        for sink in &record.sinks {
            if let Some(NodeBody::Aim(aim)) = graph.node(sink).map(|node| &node.body)
                && let Some(target) = target_of(&aim.settings, view, now)
            {
                targets.insert(sink.clone(), target);
            }
        }
    }

    fn steer(&self, state: &AppState) {
        let resolved = state.resolved.current();
        let wirings = wirings(&resolved);
        let fixes: HashMap<&str, PositionFix> = wirings
            .iter()
            .filter_map(|wiring| {
                let source = wiring.position.as_deref()?;
                Some((wiring.node.as_str(), state.gps.fix(source)?))
            })
            .collect();
        let planned: Vec<(AimStatus, Option<SkyTrack>)> = {
            let mut targets = locked(&self.targets);
            targets.retain(|node, target| {
                wirings.iter().any(|wiring| {
                    wiring.node == *node && wiring.settings.follows(target.entity.as_deref())
                })
            });
            let now = Instant::now();
            wirings
                .iter()
                .map(|wiring| {
                    let fix = fixes.get(wiring.node.as_str());
                    aim(wiring, fix, targets.get(&wiring.node), now)
                })
                .collect()
        };
        let statuses: HashMap<String, AimStatus> = planned
            .into_iter()
            .map(|(status, sky)| {
                let status = drive(state, &resolved, status, sky).with_readouts();
                (status.node.clone(), status)
            })
            .collect();
        for status in statuses.values() {
            state.engine.emit_event(ServerEvent::AimUpdate {
                status: Box::new(status.clone()),
            });
        }
        *locked(&self.statuses) = statuses;
    }
}

fn drive(
    state: &AppState,
    resolved: &Resolved,
    mut status: AimStatus,
    sky: Option<SkyTrack>,
) -> AimStatus {
    let command = sky.map_or(ControlCommand::Release, ControlCommand::Point);
    let acks = state
        .control
        .deliver(&state.engine, &resolved.patch, &status.node, &command);
    if matches!(command, ControlCommand::Point(_)) {
        take_acks(&mut status, acks);
    }
    status
}

fn take_acks(status: &mut AimStatus, acks: Vec<ControlAck>) {
    for ack in acks {
        match ack.error {
            Some(error) => status.error = Some(format!("{}: {error}", ack.target)),
            None if ack.outranked => {}
            None => status.driving.push(ack.target),
        }
    }
}

fn target_of(settings: &AimParams, view: &Presentation, now: Instant) -> Option<Target> {
    let place = view.place.as_ref()?;
    settings.follows(view.entity.as_deref()).then(|| Target {
        entity: view.entity.clone(),
        at: place.at,
        altitude_m: place.altitude_m.map(f64::from),
        seen: now,
    })
}

fn aim(
    wiring: &Wiring,
    station: Option<&PositionFix>,
    target: Option<&Target>,
    now: Instant,
) -> (AimStatus, Option<SkyTrack>) {
    let mut status = AimStatus {
        node: wiring.node.clone(),
        entity: target.and_then(|target| target.entity.clone()),
        altitude_m: target.and_then(|target| target.altitude_m),
        age_s: target.map(|target| now.saturating_duration_since(target.seen).as_secs_f64()),
        ..AimStatus::default()
    };
    let waiting = match (&wiring.position, station, target) {
        (None, _, _) => WIRE_A_POSITION,
        (Some(_), None, _) => WAITING_FOR_FIX,
        (Some(_), Some(_), None) => NO_TARGET,
        (Some(_), Some(station), Some(target)) => {
            let look = look_at(station, target);
            status.look = Some(look);
            let sky = SkyTrack {
                name: target.entity.clone(),
                look,
                visible: true,
                next_pass: None,
                pass_track: Vec::new(),
            };
            return (status, Some(sky));
        }
    };
    status.waiting = Some(waiting.to_owned());
    (status, None)
}

/// A target that reports no altitude is taken to stand as high as the antenna.
fn look_at(station: &PositionFix, target: &Target) -> SatelliteLook {
    let from = Geodetic {
        lat_deg: station.latitude,
        lon_deg: station.longitude,
        alt_m: station.altitude_m.unwrap_or(0.0),
    };
    let to = Geodetic {
        lat_deg: target.at.lat,
        lon_deg: target.at.lon,
        alt_m: target.altitude_m.unwrap_or(from.alt_m),
    };
    let seen = look(from, to);
    SatelliteLook {
        azimuth_deg: seen.azimuth_deg,
        elevation_deg: seen.elevation_deg,
        range_km: seen.range_m / METRES_PER_KM,
        range_rate_km_s: 0.0,
    }
}

fn wirings(resolved: &Resolved) -> Vec<Wiring> {
    resolved
        .graph
        .nodes
        .iter()
        .filter_map(|node| match &node.body {
            NodeBody::Aim(aim) => Some(Wiring {
                node: node.id.clone(),
                settings: aim.settings.clone(),
                position: resolved
                    .patch
                    .first_feed(&node.id, POSITION_PORT)
                    .map(|feed| feed.source.clone()),
            }),
            _ => None,
        })
        .collect()
}

pub(crate) fn start(state: &AppState) {
    let records = state.decoded.subscribe();
    let fed = state.clone();
    let _detached = crate::spawn_task("sdrmm-aims", move || follow(fed, records));
}

async fn follow(state: AppState, mut records: broadcast::Receiver<Decoded>) {
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            received = records.recv() => match received {
                Ok(Decoded::Record(routed)) if !routed.record.sinks.is_empty() => {
                    let resolved = state.resolved.current();
                    state.aims.observe(&resolved.graph, &routed.record, Instant::now());
                }
                Ok(Decoded::Record(_)) => {}
                Ok(Decoded::Lost(count)) | Err(RecvError::Lagged(count)) => {
                    tracing::warn!(count, "aims missed decoded positions");
                }
                Err(RecvError::Closed) => break,
            },
            _ = tick.tick() => state.aims.steer(&state),
        }
    }
}
