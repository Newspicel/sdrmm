use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use sdrmm_engine::Engine;
use sdrmm_orbit::{OrbitError, Satellite, Tle};
use sdrmm_wire::{
    ControlAck, ControlCommand, ControlVerb, DecodedRecord, DecoderEvent, DopplerSteer,
    EventOrigin, LatLon, NO_CHANNEL, NodeBody, POSITION_PORT, PassPoint, ResolvedPatch,
    SatelliteFix, SatelliteNode, SatellitePass, SatelliteStatus, ServerEvent, SkyTrack,
    position::{WAITING_FOR_FIX, WIRE_A_POSITION},
};

use crate::{
    AppState,
    control::{self, ControlBus},
    gps::GpsHub,
    resolved::{Resolved, ResolvedHub},
};

pub(crate) mod catalog;
mod track;

pub(crate) use catalog::Catalog;

const TICK: Duration = Duration::from_millis(250);
const PUBLISH_EVERY: Duration = Duration::from_secs(1);
const PASS_EVERY: Duration = Duration::from_secs(60);
const PASS_HORIZON_S: f64 = 2.0 * 86_400.0;
const HORIZON_DEG: f64 = 0.0;
/// Opening a radio takes seconds, so one the satellite runs opens this long before the rise.
const RUN_LEAD_S: f64 = 30.0;
const PICK_A_SATELLITE: &str = "Pick a satellite";
const SET_A_DOWNLINK: &str = "Set a downlink frequency";

/// Why a satellite is not steered: a wait shows faint, a fault red.
enum Halt {
    Waiting(&'static str),
    Fault(String),
}

impl From<String> for Halt {
    fn from(error: String) -> Self {
        Self::Fault(error)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Wiring {
    node: String,
    settings: SatelliteNode,
    position: Option<String>,
}

#[derive(Clone, Default)]
struct Plan {
    wirings: Vec<Wiring>,
}

pub(crate) struct SatelliteHub {
    plan: Arc<Mutex<Plan>>,
    statuses: Arc<Mutex<HashMap<String, SatelliteStatus>>>,
    wake: Mutex<Option<mpsc::Sender<()>>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    pub(crate) catalog: Catalog,
}

impl Default for SatelliteHub {
    fn default() -> Self {
        Self {
            plan: Arc::new(Mutex::new(Plan::default())),
            statuses: Arc::new(Mutex::new(HashMap::new())),
            wake: Mutex::new(None),
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
            catalog: Catalog::default(),
        }
    }
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl SatelliteHub {
    pub(crate) fn snapshot(&self) -> Vec<ServerEvent> {
        locked(&self.statuses)
            .values()
            .map(|status| ServerEvent::SatelliteUpdate {
                status: Box::new(status.clone()),
            })
            .collect()
    }

    pub(crate) fn reconcile(&self, state: &AppState) {
        let wirings = wirings(&state.resolved.current());
        let idle = wirings.is_empty();
        let tracked: HashSet<String> = wirings.iter().map(|wiring| wiring.node.clone()).collect();
        *locked(&self.plan) = Plan { wirings };
        locked(&self.statuses).retain(|node, _| tracked.contains(node));
        if idle && locked(&self.worker).is_none() {
            return;
        }
        self.ensure_worker(state);
        if let Some(wake) = locked(&self.wake).as_ref() {
            let _ = wake.send(());
        }
    }

    fn ensure_worker(&self, state: &AppState) {
        let mut worker = locked(&self.worker);
        if worker.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }
        let (wake_tx, wake_rx) = mpsc::channel();
        let tracker = Tracker {
            engine: Arc::downgrade(&state.engine),
            gps: state.gps.clone(),
            resolved: state.resolved.clone(),
            control: state.control.clone(),
            plan: self.plan.clone(),
            statuses: self.statuses.clone(),
            stop: self.stop.clone(),
            satellites: HashMap::new(),
            published_at: None,
            placed: HashSet::new(),
        };
        match std::thread::Builder::new()
            .name("sdrmm-satellites".to_owned())
            .spawn(move || tracker.run(&wake_rx))
        {
            Ok(handle) => {
                *worker = Some(handle);
                *locked(&self.wake) = Some(wake_tx);
            }
            Err(error) => tracing::error!(%error, "could not start the satellite tracker"),
        }
    }
}

impl Drop for SatelliteHub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        locked(&self.wake).take();
        if let Some(worker) = locked(&self.worker).take()
            && worker.join().is_err()
        {
            tracing::error!("satellite tracker panicked");
        }
    }
}

fn wirings(resolved: &Resolved) -> Vec<Wiring> {
    resolved
        .graph
        .nodes
        .iter()
        .filter_map(|node| match &node.body {
            NodeBody::Satellite(settings) => Some(Wiring {
                node: node.id.clone(),
                settings: settings.clone(),
                position: resolved
                    .patch
                    .first_feed(&node.id, POSITION_PORT)
                    .map(|feed| feed.source.clone()),
            }),
            _ => None,
        })
        .collect()
}

struct Orbit {
    tle: String,
    satellite: Result<Satellite, String>,
    pass: Option<SatellitePass>,
    sky: Vec<PassPoint>,
    passed_at: Option<Instant>,
    track: GroundTrack,
}

#[derive(Default)]
struct GroundTrack {
    points: Vec<LatLon>,
    at: Option<Instant>,
}

struct Tracker {
    engine: Weak<Engine>,
    gps: Arc<GpsHub>,
    resolved: Arc<ResolvedHub>,
    control: Arc<ControlBus>,
    plan: Arc<Mutex<Plan>>,
    statuses: Arc<Mutex<HashMap<String, SatelliteStatus>>>,
    stop: Arc<AtomicBool>,
    satellites: HashMap<String, Orbit>,
    published_at: Option<Instant>,
    placed: HashSet<String>,
}

fn fix_record(status: &SatelliteStatus) -> DecodedRecord {
    DecodedRecord {
        origin: Some(EventOrigin {
            node: status.node.clone(),
            transmission: 0,
        }),
        device_set: 0,
        channel: NO_CHANNEL,
        at: format!("{:.9}", jiff::Timestamp::now()),
        freq_hz: status.downlink_hz.unwrap_or_default(),
        event: DecoderEvent::of(&SatelliteFix::of(status)),
        sinks: Vec::new(),
        view: None,
        callsign: None,
    }
}

impl Tracker {
    fn run(mut self, wake: &mpsc::Receiver<()>) {
        let mut rebind = true;
        loop {
            if self.stop.load(Ordering::Acquire) {
                return;
            }
            let Some(engine) = self.engine.upgrade() else {
                return;
            };
            let plan = locked(&self.plan).clone();
            self.tick(&engine, &plan.wirings, rebind);
            drop(engine);
            rebind = match wake.recv_timeout(TICK) {
                Ok(()) => true,
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => return,
            };
        }
    }

