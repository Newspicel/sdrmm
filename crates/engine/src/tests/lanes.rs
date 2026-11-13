use sdrmm_wire::DeviceBackend;
use std::sync::{Arc, Mutex};

use super::*;

#[derive(Clone, Default)]
struct Started(Arc<Mutex<Vec<usize>>>);

struct LanesDriver(Started);

impl DeviceDriver for LanesDriver {
    fn id(&self) -> DeviceBackend {
        DeviceBackend::Rtlsdr
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        vec![mock_info("lanes", None)]
    }

    fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        Ok(Box::new(LanesDevice {
            capabilities: Capabilities {
                rx_streams: 2,
                per_stream: StreamScope {
                    gain: true,
                    ..StreamScope::default()
                },
                rx_inputs: vec!["RX1".to_string(), "RX2".to_string()],
                ..empty_capabilities()
            },
            settings: DeviceSettings {
                center_hz: Some(TEST_CENTER_HZ),
                rx_inputs: Some(vec![0, 1]),
                ..mock_settings()
            },
            started: self.0.clone(),
        }))
    }
}

struct LanesDevice {
    capabilities: Capabilities,
    settings: DeviceSettings,
    started: Started,
}

impl SdrDevice for LanesDevice {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        if let Some(inputs) = &settings.rx_inputs {
            self.capabilities.rx_streams = inputs.len() as u32;
        }
        self.settings.merge_from(settings);
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        lock(&self.started.0).push(sinks.len());
        for mut sink in sinks {
            sink.push(&[Complex::new(0.0, 0.0); 16]);
            sink.dropped(1_000_000);
            sink.push(&[Complex::new(0.0, 0.0); 16]);
        }
        Ok(())
    }

    fn rx_stop(&mut self) {}
}

fn lanes_engine() -> (Arc<Engine>, Started) {
    lanes_engine_in(None)
}

fn lanes_engine_in(recordings: Option<&Path>) -> (Arc<Engine>, Started) {
    let started = Started::default();
    let mut registry = DeviceRegistry::new();
    registry.register(50, Box::new(LanesDriver(started.clone())));
    (
        Engine::with_registry(registry, recordings.map(Path::to_path_buf)),
        started,
    )
}

