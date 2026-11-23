use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use sdrmm_engine::SpectrumSnapshot;
use sdrmm_wire::{
    ControlCommand, IQ_PORT, NodeBody, RotatorMode, RotatorStatus, SatelliteLook, ServerEvent,
    SkyCell, SkyPoint, SkyScanParams, SkyScanStatus, SkyTrack, SurveyAction,
};
use tokio::{
    sync::broadcast::{self, error::RecvError},
    task::JoinHandle,
    time::Instant,
};

use crate::{
    AppState, control,
    resolved::{self, Resolved},
    survey::{Radio, measure::measure_dbfs},
};

#[cfg(test)]
mod tests;

const POLL: Duration = Duration::from_secs(1);
const SETTLE: Duration = Duration::from_secs(180);
const SAME_BEARING_DEG: f64 = 1.0;
const TURN: f64 = 360.0;
const NOT_THERE: &str = "Rotator did not get there";
const NOT_HEARD: &str = "No spectrum in the band";
const RADIO_GONE: &str = "Radio stopped";
const REWIRED: &str = "Stopped, settings or wires changed";

pub(crate) enum SkyScanRefusal {
    Missing(String),
    Invalid(&'static str),
    NoRadio,
    NoRotator,
    NotTracking,
}

#[derive(Clone, Debug, PartialEq)]
struct Rotator {
    node: String,
    tolerance_deg: f64,
    tracks: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Wiring {
    settings: SkyScanParams,
    radio: Option<Radio>,
    rotator: Option<Rotator>,
}

struct Plan {
    points: Vec<SkyPoint>,
    settings: SkyScanParams,
    radio: Radio,
    rotator: Rotator,
}

#[derive(Default)]
struct Session {
    wiring: Wiring,
    status: SkyScanStatus,
    task: Option<JoinHandle<()>>,
    generation: u64,
}

impl Session {
    fn view(&self) -> SkyScanStatus {
        self.status.clone().with_readouts()
    }

    fn halt(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.generation = self.generation.wrapping_add(1);
        self.status.running = false;
    }

    fn plan(&self) -> Result<Plan, SkyScanRefusal> {
        let wiring = &self.wiring;
        if let Some(problem) = wiring.settings.problem() {
            return Err(SkyScanRefusal::Invalid(problem));
        }
        let radio = wiring.radio.ok_or(SkyScanRefusal::NoRadio)?;
        let rotator = wiring.rotator.clone().ok_or(SkyScanRefusal::NoRotator)?;
        if !rotator.tracks {
            return Err(SkyScanRefusal::NotTracking);
        }
        Ok(Plan {
            points: wiring.settings.points(),
            settings: wiring.settings,
            radio,
            rotator,
        })
    }
}

#[derive(Default)]
pub(crate) struct SkyScanHub {
    sessions: Mutex<HashMap<String, Session>>,
}

impl SkyScanHub {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Session>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn snapshot(&self) -> Vec<ServerEvent> {
        self.lock()
            .values()
            .map(|session| ServerEvent::SkyScanUpdate {
                status: Box::new(session.view()),
            })
            .collect()
    }

    pub(crate) fn status(&self, node: &str) -> Option<SkyScanStatus> {
        self.lock().get(node).map(Session::view)
    }

    pub(crate) fn act(
        &self,
        state: &AppState,
        node: &str,
        action: SurveyAction,
    ) -> Result<SkyScanStatus, SkyScanRefusal> {
        let status = {
            let mut sessions = self.lock();
            let session = sessions
                .get_mut(node)
                .ok_or_else(|| SkyScanRefusal::Missing(node.to_owned()))?;
            match action {
                SurveyAction::Start => {
                    let plan = session.plan()?;
                    session.halt();
                    session.status.cells.clear();
                    session.status.frequency_hz = None;
                    session.status.error = None;
                    session.status.waiting = None;
                    session.status.shape(&plan.settings);
                    session.status.running = true;
                    session.task = spawn(state, node, plan, session.generation);
                }
                SurveyAction::Stop => session.halt(),
                SurveyAction::Clear => {
                    session.status.cells.clear();
                    session.status.error = None;
                    session.status.waiting = None;
                }
            }
            session.view()
        };
        if action == SurveyAction::Stop {
            release(state, node);
        }
        emit(state, status.clone());
        Ok(status)
    }

