use std::sync::{Arc, Mutex, OnceLock};

use num_complex::Complex;
use sdrmm_device::{
    DeviceDriver, DeviceError, GapScope, LaneEvent, LaneMark, RxSink, Sample, SdrDevice, SinkItem,
    SweepBand, SweepPlan, SweepSink, TxStream, Uncertainty,
};
use sdrmm_wire::{
    BackendReach, BackendSpec, Capabilities, DeviceBackend, DeviceInfo, DeviceSettings,
};

use crate::{
    abi::{HostV1, PluginV1, Str},
    guest, host,
};

const FAKE: DeviceBackend = DeviceBackend::new("fake");

fn capabilities() -> Capabilities {
    serde_json::from_value(serde_json::json!({
        "freq_ranges": [], "sample_rates": [1e6], "gains": [], "antennas": [], "bandwidths": [],
        "tx_streams": 1,
    }))
    .expect("capabilities")
}

fn info(key: &str) -> DeviceInfo {
    DeviceInfo {
        driver: FAKE,
        key: key.to_owned(),
        label: format!("Fake {key}"),
        serial: Some(format!("s{key}")),
        profile: None,
    }
}

struct FakeDriver;

impl DeviceDriver for FakeDriver {
    fn id(&self) -> DeviceBackend {
        FAKE
    }

    fn spec(&self) -> BackendSpec {
        BackendSpec::new(FAKE, "Fake radio", BackendReach::Local).soapy_module("fake")
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        vec![info("0")]
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        match info.key.as_str() {
            "panic" => panic!("boom while opening"),
            "missing" => Err(DeviceError::NotFound("missing".to_owned())),
            _ => Ok(Box::new(FakeDevice {
                capabilities: capabilities(),
                settings: DeviceSettings::default(),
                key: info.key.clone(),
                held: Vec::new(),
                broken: false,
            })),
        }
    }
}

struct FakeDevice {
    capabilities: Capabilities,
    settings: DeviceSettings,
    key: String,
    held: Vec<RxSink>,
    broken: bool,
}

fn block() -> [Sample; 3] {
    [
        Complex::new(1.0, 0.0),
        Complex::new(0.0, 1.0),
        Complex::new(-1.0, 0.5),
    ]
}

impl SdrDevice for FakeDevice {
    fn capabilities(&self) -> &Capabilities {
        assert!(!self.broken, "capabilities gone");
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        if self.key == "fragile" {
            self.broken = true;
            return Err(DeviceError::Unsupported("fragile".to_owned()));
        }
        if settings.sample_rate == Some(7.0) {
            return Err(DeviceError::Unsupported("sample_rate 7".to_owned()));
        }
        self.settings = settings.clone();
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        for mut sink in sinks {
            if self.key == "thread" {
                std::thread::spawn(move || {
                    sink.push(&block());
                    panic!("boom in the stream");
                });
                continue;
            }
            sink.push(&block());
            sink.dropped(10);
            sink.deaf(5);
            sink.dropped_estimate(3, 2, GapScope::Device);
            sink.mark(LaneMark::Retuned { in_flight: 4 });
            sink.stamp_hardware(-9);
            sink.push(&block());
            self.held.push(sink);
        }
        Ok(())
    }

    fn rx_stop(&mut self) {
        self.held.clear();
    }

    fn tx_start_channels(&mut self, channels: &[u32]) -> Result<Box<dyn TxStream>, DeviceError> {
        Ok(Box::new(FakeTx(channels.len())))
    }

    fn readouts(&self) -> Result<Vec<sdrmm_wire::Readout>, DeviceError> {
        Ok(vec![sdrmm_wire::Readout::flag("lock", "Lock", true)])
    }

    fn sweep_start(&mut self, plan: &SweepPlan, mut sink: SweepSink) -> Result<(), DeviceError> {
        for band in &plan.bands {
            sink.push(band.start_hz, &block());
        }
        sink.fail(DeviceError::Io("sweep done".to_owned()));
        Ok(())
    }
}

struct FakeTx(usize);

impl TxStream for FakeTx {
    fn write_channels(
        &mut self,
        channels: &[&[Sample]],
        _timeout: std::time::Duration,
        _end_burst: bool,
    ) -> Result<usize, DeviceError> {
        if channels.len() != self.0 {
            return Err(DeviceError::Unsupported("channel count".to_owned()));
        }
        Ok(channels.iter().map(|channel| channel.len()).sum())
    }