fn set_of(engine: &Engine, ds: u32) -> sdrmm_wire::DeviceSet {
    engine
        .snapshot()
        .device_sets
        .into_iter()
        .find(|set| set.id == ds)
        .expect("set listed")
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn both_lanes_drained(engine: &Engine, ds: u32) {
    until("the first capture never counted its gaps", || {
        set_of(engine, ds).overruns >= 2_000_000
    })
    .await;
}

fn inputs(picked: &[u32]) -> DeviceSettings {
    DeviceSettings {
        rx_inputs: Some(picked.to_vec()),
        ..DeviceSettings::default()
    }
}

#[tokio::test]
async fn a_new_pick_of_inputs_restarts_the_capture_with_only_those_lanes() {
    let (engine, started) = lanes_engine();
    let ds = engine.create_device_set("rtlsdr:lanes").unwrap();
    engine.patch_device(ds, inputs(&[0])).expect("RX1");
    assert_eq!(engine.snapshot().device_sets[0].capabilities.rx_streams, 1);
    engine.patch_device(ds, inputs(&[1])).expect("RX2");
    engine.patch_device(ds, inputs(&[1])).expect("RX2 again");
    engine.patch_device(ds, inputs(&[0, 1])).expect("both");
    assert_eq!(*lock(&started.0), vec![2, 1, 1, 2]);
    engine.remove_device_set(ds).unwrap();
}

#[tokio::test]
async fn a_lane_that_is_wired_is_not_switched_off() {
    let (engine, started) = lanes_engine();
    let ds = engine.create_device_set("rtlsdr:lanes").unwrap();
    hold_tuning(&engine, ds);
    engine
        .add_channel(ds, 1, nfm_settings(0.0))
        .expect("a decoder on iq2");
    let refused = engine.patch_device(ds, inputs(&[1])).unwrap_err();
    assert!(refused.to_string().contains("iq2"), "{refused}");
    assert_eq!(engine.snapshot().device_sets[0].capabilities.rx_streams, 2);
    assert_eq!(*lock(&started.0), vec![2]);
    assert!(
        engine.patch_device(ds, inputs(&[0, 2])).is_err(),
        "an input the radio does not have"
    );
    engine.remove_device_set(ds).unwrap();
}

#[tokio::test]
async fn drops_counted_before_a_lane_restart_do_not_upset_the_next_poll() {
    let (engine, _) = lanes_engine();
    let ds = engine.create_device_set("rtlsdr:lanes").unwrap();
    let mut known = None;
    let mut missing_once = HashSet::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while engine.snapshot().device_sets[0].overruns < 2_000_000 {
        assert!(Instant::now() < deadline, "the gaps were never counted");
        engine.hotplug_tick_for_test(&mut known, &mut missing_once);
        std::thread::sleep(Duration::from_millis(20));
    }
    engine.hotplug_tick_for_test(&mut known, &mut missing_once);
    engine.patch_device(ds, inputs(&[0])).expect("one lane");
    std::thread::sleep(Duration::from_millis(50));
    engine.hotplug_tick_for_test(&mut known, &mut missing_once);
    let set = &engine.snapshot().device_sets[0];
    assert!(set.overruns < 2_000_000, "{}", set.overruns);
    engine.remove_device_set(ds).unwrap();
}

fn udp_export(receiver: &std::net::UdpSocket) -> sdrmm_wire::NetworkExportSettings {
    sdrmm_wire::NetworkExportSettings {
        address: receiver.local_addr().expect("address").to_string(),
        transport: sdrmm_wire::NetworkTransport::Udp,
        format: sdrmm_wire::NetworkSampleFormat::Cf32Le,
    }
}

#[tokio::test]
async fn sinks_count_overruns_from_their_own_start_across_a_lane_restart() {
    let dir = tempfile::TempDir::new().unwrap();
    let receiver = std::net::UdpSocket::bind("127.0.0.1:0").expect("receiver");
    let (engine, _) = lanes_engine_in(Some(dir.path()));
    let ds = engine.create_device_set("rtlsdr:lanes").unwrap();
    both_lanes_drained(&engine, ds).await;
    engine.start_recording(ds, 0, None).expect("recording");
    engine
        .arm_pre_roll(ds, "tm".to_owned(), 0, 1)
        .expect("armed");
    engine
        .start_network_export(ds, "net".to_owned(), 0, udp_export(&receiver))
        .expect("exporting");

    engine.patch_device(ds, inputs(&[0])).expect("one lane");
    let set = set_of(&engine, ds);
    assert_eq!(set.overruns, 1_000_000, "the radio counts its own run");
    let recording = set.recording.expect("still recording");
    assert_eq!((recording.overruns, recording.error), (1_000_000, None));
    let history = set.pre_roll.expect("still holding");
    assert_eq!((history.overruns, history.error), (1_000_000, None));
    let export = set.network_export.expect("still exporting");
    assert_eq!((export.overruns, export.error), (1_000_000, None));
    assert_eq!(
        engine.stop_recording(ds).expect("stopped").overruns,
        1_000_000
    );
    engine.remove_device_set(ds).unwrap();
}

#[tokio::test]
async fn a_recording_holds_its_lane_against_an_input_change() {
    let dir = tempfile::TempDir::new().unwrap();
    let (engine, _) = lanes_engine_in(Some(dir.path()));
    let ds = engine.create_device_set("rtlsdr:lanes").unwrap();
    engine.start_recording(ds, 1, None).expect("recording iq2");
    let refused = engine.patch_device(ds, inputs(&[0])).unwrap_err();
    assert!(refused.to_string().contains("iq2"), "{refused}");
    let recording = set_of(&engine, ds).recording.expect("still recording");
    assert_eq!(recording.error, None);
    engine.remove_device_set(ds).unwrap();
}

async fn recorded_past(engine: &Engine, ds: u32, samples: u64) -> u64 {
    let mut now = 0;
    until("the recording stopped growing", || {
        now = set_of(engine, ds).recording.map_or(0, |r| r.samples);
        now > samples
    })
    .await;
    now
}

#[tokio::test]
async fn an_iq_recording_carries_on_across_a_runtime_restart_and_marks_the_seam() {
    let dir = tempfile::TempDir::new().unwrap();
    let engine = recording_engine(dir.path());
    let ds = engine.create_device_set("virtual:band").unwrap();
    engine.start_recording(ds, 0, None).expect("recording");
    let before = recorded_past(&engine, ds, 0).await;

    engine.restart_lanes(ds).expect("restarted");
    recorded_past(&engine, ds, before + 100_000).await;
    let finalized = engine.stop_recording(ds).expect("stopped");

    assert_eq!(finalized.error, None);
    let reader = sdrmm_recorder::SigmfReader::open(&finalized.stem).unwrap();
    let captures = &reader.meta().captures;
    let seam = captures
        .iter()
        .find(|capture| capture.global_index.is_some())
        .expect("the restart is a capture segment");
    assert!(seam.sample_start >= before, "{captures:?}");
    assert!(seam.global_index >= Some(seam.sample_start));
    assert!(seam.datetime.is_some());
    assert_eq!(seam.frequency, captures[0].frequency);
    engine.remove_device_set(ds).unwrap();
}

#[tokio::test]
async fn a_time_machine_and_a_network_export_carry_on_across_a_runtime_restart() {
    let receiver = std::net::UdpSocket::bind("127.0.0.1:0").expect("receiver");
    let dir = tempfile::TempDir::new().unwrap();
    let engine = recording_engine(dir.path());
    let ds = engine.create_device_set("virtual:band").unwrap();
    engine
        .arm_pre_roll(ds, "tm".to_owned(), 0, 1)
        .expect("armed");
    engine
        .start_network_export(ds, "net".to_owned(), 0, udp_export(&receiver))
        .expect("exporting");
    let mut sent = 0;
    until("the export never sent", || {
        sent = set_of(&engine, ds).network_export.map_or(0, |e| e.samples);
        sent > 0
    })
    .await;

    engine.restart_lanes(ds).expect("restarted");
    engine.start_pre_roll_capture(ds, "tm").expect("capture");
    until("the export stopped after the restart", || {
        set_of(&engine, ds)
            .network_export
            .is_some_and(|e| e.samples > sent + 100_000)
    })
    .await;
    until("the time machine stopped after the restart", || {
        set_of(&engine, ds)
            .pre_roll
            .and_then(|history| history.capture)
            .is_some_and(|capture| capture.samples > 100_000)
    })
    .await;

    let export = engine.stop_network_export(ds, "net").expect("stopped");
    assert_eq!(export.error, None);
    engine.stop_pre_roll_capture(ds, "tm").expect("captured");
    let history = engine.drop_pre_roll(ds, "tm").expect("disarmed");
    assert_eq!(history.error, None);
    engine.remove_device_set(ds).unwrap();
}

#[test]
fn the_share_lost_is_what_went_missing_of_what_the_radio_sampled() {
    assert_eq!(crate::loss_share(0, 1.0, 1e6), None);
    assert_eq!(crate::loss_share(750_000, 1.0, 1e6), Some(0.75));
    assert_eq!(
        crate::loss_share(10, 1.0, 1e6),
        Some(0.01),
        "any loss shows"
    );
    assert_eq!(crate::loss_share(5_000_000, 1.0, 1e6), Some(1.0));
    assert_eq!(crate::loss_share(10, 0.0, 1e6), None);
}
