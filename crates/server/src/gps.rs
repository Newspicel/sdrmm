use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError},
    },
    time::Duration,
};

use sdrmm_serial::{SerialPortInfo, SerialPortType};
use sdrmm_wire::{
    NodeBody, PositionFix, PositionSource, ProblemSource, SerialDeviceInfo, SerialDevicesResponse,
    ServerEvent,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::broadcast,
    task::JoinHandle,
};
use tokio_serial::SerialPortBuilderExt;

use crate::{AppState, problems::ProblemBoard};

mod gpsd;
mod nmea;
mod pose;
mod replay;
mod routes;

pub(crate) use routes::PositionRoutes;

use gpsd::{GpsdOutcome, GpsdState};
use nmea::NmeaState;

const EVENT_CAPACITY: usize = 128;
const RETRY_DELAY: Duration = Duration::from_secs(3);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const STABLE_SESSION: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const GPSD_MAX_LINE: usize = 16 * 1024;
const NMEA_MAX_LINE: usize = 512;
const MAX_ERROR_LEN: usize = 256;
const WAITING: &str = "waiting for a position fix";

#[derive(Clone, Debug, PartialEq)]
struct PositionState {
    fix: Option<PositionFix>,
    error: Option<String>,
}

#[derive(Default, PartialEq)]
struct GpsConfiguration {
    sources: HashMap<String, PositionSource>,
    routes: Arc<PositionRoutes>,
}

struct SourceTask {
    source: PositionSource,
    handle: JoinHandle<()>,
}

#[derive(Clone)]
struct RouteState {
    engine: Arc<sdrmm_engine::Engine>,
    problems: Arc<ProblemBoard>,
}

impl From<&AppState> for RouteState {
    fn from(state: &AppState) -> Self {
        Self {
            engine: state.engine.clone(),
            problems: state.problems.clone(),
        }
    }
}

pub(crate) struct GpsHub {
    latest: Arc<Mutex<HashMap<String, PositionState>>>,
    tasks: Mutex<HashMap<String, SourceTask>>,
    configuration: Mutex<GpsConfiguration>,
    routes: Arc<Mutex<Arc<PositionRoutes>>>,
    route_signal: Option<SyncSender<RouteState>>,
    route_worker: Option<std::thread::JoinHandle<()>>,
    clear_before_route: Arc<AtomicBool>,
    events: broadcast::Sender<ServerEvent>,
    pose_seen: Mutex<HashMap<String, tokio::time::Instant>>,
    online: Mutex<HashSet<String>>,
    replays: Mutex<HashMap<String, replay::Replay>>,
}

impl Default for GpsHub {
    fn default() -> Self {
        let latest = Arc::new(Mutex::new(HashMap::<String, PositionState>::new()));
        let route_latest = latest.clone();
        let routes = Arc::new(Mutex::new(Arc::new(PositionRoutes::default())));
        let route_table = routes.clone();
        let clear_before_route = Arc::new(AtomicBool::new(false));
        let route_clear = clear_before_route.clone();
        let (route_signal, route_rx) = std::sync::mpsc::sync_channel::<RouteState>(1);
        let route_worker = match std::thread::Builder::new()
            .name("sdrmm-gps-route".to_owned())
            .spawn(move || {
                while let Ok(state) = route_rx.recv() {
                    if route_clear.swap(false, Ordering::AcqRel) {
                        clear_position_consumers(&state);
                    }
                    let table = route_table
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    let positions = route_latest
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    for (node, current) in positions {
                        routes::push(
                            &state.engine,
                            &state.problems,
                            &table,
                            &node,
                            current.fix.as_ref(),
                        );
                    }
                }
            }) {
            Ok(worker) => Some(worker),
            Err(error) => {
                tracing::error!(%error, "could not start GPS routing thread");
                None
            }
        };
        Self {
            latest,
            tasks: Mutex::new(HashMap::new()),
            configuration: Mutex::new(GpsConfiguration::default()),
            routes,
            route_signal: Some(route_signal),
            route_worker,
            clear_before_route,
            events: broadcast::channel(EVENT_CAPACITY).0,
            pose_seen: Mutex::new(HashMap::new()),
            online: Mutex::new(HashSet::new()),
            replays: Mutex::new(HashMap::new()),
        }
    }
}