    fn stop(&mut self) -> Result<(), DeviceError> {
        Err(DeviceError::Io("already stopped".to_owned()))
    }
}

fn table() -> &'static PluginV1 {
    let table =
        unsafe { guest::entry(host::host_table(), "fake 1.0", || Box::new(FakeDriver), false) };
    unsafe { table.as_ref() }.expect("the fake plugin starts")
}

fn loaded() -> host::Loaded {
    host::from_table(table()).expect("the fake plugin loads")
}

#[test]
fn the_spec_and_names_cross() {
    let loaded = loaded();
    assert_eq!(loaded.spec, FakeDriver.spec());
    assert_eq!(loaded.driver.id(), FAKE);
    assert_eq!(loaded.plugin, "fake 1.0");
}

#[test]
fn probe_open_apply_round_trip() {
    let driver = loaded().driver;
    assert_eq!(driver.probe(), vec![info("0")]);
    assert_eq!(driver.probe_deep(), vec![info("0")]);
    let mut device = driver.open(&info("0")).expect("opens");
    assert_eq!(device.capabilities().sample_rates, vec![1e6]);
    let settings = DeviceSettings {
        sample_rate: Some(1e6),
        ..DeviceSettings::default()
    };
    device.apply(&settings).expect("applies");
    assert_eq!(device.settings().sample_rate, Some(1e6));
    let refused = DeviceSettings {
        sample_rate: Some(7.0),
        ..DeviceSettings::default()
    };
    assert_eq!(
        device.apply(&refused).map_err(|error| error.to_string()),
        Err("unsupported setting: sample_rate 7".to_owned())
    );
    assert_eq!(device.readouts().expect("readouts").len(), 1);
    assert!(matches!(
        driver.open(&info("missing")),
        Err(DeviceError::NotFound(_))
    ));
}

type Seen = Arc<Mutex<Vec<(u64, Option<LaneEvent>, usize)>>>;

fn recording_sink(seen: &Seen, failed: &Arc<Mutex<Option<String>>>) -> RxSink {
    let log = seen.clone();
    let failed = failed.clone();
    RxSink::with_items(
        move |item: SinkItem<'_>| {
            let entry = match item {
                SinkItem::Samples { samples, index } => (index, None, samples.len()),
                SinkItem::Event(event) => (0, Some(event), 0),
            };
            log.lock().expect("log").push(entry);
        },
        move |error| *failed.lock().expect("failed") = Some(error.to_string()),
    )
}

#[test]
fn samples_and_events_cross_unchanged() {
    let driver = loaded().driver;
    let mut device = driver.open(&info("0")).expect("opens");
    let seen: Seen = Arc::default();
    let failed = Arc::default();
    device
        .rx_start(vec![recording_sink(&seen, &failed)])
        .expect("starts");
    let seen = seen.lock().expect("log").clone();
    assert_eq!(
        seen,
        vec![
            (0, None, 3),
            (0, Some(LaneEvent::Deaf { at: 13, samples: 5 }), 0),
            (
                0,
                Some(LaneEvent::Uncertain {
                    at: 21,
                    error: 2,
                    scope: GapScope::Device,
                    cause: Uncertainty::EstimatedGap,
                }),
                0
            ),
            (
                0,
                Some(LaneEvent::Mark {
                    at: 21,
                    mark: LaneMark::Retuned { in_flight: 4 },
                }),
                0
            ),
            (0, Some(LaneEvent::HardwareTime { at: 21, ns: -9 }), 0),
            (21, None, 3),
        ]
    );
    device.rx_stop();
    assert_eq!(*failed.lock().expect("failed"), None);
}

#[test]
fn a_panic_in_a_stream_thread_fails_the_stream() {
    let driver = loaded().driver;
    let mut device = driver.open(&info("thread")).expect("opens");
    let seen: Seen = Arc::default();
    let failed: Arc<Mutex<Option<String>>> = Arc::default();
    device
        .rx_start(vec![recording_sink(&seen, &failed)])
        .expect("starts");
    for _ in 0..200 {
        if failed.lock().expect("failed").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        failed.lock().expect("failed").as_deref(),
        Some("device I/O error: plugin stream panicked")
    );
}

#[test]
fn a_panic_in_open_is_an_io_error() {
    let driver = loaded().driver;
    let Err(error) = driver.open(&info("panic")) else {
        panic!("a panicking open must fail");
    };
    assert_eq!(
        error.to_string(),
        "device I/O error: plugin panicked: boom while opening"
    );
    assert_eq!(
        driver.probe().len(),
        1,
        "the plugin still answers after a panic"
    );
}