    fn apply(&self, wanted: HashMap<String, Wiring>) -> Vec<SkyScanStatus> {
        let mut sessions = self.lock();
        sessions.retain(|node, session| {
            let keep = wanted.contains_key(node);
            if !keep {
                session.halt();
            }
            keep
        });
        let mut changed = Vec::new();
        for (node, wiring) in wanted {
            let session = sessions.entry(node.clone()).or_default();
            if session.wiring == wiring && session.status.node == node {
                continue;
            }
            if session.status.running {
                session.halt();
                session.status.waiting = Some(REWIRED.to_owned());
            }
            session.status.node = node;
            if session.wiring.settings != wiring.settings {
                session.status.cells.clear();
            }
            session.status.shape(&wiring.settings);
            session.wiring = wiring;
            changed.push(session.view());
        }
        changed
    }

    fn record(
        &self,
        node: &str,
        generation: u64,
        cell: SkyCell,
        frequency_hz: f64,
    ) -> Option<SkyScanStatus> {
        let mut sessions = self.lock();
        let session = sessions
            .get_mut(node)
            .filter(|session| session.generation == generation)?;
        session.status.cells.push(cell);
        session.status.frequency_hz = Some(frequency_hz);
        Some(session.view())
    }

    fn finish(&self, node: &str, generation: u64, error: Option<String>) -> Option<SkyScanStatus> {
        let mut sessions = self.lock();
        let session = sessions
            .get_mut(node)
            .filter(|session| session.generation == generation)?;
        session.task = None;
        session.status.running = false;
        session.status.error = error;
        Some(session.view())
    }
}

fn spawn(state: &AppState, node: &str, plan: Plan, generation: u64) -> Option<JoinHandle<()>> {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::warn!(node, "no runtime: the sky scan cannot run");
        return None;
    };
    Some(runtime.spawn(run(state.clone(), node.to_owned(), plan, generation)))
}

async fn run(state: AppState, node: String, plan: Plan, generation: u64) {
    let error = sweep(&state, &node, &plan, generation).await.err();
    if let Some(status) = state.sky_scans.finish(&node, generation, error) {
        release(&state, &node);
        emit(&state, status);
    }
}

async fn sweep(state: &AppState, node: &str, plan: &Plan, generation: u64) -> Result<(), String> {
    let mut spectrum = subscribe(state, plan.radio).await?;
    let close_deg = (plan.settings.step_deg / 2.0).max(plan.rotator.tolerance_deg);
    for &point in &plan.points {
        reach(state, node, &plan.rotator.node, point, close_deg).await?;
        let (level_dbfs, frequency_hz) = listen(&mut spectrum, &plan.settings).await?;
        let cell = SkyCell {
            azimuth_deg: point.azimuth_deg,
            elevation_deg: point.elevation_deg,
            level_dbfs,
        };
        let Some(status) = state.sky_scans.record(node, generation, cell, frequency_hz) else {
            return Ok(());
        };
        emit(state, status);
    }
    Ok(())
}

async fn subscribe(
    state: &AppState,
    radio: Radio,
) -> Result<broadcast::Receiver<SpectrumSnapshot>, String> {
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || engine.subscribe_spectrum(radio.device_set, radio.stream))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

async fn reach(
    state: &AppState,
    node: &str,
    rotator: &str,
    point: SkyPoint,
    close_deg: f64,
) -> Result<(), String> {
    let mut deadline = Instant::now() + SETTLE;
    let command = ControlCommand::Point(sky(point));
    loop {
        let resolved = state.resolved.current();
        let acks = state
            .control
            .deliver(&state.engine, &resolved.patch, node, &command);
        if acks.iter().any(|ack| ack.outranked) {
            deadline = Instant::now() + SETTLE;
            tokio::time::sleep(POLL).await;
            continue;
        }
        if let Some(error) = acks
            .into_iter()
            .find_map(|ack| Some(format!("{}: {}", ack.target, ack.error?)))
        {
            return Err(error);
        }
        let status = state.rotators.status(rotator);
        if status
            .as_ref()
            .is_some_and(|status| arrived(status, point, close_deg))
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(status.and_then(|status| status.error).map_or_else(
                || NOT_THERE.to_owned(),
                |error| format!("{NOT_THERE}: {error}"),
            ));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The loudest level in the band while it dwells, so a bursty transmitter still shows.
async fn listen(
    spectrum: &mut broadcast::Receiver<SpectrumSnapshot>,
    settings: &SkyScanParams,
) -> Result<(f32, f64), String> {
    *spectrum = spectrum.resubscribe();
    let deadline = Instant::now() + Duration::from_millis(u64::from(settings.dwell_ms));
    let mut loudest: Option<(f32, f64)> = None;
    loop {
        match tokio::time::timeout_at(deadline, spectrum.recv()).await {
            Err(_) => break,
            Ok(Ok(snapshot)) => {
                let heard = level(&snapshot, settings);
                loudest = match (loudest, heard) {
                    (Some(old), Some(new)) if new.0 > old.0 => Some(new),
                    (None, new) => new,
                    (old, _) => old,
                };
            }
            Ok(Err(RecvError::Lagged(_))) => {}
            Ok(Err(RecvError::Closed)) => return Err(RADIO_GONE.to_owned()),
        }
    }
    loudest.ok_or_else(|| NOT_HEARD.to_owned())
}

fn level(snapshot: &SpectrumSnapshot, settings: &SkyScanParams) -> Option<(f32, f64)> {
    let target_hz = snapshot.center_hz + settings.offset_hz as f64;
    let level = measure_dbfs(
        snapshot.center_hz,
        f64::from(snapshot.span_hz),
        &snapshot.db,
        target_hz,
        settings.bandwidth_hz as f64,
    )?;
    Some((level, target_hz))
}

fn sky(point: SkyPoint) -> SkyTrack {
    SkyTrack {
        name: None,
        look: SatelliteLook {
            azimuth_deg: point.azimuth_deg,
            elevation_deg: point.elevation_deg,
            range_km: 0.0,
            range_rate_km_s: 0.0,
        },
        visible: true,
        next_pass: None,
        pass_track: Vec::new(),
    }
}

/// The rotator took this point as its target and stands close enough to it.
fn arrived(status: &RotatorStatus, point: SkyPoint, close_deg: f64) -> bool {
    let (Some(position), Some(target)) = (status.position, status.target) else {
        return false;
    };
    azimuth_apart(target.azimuth_deg, point.azimuth_deg) <= SAME_BEARING_DEG
        && apart(position, target) <= close_deg
}

fn azimuth_apart(a: f64, b: f64) -> f64 {
    let turned = (a - b).rem_euclid(TURN);
    turned.min(TURN - turned)
}

fn apart(a: SkyPoint, b: SkyPoint) -> f64 {
    azimuth_apart(a.azimuth_deg, b.azimuth_deg).max((a.elevation_deg - b.elevation_deg).abs())
}

fn release(state: &AppState, node: &str) {
    let resolved = state.resolved.current();
    state.control.deliver(
        &state.engine,
        &resolved.patch,
        node,
        &ControlCommand::Release,
    );
}

fn emit(state: &AppState, status: SkyScanStatus) {
    state.engine.emit_event(ServerEvent::SkyScanUpdate {
        status: Box::new(status),
    });
}

fn rotator_of(resolved: &Resolved, node: &str) -> Option<Rotator> {
    control::targets(&resolved.patch, node)
        .into_iter()
        .find_map(|target| match &resolved.graph.node(&target.node)?.body {
            NodeBody::Rotator(rotator) => Some(Rotator {
                tolerance_deg: rotator.tolerance_deg,
                tracks: rotator.mode == RotatorMode::Track,
                node: target.node,
            }),
            _ => None,
        })
}

fn wirings(resolved: &Resolved) -> HashMap<String, Wiring> {
    resolved
        .graph
        .nodes
        .iter()
        .filter_map(|node| {
            let NodeBody::SkyScan(scan) = &node.body else {
                return None;
            };
            let radio = resolved
                .patch
                .first_feed(&node.id, IQ_PORT)
                .and_then(|feed| feed.tap.as_ref()?.lane())
                .map(|(device_set, stream)| Radio { device_set, stream });
            let wiring = Wiring {
                settings: scan.settings,
                radio,
                rotator: rotator_of(resolved, &node.id),
            };
            Some((node.id.clone(), wiring))
        })
        .collect()
}

pub(crate) fn follow(state: &AppState) {
    resolved::follow(state, "sky scans", |state| {
        let changed = state.sky_scans.apply(wirings(&state.resolved.current()));
        for status in changed {
            emit(state, status);
        }
    });
}