    fn tick(&mut self, engine: &Arc<Engine>, wirings: &[Wiring], changed: bool) {
        let now = unix_now();
        let publish = changed
            || self
                .published_at
                .is_none_or(|at| at.elapsed() >= PUBLISH_EVERY);
        self.satellites
            .retain(|node, _| wirings.iter().any(|wiring| wiring.node == *node));
        let resolved = self.resolved.current();
        let mut statuses = HashMap::new();
        for wiring in wirings {
            let (mut status, steering) = self.follow(wiring, now);
            drive(
                &self.control,
                engine,
                &resolved.patch,
                (wiring, steering),
                &mut status,
            );
            statuses.insert(wiring.node.clone(), status);
        }
        locked(&self.statuses).clone_from(&statuses);
        self.placed.retain(|node| statuses.contains_key(node));
        for status in statuses.values() {
            if status.ground.is_none() && self.placed.remove(&status.node) {
                engine.publish_decoded(fix_record(status));
            }
        }
        if publish {
            self.published_at = Some(Instant::now());
            for status in statuses.into_values() {
                if status.ground.is_some() {
                    self.placed.insert(status.node.clone());
                    engine.publish_decoded(fix_record(&status));
                }
                engine.emit_event(ServerEvent::SatelliteUpdate {
                    status: Box::new(status),
                });
            }
        }
    }

    fn follow(&mut self, wiring: &Wiring, now: f64) -> (SatelliteStatus, Option<track::Steering>) {
        let mut status = SatelliteStatus {
            node: wiring.node.clone(),
            ..SatelliteStatus::default()
        };
        match self.steer(wiring, now, &mut status) {
            Ok(steering) => (status, Some(steering)),
            Err(Halt::Waiting(waiting)) => {
                status.waiting = Some(waiting.to_owned());
                (status, None)
            }
            Err(Halt::Fault(error)) => {
                status.error = Some(error);
                (status, None)
            }
        }
    }