#[test]
fn tx_writes_cross() {
    let driver = loaded().driver;
    let mut device = driver.open(&info("0")).expect("opens");
    let mut tx = device.tx_start_channels(&[0, 1]).expect("tx starts");
    let samples = block();
    assert_eq!(
        tx.write_channels(
            &[&samples, &samples[..2]],
            std::time::Duration::from_millis(5),
            true
        )
        .expect("writes"),
        5
    );
    assert!(
        tx.write_channels(&[&samples], std::time::Duration::ZERO, false)
            .is_err()
    );
    assert_eq!(
        tx.stop().map_err(|error| error.to_string()),
        Err("device I/O error: already stopped".to_owned())
    );
}

#[test]
fn sweep_blocks_cross() {
    let driver = loaded().driver;
    let mut device = driver.open(&info("0")).expect("opens");
    let pushed = Arc::new(Mutex::new(Vec::new()));
    let failed: Arc<Mutex<Option<String>>> = Arc::default();
    let (log, fail) = (pushed.clone(), failed.clone());
    let sink = SweepSink::with_fatal_handler(
        move |center_hz, samples: &[Sample]| {
            log.lock().expect("log").push((center_hz, samples.len()));
        },
        move |error| *fail.lock().expect("failed") = Some(error.to_string()),
    );
    let plan = SweepPlan::new(
        vec![
            SweepBand {
                start_hz: 88e6,
                stop_hz: 108e6,
            },
            SweepBand {
                start_hz: 400e6,
                stop_hz: 410e6,
            },
        ],
        20e6,
    );
    device.sweep_start(&plan, sink).expect("sweeps");
    assert_eq!(*pushed.lock().expect("log"), vec![(88e6, 3), (400e6, 3)]);
    assert_eq!(
        failed.lock().expect("failed").as_deref(),
        Some("device I/O error: sweep done")
    );
}

#[test]
fn an_old_abi_is_refused() {
    let mut old = *table();
    old.abi = 0;
    let Err(error) = host::from_table(Box::leak(Box::new(old))) else {
        panic!("an old table must be refused");
    };
    assert!(matches!(error, host::LoadError::Abi { found: 0 }));
}

#[test]
fn a_host_speaking_another_abi_gets_no_table() {
    let host = host::host_table();
    let other = Box::leak(Box::new(HostV1 {
        abi: 99,
        ..copy_host(host)
    }));
    assert!(unsafe { guest::entry(other, "fake", || Box::new(FakeDriver), false) }.is_null());
}

fn copy_host(host: &HostV1) -> HostV1 {
    HostV1 {
        abi: host.abi,
        size: host.size,
        max_level: host.max_level,
        log: host.log,
        sink_samples: host.sink_samples,
        sink_event: host.sink_event,
        sink_dropped: host.sink_dropped,
        sink_fail: host.sink_fail,
        sink_release: host.sink_release,
        sweep_push: host.sweep_push,
        sweep_fail: host.sweep_fail,
        sweep_release: host.sweep_release,
    }
}

static LOGGED: Mutex<Vec<(u32, String, String)>> = Mutex::new(Vec::new());

extern "C" fn record_log(level: u32, target: Str, message: Str) {
    LOGGED.lock().expect("logged").push((
        level,
        unsafe { target.text() }.into_owned(),
        unsafe { message.text() }.into_owned(),
    ));
}

#[test]
fn logs_reach_the_host() {
    static HOST: OnceLock<HostV1> = OnceLock::new();
    let host = HOST.get_or_init(|| HostV1 {
        max_level: 3,
        log: record_log,
        ..copy_host(host::host_table())
    });
    tracing::subscriber::with_default(guest::Forwarder::new(host), || {
        tracing::warn!(target: "fake_radio", answer = 42, "hello");
        tracing::debug!(target: "fake_radio", "too quiet");
    });
    let from_radio: Vec<_> = LOGGED
        .lock()
        .expect("logged")
        .iter()
        .filter(|(_, target, _)| target == "fake_radio")
        .cloned()
        .collect();
    assert_eq!(
        from_radio,
        vec![(2, "fake_radio".to_owned(), "hello answer=42".to_owned())]
    );
}