impl GpsHub {
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.events.subscribe()
    }

    pub(crate) fn fix(&self, node: &str) -> Option<PositionFix> {
        self.latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(node)
            .and_then(|state| state.fix.clone())
    }

    pub(crate) fn snapshot(&self) -> Vec<ServerEvent> {
        self.latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(node, state)| position_event(node, state))
            .collect()
    }

    pub(crate) fn reconcile(self: &Arc<Self>, state: &AppState) {
        let resolved = state.resolved.current();
        let wanted = resolved
            .graph
            .nodes
            .iter()
            .filter_map(|node| match &node.body {
                NodeBody::Gps(gps) => gps.source.clone().map(|source| (node.id.clone(), source)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let replayed = self.reconcile_replays(state, &resolved);
        let next = GpsConfiguration {
            sources: wanted.clone(),
            routes: resolved.positions.clone(),
        };
        *self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = resolved.positions.clone();
        state.problems.retain(&state.engine, |node, source| {
            source != ProblemSource::Position || resolved.positions.reaches(node)
        });
        let (configuration_changed, changed_sources) = {
            let mut current = self
                .configuration
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let changed_sources = next
                .sources
                .keys()
                .filter(|node| current.sources.get(*node) != next.sources.get(*node))
                .cloned()
                .collect::<Vec<_>>();
            let changed = *current != next;
            *current = next;
            (changed, changed_sources)
        };

        if configuration_changed {
            self.clear_before_route.store(true, Ordering::Release);
        }

        {
            let mut latest = self
                .latest
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            latest.retain(|node, _| wanted.contains_key(node) || replayed.contains(node));
            for node in &changed_sources {
                latest.remove(node);
            }
        }
        for node in &changed_sources {
            self.publish_state(state, node, None, Some(WAITING.to_owned()));
        }

        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let removed: Vec<String> = tasks
            .keys()
            .filter(|node| !wanted.contains_key(*node))
            .cloned()
            .collect();
        for node in removed {
            if let Some(task) = tasks.remove(&node) {
                task.handle.abort();
            }
        }

        for (node, source) in &wanted {
            let unchanged = tasks
                .get(node)
                .is_some_and(|task| task.source == *source && !task.handle.is_finished());
            if unchanged {
                continue;
            }
            if let Some(old) = tasks.remove(node) {
                old.handle.abort();
            }
            if matches!(
                source,
                PositionSource::Fixed { .. } | PositionSource::Phone { .. }
            ) {
                continue;
            }
            let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                self.publish_state(
                    state,
                    node,
                    None,
                    Some("no async runtime is available for this GPS source".to_owned()),
                );
                continue;
            };
            let hub = self.clone();
            let app = state.clone();
            let task_node = node.clone();
            let task_source = source.clone();
            let handle = runtime.spawn(async move {
                match task_source {
                    PositionSource::Gpsd { address } => {
                        run_gpsd(hub, app, task_node, address).await
                    }
                    PositionSource::Nmea {
                        device,
                        baud,
                        update_interval_ms,
                    } => {
                        run_nmea(
                            hub,
                            app,
                            task_node,
                            device,
                            baud,
                            Duration::from_millis(u64::from(update_interval_ms)),
                        )
                        .await;
                    }
                    PositionSource::Fixed { .. } | PositionSource::Phone { .. } => {}
                }
            });
            tasks.insert(
                node.clone(),
                SourceTask {
                    source: source.clone(),
                    handle,
                },
            );
        }
        drop(tasks);
        self.publish_standing_sources(state, &wanted);
        self.route_current(state);
    }

    fn publish_standing_sources(&self, state: &AppState, wanted: &HashMap<String, PositionSource>) {
        for (node, source) in wanted {
            match source {
                PositionSource::Fixed {
                    lat,
                    lon,
                    altitude_m,
                } => {
                    self.publish_state(state, node, Some(fixed_fix(*lat, *lon, *altitude_m)), None);
                }
                PositionSource::Phone { phone } => self.publish_phone_standing(state, node, phone),
                PositionSource::Gpsd { .. } | PositionSource::Nmea { .. } => {}
            }
        }
    }

    fn publish_state(
        &self,
        state: &AppState,
        node: &str,
        fix: Option<PositionFix>,
        error: Option<String>,
    ) {
        self.publish_routed(RouteState::from(state), node, fix, error);
    }

    fn publish_routed(
        &self,
        route: RouteState,
        node: &str,
        fix: Option<PositionFix>,
        error: Option<String>,
    ) {
        let next = PositionState { fix, error };
        let changed = {
            let mut latest = self
                .latest
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if latest.get(node) == Some(&next) {
                false
            } else {
                latest.insert(node.to_owned(), next.clone());
                true
            }
        };
        if changed {
            self.send_route(route);
            let _ = self.events.send(position_event(node, &next));
        }
    }

    pub(crate) fn route_current(&self, state: &AppState) {
        self.queue_route(state);
    }

    pub(crate) fn route_for(
        &self,
        engine: &Arc<sdrmm_engine::Engine>,
        problems: &Arc<ProblemBoard>,
    ) {
        self.send_route(RouteState {
            engine: engine.clone(),
            problems: problems.clone(),
        });
    }

    fn queue_route(&self, state: &AppState) {
        self.send_route(RouteState::from(state));
    }

    fn send_route(&self, route: RouteState) {
        let Some(route_signal) = &self.route_signal else {
            return;
        };
        match route_signal.try_send(route) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {
                tracing::error!("GPS routing thread stopped");
            }
        }
    }
}