    fn steer(
        &mut self,
        wiring: &Wiring,
        now: f64,
        status: &mut SatelliteStatus,
    ) -> Result<track::Steering, Halt> {
        let orbit =
            orbit_of(&mut self.satellites, wiring).ok_or(Halt::Waiting(PICK_A_SATELLITE))?;
        let satellite = orbit.satellite.as_ref().map_err(Clone::clone)?;
        status.name = satellite.tle.name.clone();
        status.catalog = Some(satellite.tle.catalog.clone());
        status.tle_age_days = Some(satellite.age_days(now));
        place(&mut orbit.track, satellite, status, now).map_err(|error| error.to_string())?;
        let source = wiring
            .position
            .as_ref()
            .ok_or(Halt::Waiting(WIRE_A_POSITION))?;
        let fix = self.gps.fix(source).ok_or(Halt::Waiting(WAITING_FOR_FIX))?;
        let observer = track::observer(&fix);
        let steering = track::steer(satellite, &observer, &wiring.settings, now)
            .map_err(|error| error.to_string())?;
        if orbit.passed_at.is_none_or(|at| at.elapsed() >= PASS_EVERY)
            || orbit
                .pass
                .is_some_and(|pass| pass.los.is_some_and(|los| (los as f64) < now))
        {
            orbit.passed_at = Some(Instant::now());
            let found = satellite
                .next_pass(&observer, now, PASS_HORIZON_S, HORIZON_DEG)
                .ok()
                .flatten();
            orbit.pass = found.map(track::pass);
            orbit.sky = found
                .map(|pass| track::sky(satellite, &observer, &pass))
                .transpose()
                .unwrap_or_else(|error| {
                    tracing::debug!(%error, "could not trace a pass across the sky");
                    None
                })
                .flatten()
                .unwrap_or_default();
        }
        status.look = Some(steering.look);
        status.visible = steering.look.elevation_deg >= HORIZON_DEG;
        status.next_pass = orbit.pass;
        status.pass_track.clone_from(&orbit.sky);
        status.uplink_hz = steering.uplink_hz;
        status.downlink_hz = steering.downlink_hz;
        status.doppler_hz = steering.doppler.map(|doppler| doppler.shift_hz);
        status.doppler_rate_hz_s = steering.doppler.map(|doppler| doppler.rate_hz_s);
        Ok(steering)
    }
}

fn drive(
    control: &ControlBus,
    engine: &Arc<Engine>,
    patch: &ResolvedPatch,
    (wiring, steering): (&Wiring, Option<track::Steering>),
    status: &mut SatelliteStatus,
) {
    let node = wiring.node.as_str();
    let Some(steering) = steering else {
        control.deliver(engine, patch, node, &ControlCommand::Release);
        return;
    };
    let sky = SkyTrack {
        name: status.name.clone(),
        look: steering.look,
        visible: status.visible,
        next_pass: status.next_pass,
        pass_track: status.pass_track.clone(),
    };
    let due = runs(status, unix_now());
    let (mut acks, on) = point_and_run(control, engine, patch, node, sky, due);
    let doppler =
        if let (Some(freq_hz), Some(doppler)) = (wiring.settings.downlink_hz, steering.doppler) {
            acks.extend(control.deliver(engine, patch, node, &ControlCommand::Tune { freq_hz }));
            doppler
        } else {
            if control::takes(patch, node, ControlVerb::Tune) {
                status.waiting = Some(SET_A_DOWNLINK.to_owned());
            }
            DopplerSteer::default()
        };
    acks.extend(control.deliver(engine, patch, node, &ControlCommand::Doppler(doppler)));
    if !on && control::takes(patch, node, ControlVerb::Run) {
        acks.retain(|ack| ack.error.as_deref() != Some(control::NOT_RUNNING));
    }
    take_acks(status, acks);
}

/// Points its rotators first: their acks say whether one follows it, and only then may its
/// radios run. Gives the acks and whether it ran them.
fn point_and_run(
    control: &ControlBus,
    rig: &dyn control::Rig,
    patch: &ResolvedPatch,
    node: &str,
    sky: SkyTrack,
    due: bool,
) -> (Vec<ControlAck>, bool) {
    let pointed = control.deliver(rig, patch, node, &ControlCommand::Point(sky));
    let on = due && holds_a_rotator(&pointed);
    let mut acks = control.deliver(rig, patch, node, &ControlCommand::Run { on });
    acks.extend(pointed);
    (acks, on)
}

/// A satellite wired to rotators runs its radios only while one of them follows it, so a
/// recorder never takes a pass the antenna points away from.
fn holds_a_rotator(pointed: &[ControlAck]) -> bool {
    pointed.is_empty() || pointed.iter().any(|ack| !ack.outranked)
}

/// Between passes the radios it runs rest, and the decoders on them with it.
fn runs(status: &SatelliteStatus, now: f64) -> bool {
    status.visible
        || status
            .next_pass
            .and_then(|pass| pass.aos)
            .is_some_and(|aos| aos as f64 - now <= RUN_LEAD_S)
}

fn take_acks(status: &mut SatelliteStatus, acks: Vec<ControlAck>) {
    for ack in acks {
        match ack.error {
            Some(error) => status.error = Some(format!("{}: {error}", ack.target)),
            None if ack.outranked => {}
            None if !status.driving.contains(&ack.target) => status.driving.push(ack.target),
            None => {}
        }
    }
}

fn place(
    cached: &mut GroundTrack,
    satellite: &Satellite,
    status: &mut SatelliteStatus,
    now: f64,
) -> Result<(), OrbitError> {
    status.ground = Some(track::ground(satellite, now)?);
    if cached.at.is_none_or(|at| at.elapsed() >= PASS_EVERY) {
        cached.at = Some(Instant::now());
        cached.points = track::ground_track(satellite, now)?;
    }
    status.ground_track.clone_from(&cached.points);
    Ok(())
}

fn orbit_of<'a>(
    satellites: &'a mut HashMap<String, Orbit>,
    wiring: &Wiring,
) -> Option<&'a mut Orbit> {
    let tle = wiring.settings.tle.as_ref()?;
    if satellites
        .get(&wiring.node)
        .is_none_or(|orbit| orbit.tle != *tle)
    {
        let satellite = Tle::parse(tle)
            .and_then(Satellite::new)
            .map_err(|error| error.to_string());
        satellites.insert(
            wiring.node.clone(),
            Orbit {
                tle: tle.clone(),
                satellite,
                pass: None,
                sky: Vec::new(),
                passed_at: None,
                track: GroundTrack::default(),
            },
        );
    }
    satellites.get_mut(&wiring.node)
}

