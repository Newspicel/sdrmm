use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use sdrmm_device::{
    DeviceBackend, DeviceDriver, DeviceError, RxSink, SdrDevice, playback::PlaybackShared,
};

use super::*;

const RATE: f64 = 2_048_000.0;
const CHUNK: usize = 2_048;
const LEAD_SECONDS: f64 = 0.1;
const LISTENERS: usize = 2;
const RETUNE_EVERY_FRAMES: u64 = sdrmm_channels::AUDIO_RATE as u64 / 4;
const PATIENCE: Duration = Duration::from_secs(60);
const STALL: Duration = Duration::from_secs(1);
const FRAME: u64 = sdrmm_engine::audio::OPUS_FRAME_SAMPLES as u64;

#[derive(Default)]
struct Ear {
    ready: AtomicBool,
    heard: AtomicU64,
}

#[derive(Default)]
struct Pace {
    ears: [Ear; LISTENERS],
}

impl Pace {
    fn heard(&self) -> Option<u64> {
        self.ears
            .iter()
            .all(|ear| ear.ready.load(Ordering::Acquire))
            .then(|| {
                self.ears
                    .iter()
                    .map(|ear| ear.heard.load(Ordering::Acquire))
                    .min()
            })
            .flatten()
    }

    fn allowed_samples(&self) -> u64 {
        self.heard().map_or(0, |frames| {
            ((frames as f64 / f64::from(sdrmm_channels::AUDIO_RATE) + LEAD_SECONDS) * RATE) as u64
        })
    }