impl Drop for GpsHub {
    fn drop(&mut self) {
        self.route_signal.take();
        if let Some(worker) = self.route_worker.take()
            && worker.join().is_err()
        {
            tracing::error!("GPS routing thread panicked");
        }
    }
}

fn fixed_fix(lat: f64, lon: f64, altitude_m: Option<f64>) -> PositionFix {
    PositionFix {
        latitude: lat,
        longitude: lon,
        altitude_m,
        accuracy_m: None,
        speed_mps: None,
        track_deg: None,
        time: crate::store::rfc3339(jiff::Timestamp::now()),
        attitude: sdrmm_wire::Attitude::default(),
    }
}

fn position_event(node: &str, state: &PositionState) -> ServerEvent {
    ServerEvent::PositionChanged {
        node: node.to_owned(),
        fix: state.fix.clone(),
        error: state.error.clone(),
    }
}

fn clear_position_consumers(state: &RouteState) {
    let snapshot = state.engine.snapshot();
    for set in snapshot.device_sets {
        for channel in set.channels {
            if let Err(error) = state
                .engine
                .update_channel_position(set.id, channel.id, None)
            {
                tracing::debug!(%error, channel = channel.id, "could not clear channel GPS fix");
            }
        }
        if set.recording.is_some()
            && let Err(error) = state.engine.update_recording_position(set.id, None)
        {
            tracing::debug!(%error, device_set = set.id, "could not clear recording GPS fix");
        }
        if set.pre_roll.is_some()
            && let Err(error) = state.engine.update_pre_roll_position(set.id, None)
        {
            tracing::debug!(%error, device_set = set.id, "could not clear pre-roll GPS fix");
        }
    }
}

async fn run_gpsd(hub: Arc<GpsHub>, state: AppState, node: String, address: String) {
    retry_forever(&hub, &state, &node, || {
        gpsd_session(&hub, &state, &node, &address)
    })
    .await;
}

async fn retry_forever<F, Fut>(hub: &GpsHub, state: &AppState, node: &str, mut session: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let mut retry = RETRY_DELAY;
    loop {
        let started = std::time::Instant::now();
        if let Err(error) = session().await {
            hub.publish_state(state, node, None, Some(limit_error(error)));
        }
        let delay = retry;
        retry = if started.elapsed() >= STABLE_SESSION {
            RETRY_DELAY
        } else {
            retry.saturating_mul(2).min(MAX_RETRY_DELAY)
        };
        tokio::time::sleep(delay).await;
    }
}

async fn gpsd_session(
    hub: &GpsHub,
    state: &AppState,
    node: &str,
    address: &str,
) -> Result<(), String> {
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(address))
        .await
        .map_err(|_| format!("gpsd {address}: connection timed out"))?
        .map_err(|error| format!("gpsd {address}: {error}"))?;
    let (read, mut write) = stream.into_split();
    write
        .write_all(b"?WATCH={\"enable\":true,\"json\":true};\n")
        .await
        .map_err(|error| format!("gpsd watch: {error}"))?;
    let mut reader = BufReader::new(read);
    let mut parser = GpsdState::default();
    while let Some(line) =
        tokio::time::timeout(READ_TIMEOUT, read_bounded_line(&mut reader, GPSD_MAX_LINE))
            .await
            .map_err(|_| "gpsd read timed out".to_owned())?
            .map_err(|error| format!("gpsd read: {error}"))?
    {
        match parser.line(&line, std::time::Instant::now()) {
            GpsdOutcome::Fix(fix) => hub.publish_state(state, node, Some(fix), None),
            GpsdOutcome::NoFix => hub.publish_state(
                state,
                node,
                None,
                Some("gpsd has no position fix".to_owned()),
            ),
            GpsdOutcome::Nothing => {}
        }
    }
    Err("gpsd connection closed".to_owned())
}

async fn run_nmea(
    hub: Arc<GpsHub>,
    state: AppState,
    node: String,
    device: String,
    baud: u32,
    update_interval: Duration,
) {
    retry_forever(&hub, &state, &node, || {
        nmea_session(&hub, &state, &node, &device, baud, update_interval)
    })
    .await;
}