pub(crate) fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{ChannelNode, GpsNode, PatchEdge, PatchGraph, PatchNode, PortRef, Position};

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

    fn edge(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
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

    struct NoRig;

    impl control::Rig for NoRig {
        fn tune(&self, _: sdrmm_wire::ChannelAddr, _: f64) -> Result<(), String> {
            Ok(())
        }

        fn steer(&self, _: sdrmm_wire::ChannelAddr, _: DopplerSteer) -> Result<(), String> {
            Ok(())
        }

        fn sweep(
            &self,
            _: sdrmm_wire::ChannelAddr,
            _: &control::Sweep<'_>,
        ) -> Result<Option<sdrmm_wire::SweepStatus>, String> {
            Ok(None)
        }

        fn halt(&self, _: sdrmm_wire::ChannelAddr, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn up(name: &str, peak_deg: f64) -> SkyTrack {
        SkyTrack {
            name: Some(name.to_owned()),
            look: sdrmm_wire::SatelliteLook {
                azimuth_deg: 90.0,
                elevation_deg: 30.0,
                range_km: 900.0,
                range_rate_km_s: 1.0,
            },
            visible: true,
            next_pass: Some(SatellitePass {
                aos: Some(0),
                los: Some(600),
                max_elevation_deg: peak_deg,
                max_at: 300,
            }),
            pass_track: Vec::new(),
        }
    }

    #[test]
    fn a_satellite_the_rotator_does_not_follow_lets_its_recorder_rest() {
        let graph = PatchGraph {
            nodes: vec![
                node("top", NodeBody::Satellite(SatelliteNode::default())),
                node("low", NodeBody::Satellite(SatelliteNode::default())),
                node(
                    "dish",
                    NodeBody::Rotator(sdrmm_wire::RotatorNode::default()),
                ),
                node("dev", NodeBody::Device(sdrmm_wire::DeviceNode::default())),
            ],
            edges: vec![
                edge(("top", "control"), ("dish", "control")),
                edge(("low", "control"), ("dish", "control")),
                edge(("low", "control"), ("dev", "control")),
            ],
        };
        let patch =
            sdrmm_wire::patch::resolve::compile(&graph, &Default::default()).expect("compiles");
        let runs = Arc::new(crate::run_gate::RunGate::default());
        let bus = ControlBus::gated_by(runs.clone());
        let resting = || {
            runs.gated(&graph)
                .node("dev")
                .is_some_and(|node| matches!(&node.body, NodeBody::Device(dev) if dev.waiting))
        };
        point_and_run(&bus, &NoRig, &patch, "top", up("top", 70.0), true);
        let (acks, on) = point_and_run(&bus, &NoRig, &patch, "low", up("low", 20.0), true);
        assert!(!on);
        assert!(
            resting(),
            "the dish follows the other pass, so the recorder rests"
        );
        assert!(acks.iter().any(|ack| ack.target == "dish" && ack.outranked));
        bus.deliver(&NoRig, &patch, "top", &ControlCommand::Release);
        let (_, on) = point_and_run(&bus, &NoRig, &patch, "low", up("low", 20.0), true);
        assert!(on);
        assert!(!resting(), "the dish came back, and so does the recording");
    }

    #[test]
    fn a_satellite_without_a_rotator_runs_on_its_pass_alone() {
        assert!(holds_a_rotator(&[]));
    }

    #[test]
    fn a_satellite_runs_its_radios_from_just_before_the_rise() {
        let rising = |aos: i64| SatelliteStatus {
            next_pass: Some(SatellitePass {
                aos: Some(aos),
                los: Some(aos + 600),
                max_elevation_deg: 40.0,
                max_at: aos + 300,
            }),
            ..SatelliteStatus::default()
        };
        assert!(!runs(&rising(1_000), 900.0));
        assert!(runs(&rising(1_000), 1_000.0 - RUN_LEAD_S));
        assert!(runs(
            &SatelliteStatus {
                visible: true,
                ..SatelliteStatus::default()
            },
            0.0
        ));
        assert!(!runs(&SatelliteStatus::default(), 0.0));
    }

    const ISS: &str = "ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000-0  30306-3 0  9999
2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.50377579432041";

    fn tracker() -> Tracker {
        Tracker {
            engine: Weak::new(),
            gps: Arc::default(),
            resolved: Arc::default(),
            control: Arc::default(),
            plan: Arc::default(),
            statuses: Arc::default(),
            stop: Arc::default(),
            satellites: HashMap::new(),
            published_at: None,
            placed: HashSet::new(),
        }
    }

    #[test]
    fn a_missing_satellite_or_position_is_a_wait_not_a_fault() {
        let mut tracker = tracker();
        let unpicked = Wiring {
            node: "sat".to_owned(),
            settings: SatelliteNode::default(),
            position: None,
        };
        let (status, steering) = tracker.follow(&unpicked, 0.0);
        assert!(steering.is_none());
        assert_eq!(status.waiting.as_deref(), Some(PICK_A_SATELLITE));
        assert_eq!(status.error, None);
        let broken = Wiring {
            settings: SatelliteNode {
                tle: Some("not elements".to_owned()),
                ..SatelliteNode::default()
            },
            ..unpicked
        };
        let (status, _) = tracker.follow(&broken, 0.0);
        assert_eq!(status.waiting, None);
        assert!(status.error.is_some());
        let unplaced = Wiring {
            settings: SatelliteNode {
                tle: Some(ISS.to_owned()),
                ..SatelliteNode::default()
            },
            ..broken
        };
        let now = 1_704_110_400.0;
        let (status, _) = tracker.follow(&unplaced, now);
        assert_eq!(status.waiting.as_deref(), Some(WIRE_A_POSITION));
        let fixless = Wiring {
            position: Some("gps".to_owned()),
            ..unplaced
        };
        let (status, _) = tracker.follow(&fixless, now);
        assert_eq!(status.waiting.as_deref(), Some(WAITING_FOR_FIX));
        assert_eq!(status.error, None);
    }

    #[test]
    fn only_the_wired_position_is_followed() {
        let channel = || {
            NodeBody::Channel(ChannelNode {
                channel_type: "nfm".to_owned(),
                tuning_locked: false,
                audio: Default::default(),
            })
        };
        let graph = PatchGraph {
            nodes: vec![
                node("sat", NodeBody::Satellite(SatelliteNode::default())),
                node("gps", NodeBody::Gps(GpsNode::default())),
                node("voice", channel()),
                node("other", channel()),
            ],
            edges: vec![
                edge(("gps", "position"), ("sat", "position")),
                edge(("sat", "control"), ("voice", "control")),
            ],
        };
        let found = wirings(&Resolved {
            patch: sdrmm_wire::patch::resolve::compile(&graph, &Default::default())
                .expect("compiles"),
            graph: Arc::new(graph),
            ..Resolved::default()
        });
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].position.as_deref(), Some("gps"));
    }
}