    fn wait_until_heard(&self, frames: u64) {
        let deadline = Instant::now() + PATIENCE;
        while self.heard().is_none_or(|heard| heard < frames) {
            assert!(
                Instant::now() < deadline,
                "listeners stalled before {frames} frames"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

struct Gate {
    allowed: u64,
    since: Instant,
}

impl Gate {
    fn new() -> Self {
        Self {
            allowed: 0,
            since: Instant::now(),
        }
    }

    fn allows(&mut self, pace: &Pace, produced: u64) -> bool {
        let allowed = pace.allowed_samples();
        if allowed != self.allowed {
            self.allowed = allowed;
            self.since = Instant::now();
        }
        let stalls = (self.since.elapsed().as_secs_f64() / STALL.as_secs_f64()).floor();
        let grace = (stalls * LEAD_SECONDS * RATE) as u64;
        produced + CHUNK as u64 <= allowed + grace
    }
}

struct PacedDriver {
    pace: Arc<Pace>,
}

impl DeviceDriver for PacedDriver {
    fn id(&self) -> DeviceBackend {
        DeviceBackend::Sdrconnect
    }

    fn probe(&self) -> Vec<sdrmm_wire::DeviceInfo> {
        vec![sdrmm_wire::DeviceInfo {
            driver: DeviceBackend::Sdrconnect,
            key: "band".to_string(),
            label: "Paced band".to_string(),
            serial: None,
            profile: None,
        }]
    }

    fn open(&self, _info: &sdrmm_wire::DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        let band = sdrmm_device_virtual::VirtualDriver::new();
        let info = band
            .probe()
            .into_iter()
            .find(|info| info.key == "band")
            .ok_or_else(|| DeviceError::NotFound("virtual:band".to_string()))?;
        let band = band.open(&info)?;
        Ok(Box::new(PacedBand {
            capabilities: band.capabilities().clone(),
            settings: band.settings().clone(),
            pace: self.pace.clone(),
            playback: Arc::new(PlaybackShared::new(u64::MAX)),
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
        }))
    }
}

struct PacedBand {
    capabilities: sdrmm_wire::Capabilities,
    settings: sdrmm_wire::DeviceSettings,
    pace: Arc<Pace>,
    playback: Arc<PlaybackShared>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl SdrDevice for PacedBand {
    fn capabilities(&self) -> &sdrmm_wire::Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &sdrmm_wire::DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &sdrmm_wire::DeviceSettings) -> Result<(), DeviceError> {
        self.settings.merge_from(settings);
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        let mut sink = sdrmm_device::single_rx_sink(sinks)?;
        let pace = self.pace.clone();
        let stop = self.stop.clone();
        self.worker = Some(std::thread::spawn(move || {
            let block = sdrmm_device_virtual::render(RATE, CHUNK);
            let started = Instant::now();
            let mut gate = Gate::new();
            let mut produced = 0u64;
            while !stop.load(Ordering::Acquire) {
                let due = started + Duration::from_secs_f64(produced as f64 / RATE);
                let room = sink.room().is_none_or(|room| room.free() >= CHUNK);
                if !room || Instant::now() < due || !gate.allows(&pace, produced) {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                sink.push(&block);
                produced += CHUNK as u64;
            }
        }));
        Ok(())
    }

    fn rx_stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    fn playback(&self) -> Option<Arc<PlaybackShared>> {
        Some(self.playback.clone())
    }
}

#[derive(Default)]
struct AudioClock {
    next: Option<(u32, u64)>,
    packets: usize,
}

impl AudioClock {
    fn receive(&mut self, bytes: &[u8]) -> u64 {
        assert!(bytes.len() > sdrmm_wire::HEADER_LEN + 1);
        let sequence = u32::from_le_bytes(bytes[4..8].try_into().expect("sequence"));
        let timestamp = u64::from_le_bytes(bytes[8..16].try_into().expect("timestamp"));
        let shed = self.next.map_or(0, |(due_sequence, due_timestamp)| {
            let shed = u64::from(sequence.wrapping_sub(due_sequence));
            assert_eq!(
                timestamp,
                due_timestamp + shed * FRAME,
                "WebSocket audio gap at sequence {sequence}"
            );
            shed
        });
        self.next = Some((sequence.wrapping_add(1), timestamp + FRAME));
        self.packets += 1;
        shed
    }

    fn heard(&self) -> u64 {
        self.next.map_or(0, |(_, timestamp)| timestamp)
    }
}

async fn listen(mut socket: WsClient, channels: usize, ear: &Ear) {
    subscribe_with(
        &mut socket,
        "scope",
        "iq",
        sdrmm_wire::StreamParams {
            fps: 30,
            bins: 4096,
            spectrum: false,
        },
    )
    .await;
    send(
        &mut socket,
        &ClientCommand::SubscribeDiagnostics { enabled: true },
    )
    .await;
    subscribe_port(&mut socket, "spk", "audio").await;
    let mut audio = BTreeMap::<u16, AudioClock>::new();
    let mut spectra = 0;
    let mut diagnostics = 0;
    let mut shed = 0;
    let mut reported = 0;
    timeout(PATIENCE, async {
        loop {
            let message = socket.next().await.expect("open socket").expect("frame");
            match message {
                tungstenite::Message::Text(text) => {
                    match serde_json::from_str::<ServerEvent>(&text).expect("event") {
                        ServerEvent::PortStream {
                            stream_id,
                            kind: StreamKind::Audio,
                            ..
                        } => {
                            assert!(audio.insert(stream_id, AudioClock::default()).is_none());
                            ear.ready.store(audio.len() == channels, Ordering::Release);
                        }
                        ServerEvent::PipelineHealth { queues, websocket } => {
                            diagnostics += 1;
                            reported = websocket.dropped;
                            let lossy: Vec<_> = queues
                                .iter()
                                .filter(|queue| queue.health.dropped > 0)
                                .collect();
                            assert!(lossy.is_empty(), "queues lost frames: {lossy:?}");
                        }
                        ServerEvent::Error { message } => panic!("stream failed: {message}"),
                        ServerEvent::StreamStopped { .. } => panic!("stream stopped early"),
                        _ => {}
                    }
                }
                tungstenite::Message::Binary(bytes) => {
                    assert!(bytes.len() >= sdrmm_wire::HEADER_LEN);
                    assert_eq!(bytes[0], sdrmm_wire::PROTOCOL_VERSION);
                    let stream = u16::from_le_bytes(bytes[2..4].try_into().expect("stream"));
                    match sdrmm_wire::FrameKind::from_u8(bytes[1]).expect("frame kind") {
                        sdrmm_wire::FrameKind::AudioOpus => {
                            shed += audio
                                .get_mut(&stream)
                                .expect("announced audio")
                                .receive(&bytes);
                            let heard = audio.values().map(AudioClock::heard).min();
                            ear.heard.store(heard.unwrap_or(0), Ordering::Release);
                        }
                        sdrmm_wire::FrameKind::Spectrum => spectra += 1,
                        other => panic!("unexpected frame {other:?}"),
                    }
                }
                tungstenite::Message::Close(_) => panic!("connection closed early"),
                _ => {}
            }
            if audio.len() == channels
                && audio.values().all(|clock| clock.packets >= 100)
                && spectra >= 30
                && diagnostics > 0
                && reported >= shed
            {
                break;
            }
        }
    })
    .await
    .expect("audio sustained two seconds, every shed packet reported by the server");
    socket.close(None).await.expect("close");
}

fn paced_engine(pace: Arc<Pace>) -> Arc<Engine> {
    let mut registry = sdrmm_device::DeviceRegistry::new();
    registry.register(1, Box::new(PacedDriver { pace }));
    Engine::with_registry(registry, None)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn simultaneous_socket_listeners_keep_audio_continuous_during_retunes() {
    let names: Vec<String> = (0..8).map(|index| format!("ch{index}")).collect();
    let mut nodes = vec![
        radio_of(DeviceBackend::Sdrconnect, "band"),
        of("scope", sdrmm_wire::NodeKind::Scope),
        of("spk", sdrmm_wire::NodeKind::Speaker),
    ];
    let mut edges = vec![wire(("radio", "iq"), ("scope", "iq"))];
    for name in &names {
        nodes.push(channel(name, "nfm"));
        edges.push(wire(("radio", "iq"), (name, "iq")));
        edges.push(wire((name, "audio"), ("spk", "audio")));
    }
    let pace = Arc::new(Pace::default());
    let engine = paced_engine(pace.clone());
    let (address, state) = serve_ws(engine.clone()).await;
    workspace_on(&state, graph(nodes, edges)).await;
    let resolved = state.resolved.current();
    let ds = resolved.patch.device_set("radio").expect("radio open");
    let channels: Vec<u32> = names
        .iter()
        .map(|name| resolved.patch.channel(name).expect("channel open").channel)
        .collect();
    let first = dial(address).await;
    let second = dial(address).await;
    let control = {
        let engine = engine.clone();
        let channels = channels.clone();
        let pace = pace.clone();
        tokio::task::spawn_blocking(move || {
            for round in 0..6 {
                pace.wait_until_heard((round + 1) * RETUNE_EVERY_FRAMES);
                for (index, &channel) in channels.iter().enumerate() {
                    let offset = index as f64 * 25_000.0 + (round % 2) as f64 * 1_000.0;
                    engine
                        .patch_channel(ds, channel, nfm_channel(offset))
                        .expect("retune");
                }
            }
        })
    };
    tokio::join!(
        listen(first, channels.len(), &pace.ears[0]),
        listen(second, channels.len(), &pace.ears[1])
    );
    control.await.expect("retune task");
    assert_eq!(engine.snapshot().device_sets[0].overruns, 0);
    engine.remove_device_set(ds).expect("remove radio");
}