async fn nmea_session(
    hub: &GpsHub,
    state: &AppState,
    node: &str,
    device: &str,
    baud: u32,
    update_interval: Duration,
) -> Result<(), String> {
    let device_name = device.to_owned();
    let open_device = device_name.clone();
    let serial = tokio::task::spawn_blocking(move || {
        tokio_serial::new(open_device, baud).open_native_async()
    })
    .await
    .map_err(|error| format!("NMEA {device_name}: open task failed: {error}"))?
    .map_err(|error| format!("NMEA {device_name}: {error}"))?;
    let mut reader = BufReader::new(serial);
    let mut parser = NmeaState::default();
    let mut published_at: Option<std::time::Instant> = None;
    while let Some(line) =
        tokio::time::timeout(READ_TIMEOUT, read_bounded_line(&mut reader, NMEA_MAX_LINE))
            .await
            .map_err(|_| "NMEA read timed out".to_owned())?
            .map_err(|error| format!("NMEA read: {error}"))?
    {
        if let Some(fix) = parser.parse(&line, std::time::Instant::now())
            && published_at.is_none_or(|last| last.elapsed() >= update_interval)
        {
            hub.publish_state(state, node, Some(fix), None);
            published_at = Some(std::time::Instant::now());
        }
    }
    Err("NMEA device closed".to_owned())
}

async fn read_bounded_line<R>(reader: &mut R, max_len: usize) -> std::io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::with_capacity(max_len.min(256));
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |at| at + 1);
        if line.len().saturating_add(take) > max_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "position source line is too long",
            ));
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            break;
        }
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8(line).map(Some).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "position source line is not UTF-8",
        )
    })
}

pub(crate) fn nmea_devices() -> Result<SerialDevicesResponse, String> {
    let ports = sdrmm_serial::ports()
        .map_err(|error| format!("list serial devices: {error}"))?
        .into_iter()
        .map(serial_device_info)
        .collect();
    Ok(SerialDevicesResponse {
        devices: offered_ports(ports),
    })
}

pub(crate) fn serial_ports() -> Result<SerialDevicesResponse, String> {
    let mut devices: Vec<SerialDeviceInfo> = sdrmm_serial::ports()
        .map_err(|error| format!("list serial devices: {error}"))?
        .into_iter()
        .map(serial_device_info)
        .collect();
    devices.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(SerialDevicesResponse { devices })
}

fn offered_ports(mut ports: Vec<SerialDeviceInfo>) -> Vec<SerialDeviceInfo> {
    ports.retain(|port| receiver_rank(port) == RECEIVER || !describes(port, &PERIPHERAL_WORDS));
    ports.sort_by(|left, right| {
        receiver_rank(left)
            .cmp(&receiver_rank(right))
            .then_with(|| left.path.cmp(&right.path))
    });
    let receivers = ports
        .iter()
        .filter(|port| receiver_rank(port) == RECEIVER)
        .cloned()
        .collect::<Vec<_>>();
    if receivers.is_empty() {
        ports
    } else {
        receivers
    }
}

const GNSS_USB_VIDS: [u16; 2] = [0x1546, 0x091e];

const GNSS_WORDS: [&str; 10] = [
    "gps", "gnss", "glonass", "galileo", "beidou", "u-blox", "ublox", "garmin", "sirf", "navilock",
];

const PERIPHERAL_WORDS: [&str; 14] = [
    "monitor",
    "display",
    "keyboard",
    "mouse",
    "trackpad",
    "touchpad",
    "webcam",
    "camera",
    "headset",
    "speaker",
    "microphone",
    "printer",
    "scanner",
    "hub",
];

const RECEIVER: u8 = 0;

fn describes(device: &SerialDeviceInfo, words: &[&str]) -> bool {
    device
        .product
        .iter()
        .chain(device.manufacturer.iter())
        .any(|text| {
            let text = text.to_lowercase();
            words.iter().any(|word| text.contains(word))
        })
}

fn receiver_rank(device: &SerialDeviceInfo) -> u8 {
    if describes(device, &GNSS_WORDS)
        || device
            .usb_vid
            .is_some_and(|vid| GNSS_USB_VIDS.contains(&vid))
    {
        RECEIVER
    } else if device.usb_vid.is_some() {
        1
    } else {
        2
    }
}

fn serial_device_info(info: SerialPortInfo) -> SerialDeviceInfo {
    let mut device = SerialDeviceInfo {
        path: info.port_name,
        product: None,
        manufacturer: None,
        serial: None,
        usb_vid: None,
        usb_pid: None,
    };
    match info.port_type {
        SerialPortType::UsbPort(usb) => {
            device.product = usb.product;
            device.manufacturer = usb.manufacturer;
            device.serial = usb.serial_number;
            device.usb_vid = Some(usb.vid);
            device.usb_pid = Some(usb.pid);
        }
        SerialPortType::BluetoothPort => device.product = Some("Bluetooth serial".to_owned()),
        SerialPortType::PciPort => device.product = Some("PCI serial".to_owned()),
        SerialPortType::Unknown => {}
    }
    device
}

fn now() -> String {
    jiff::Timestamp::now().to_string()
}

fn limit_error(error: impl Into<String>) -> String {
    error.into().chars().take(MAX_ERROR_LEN).collect()
}

#[cfg(test)]
mod tests;