fn crossing_schemas() -> String {
    use utoipa::ToSchema;
    fn add<T: ToSchema>(
        all: &mut Vec<(
            String,
            utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
        )>,
    ) {
        all.push((T::name().into_owned(), T::schema()));
        T::schemas(all);
    }
    let mut all = Vec::new();
    add::<sdrmm_wire::BackendSpec>(&mut all);
    add::<sdrmm_wire::DeviceInfo>(&mut all);
    add::<sdrmm_wire::Capabilities>(&mut all);
    add::<sdrmm_wire::DeviceSettings>(&mut all);
    add::<sdrmm_wire::Readout>(&mut all);
    add::<sdrmm_wire::AgcGain>(&mut all);
    add::<sdrmm_wire::DoctorCheck>(&mut all);
    add::<sdrmm_wire::Duplex>(&mut all);
    add::<sdrmm_wire::Direction>(&mut all);
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all.dedup_by(|a, b| a.0 == b.0);
    serde_json::to_string(&all).expect("schemas serialize")
}

/// The wire types that cross the plugin boundary as JSON, pinned. When this fails, a crossing
/// type changed: bump `DEVICE_ABI_VERSION` and then the pin.
#[test]
fn device_abi_types_are_pinned() {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(crossing_schemas());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        (crate::abi::DEVICE_ABI_VERSION, hex.as_str()),
        (
            1,
            "5f9ba9b02dcdc3a9dc216eefd85a3c9122954f6c367fb18e89f1304eb3cae42f"
        ),
        "a wire type crossing the device plugin boundary changed: bump DEVICE_ABI_VERSION, then this pin"
    );
}

struct PanickyDriver;

impl DeviceDriver for PanickyDriver {
    fn id(&self) -> DeviceBackend {
        DeviceBackend::new("panicky")
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        panic!("probe boom");
    }

    fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        Err(DeviceError::NotFound("panicky".to_owned()))
    }
}

#[test]
fn a_failing_probe_shows_in_the_doctor() {
    let table = unsafe {
        guest::entry(
            host::host_table(),
            "panicky 1.0",
            || Box::new(PanickyDriver),
            false,
        )
    };
    let loaded = host::from_table(unsafe { table.as_ref() }.expect("starts")).expect("loads");
    assert!(loaded.driver.probe().is_empty());
    let check = loaded
        .driver
        .health()
        .expect("a failing plugin reports itself");
    assert_eq!(check.status, sdrmm_wire::CheckStatus::Fail);
    assert!(check.detail.contains("probe boom"), "{}", check.detail);
}

#[test]
fn a_failed_apply_reports_its_own_error() {
    let driver = loaded().driver;
    let mut device = driver.open(&info("fragile")).expect("opens");
    assert_eq!(
        device
            .apply(&DeviceSettings::default())
            .map_err(|error| error.to_string()),
        Err("unsupported setting: fragile".to_owned())
    );
}

#[test]
fn a_plugin_that_aborts_on_panic_is_refused() {
    let mut aborting = *table();
    aborting.panic_unwind = false;
    let Err(error) = host::from_table(Box::leak(Box::new(aborting))) else {
        panic!("a plugin that cannot catch panics must be refused");
    };
    assert!(matches!(error, host::LoadError::PanicAbort));
}

#[test]
fn a_host_on_another_abi_hears_why() {
    static HOST: OnceLock<HostV1> = OnceLock::new();
    let host = HOST.get_or_init(|| HostV1 {
        abi: 99,
        log: record_log,
        ..copy_host(host::host_table())
    });
    assert!(unsafe { guest::entry(host, "fake 1.0", || Box::new(FakeDriver), false) }.is_null());
    assert!(
        LOGGED
            .lock()
            .expect("logged")
            .iter()
            .any(|(level, _, message)| *level == 1 && message.contains("ABI 99")),
        "{:?}",
        LOGGED.lock().expect("logged")
    );
}

static DROPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct DropWatch;

impl Drop for DropWatch {
    fn drop(&mut self) {
        DROPPED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl DeviceDriver for DropWatch {
    fn id(&self) -> DeviceBackend {
        DeviceBackend::new("dropwatch")
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        Vec::new()
    }

    fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        Err(DeviceError::NotFound("dropwatch".to_owned()))
    }
}

#[test]
fn dropping_a_driver_releases_the_plugin_side() {
    let table =
        unsafe { guest::entry(host::host_table(), "dropwatch", || Box::new(DropWatch), false) };
    let loaded = host::from_table(unsafe { table.as_ref() }.expect("starts")).expect("loads");
    drop(loaded);
    assert!(DROPPED.load(std::sync::atomic::Ordering::SeqCst));
}
