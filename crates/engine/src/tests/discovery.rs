use super::*;
use sdrmm_wire::DeviceBackend;

#[tokio::test]
async fn probes_virtual_device() {
    let engine = virtual_engine();
    assert!(
        engine
            .probe_devices()
            .iter()
            .any(|d| d.id() == "virtual:band")
    );
}

#[tokio::test]
async fn one_radio_opens_into_one_device_set() {
    let engine = virtual_engine();
    let ds = engine.create_device_set("virtual:band").unwrap();
    let refused = engine.create_device_set("virtual:band").unwrap_err();
    assert!(
        matches!(&refused, EngineError::DeviceAlreadyOpen(device, held)
            if device == "virtual:band" && *held == ds),
        "expected a reopen refusal, got {refused}"
    );
    assert!(refused.is_conflict());
    assert!(!refused.is_bad_request());
    assert_eq!(engine.snapshot().device_sets.len(), 1);

    engine.remove_device_set(ds).unwrap();
    engine.create_device_set("virtual:band").unwrap();
}

struct GatedDriver {
    opens: Arc<AtomicUsize>,
    entered: Mutex<std::sync::mpsc::Sender<()>>,
    released: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl DeviceDriver for GatedDriver {
    fn id(&self) -> DeviceBackend {
        DeviceBackend::RTLSDR
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        vec![mock_info("gated", None)]
    }

    fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let _ = lock(&self.entered).send(());
        let _ = lock(&self.released).recv_timeout(Duration::from_secs(5));
        Ok(Box::new(SilentDevice {
            capabilities: empty_capabilities(),
            settings: mock_settings(),
        }))
    }
}

#[test]
fn a_radio_opened_from_two_places_at_once_is_only_dialled_once() {
    let (entered, waiting) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let opens = Arc::new(AtomicUsize::new(0));
    let mut registry = DeviceRegistry::new();
    registry.register(
        0,
        Box::new(GatedDriver {
            opens: opens.clone(),
            entered: Mutex::new(entered),
            released: Mutex::new(released),
        }),
    );
    let engine = Engine::with_registry(registry, None);

    let opening = {
        let engine = engine.clone();
        std::thread::spawn(move || engine.create_device_set("rtlsdr:gated"))
    };
    waiting
        .recv_timeout(Duration::from_secs(5))
        .expect("the driver is inside open");
    let refused = engine
        .create_device_set("rtlsdr:gated")
        .expect_err("one radio, one open");
    assert!(
        matches!(&refused, EngineError::DeviceOpening(device) if device == "rtlsdr:gated"),
        "expected a refusal while it is being opened, got {refused}"
    );
    assert!(refused.is_conflict());

    let _ = release.send(());
    let ds = opening.join().expect("the open finishes").expect("opens");
    assert_eq!(
        opens.load(Ordering::SeqCst),
        1,
        "the radio was dialled once"
    );
    assert_eq!(engine.snapshot().device_sets.len(), 1);

    engine.remove_device_set(ds).unwrap();
    engine.shutdown();
}

#[tokio::test]
async fn spectrum_flows_with_a_visible_tone() {
    let engine = virtual_engine();
    let ds = engine.create_device_set("virtual:band").unwrap();
    let mut rx = engine.subscribe_spectrum(ds, 0).unwrap();

    let snap = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("spectrum within timeout")
        .expect("snapshot");
    assert_eq!(snap.db.len(), 4096);

    let mut sorted = snap.db.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[sorted.len() / 2];
    let peak = *sorted.last().unwrap();
    assert!(
        peak - median > 20.0,
        "expected tone peak above floor (peak {peak}, median {median})"
    );

    engine.remove_device_set(ds).unwrap();
    assert!(engine.snapshot().device_sets.is_empty());
}

#[tokio::test]
async fn a_wider_rate_resizes_the_spectrum_to_keep_its_bins_narrow() {
    const WIDE_RATE: f64 = 10_000_000.0;
    let engine = virtual_engine();
    let ds = engine.create_device_set("virtual:band").unwrap();
    let mut rx = engine.subscribe_spectrum(ds, 0).unwrap();
    engine
        .patch_device(
            ds,
            DeviceSettings {
                sample_rate: Some(WIDE_RATE),
                ..DeviceSettings::default()
            },
        )
        .unwrap();

    let wide = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(snap) if f64::from(snap.span_hz) == WIDE_RATE => break snap,
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(error) => panic!("spectrum closed: {error}"),
            }
        }
    })
    .await
    .expect("a spectrum at the wider rate");
    assert_eq!(wide.db.len(), 16_384);

    engine.remove_device_set(ds).unwrap();
}

#[test]
fn the_builtin_registry_carries_every_backend_this_build_compiled_in() {
    let ids: Vec<DeviceBackend> = builtin_registry(None)
        .driver_ids()
        .into_iter()
        .map(|(_, id)| id)
        .collect();
    assert!(ids.contains(&DeviceBackend::VIRTUAL), "{ids:?}");
    #[cfg(feature = "rtlsdr")]
    assert!(ids.contains(&DeviceBackend::RTLSDR), "{ids:?}");
    #[cfg(feature = "hackrf")]
    assert!(ids.contains(&DeviceBackend::HACKRF), "{ids:?}");
    #[cfg(feature = "ad936x")]
    assert!(ids.contains(&DeviceBackend::AD936X), "{ids:?}");
    #[cfg(all(
        feature = "funcube",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    assert!(ids.contains(&DeviceBackend::FUNCUBE), "{ids:?}");
    #[cfg(feature = "soapy")]
    assert!(ids.contains(&DeviceBackend::SOAPY), "{ids:?}");
}

#[test]
fn soapy_hides_exactly_the_radios_this_build_drives_over_usb() {
    let handled = soapy_handled_natively(&builtin_registry(None));
    let handles = |module: &str| handled.iter().any(|known| known == module);
    assert_eq!(handles("rtlsdr"), cfg!(feature = "rtlsdr"));
    assert_eq!(handles("hackrf"), cfg!(feature = "hackrf"));
    assert_eq!(
        handles("fcdpp"),
        cfg!(all(
            feature = "funcube",
            any(
                target_os = "macos",
                target_os = "linux",
                target_os = "windows"
            )
        ))
    );
    assert_eq!(
        handles("plutosdr"),
        cfg!(feature = "ad936x"),
        "the AD936x boards Soapy calls plutosdr are driven over their own iiod connection"
    );
    assert!(
        !handles("sdrplay"),
        "the SDRplay driver reports unique serials and settles its duplicate by priority instead"
    );
}

#[test]
fn soapy_hides_modules_a_plugin_claims() {
    struct Claiming;

    impl DeviceDriver for Claiming {
        fn id(&self) -> DeviceBackend {
            DeviceBackend::new("claiming")
        }

        fn spec(&self) -> sdrmm_wire::BackendSpec {
            sdrmm_wire::BackendSpec::bare(self.id()).soapy_module("lime")
        }

        fn probe(&self) -> Vec<DeviceInfo> {
            Vec::new()
        }

        fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
            Err(DeviceError::NotFound("claiming".to_string()))
        }
    }

    let registry = crate::registry(
        None,
        1.0,
        crate::DevicePlugins::default().with_driver("claiming", Box::new(Claiming)),
    );
    assert!(soapy_handled_natively(&registry).contains(&"lime".to_string()));
    assert!(
        registry
            .driver_ids()
            .contains(&(crate::PLUGIN_PRIORITY, DeviceBackend::new("claiming")))
    );
}

struct Named(DeviceBackend);

impl DeviceDriver for Named {
    fn id(&self) -> DeviceBackend {
        self.0
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        Vec::new()
    }

    fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        Err(DeviceError::NotFound(self.0.to_string()))
    }
}

fn plugin_check(registry: &DeviceRegistry, file: &str) -> sdrmm_wire::DoctorCheck {
    registry
        .health()
        .into_iter()
        .find(|check| check.id == format!("plugins.device.{file}"))
        .unwrap_or_else(|| panic!("no doctor line for {file}"))
}

#[test]
fn a_duplicate_backend_is_refused() {
    let plugins = crate::DevicePlugins::default()
        .with_driver("first", Box::new(Named(DeviceBackend::new("twin"))))
        .with_driver("second", Box::new(Named(DeviceBackend::new("twin"))))
        .with_driver("shadow", Box::new(Named(DeviceBackend::VIRTUAL)));
    let registry = crate::registry(None, 1.0, plugins);
    let twins = registry
        .driver_ids()
        .into_iter()
        .filter(|(_, id)| *id == DeviceBackend::new("twin"))
        .count();
    assert_eq!(twins, 1);
    assert_eq!(
        plugin_check(&registry, "first").status,
        sdrmm_wire::CheckStatus::Ok
    );
    assert_eq!(
        plugin_check(&registry, "second").status,
        sdrmm_wire::CheckStatus::Fail
    );
    assert_eq!(
        plugin_check(&registry, "shadow").status,
        sdrmm_wire::CheckStatus::Fail
    );
}

#[test]
fn a_broken_plugin_file_is_reported_and_the_rest_still_load() {
    let dir = std::env::temp_dir().join(format!("sdrmm-device-plugins-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("plugin dir");
    let file = format!("broken.{}", std::env::consts::DLL_EXTENSION);
    std::fs::write(dir.join(&file), b"not a library").expect("broken plugin");
    std::fs::write(dir.join("notes.txt"), b"ignored").expect("other file");
    let plugins = crate::DevicePlugins::load(&dir)
        .with_driver("fine", Box::new(Named(DeviceBackend::new("fine"))));
    let registry = crate::registry(None, 1.0, plugins);
    std::fs::remove_dir_all(&dir).expect("clean up");
    let broken = plugin_check(&registry, &file);
    assert_eq!(broken.status, sdrmm_wire::CheckStatus::Fail);
    assert!(
        broken.detail.starts_with("not a loadable library"),
        "{}",
        broken.detail
    );
    assert!(registry.spec(DeviceBackend::new("fine")).is_some());
    assert!(
        !registry
            .health()
            .iter()
            .any(|check| check.id.ends_with("notes.txt")),
        "only libraries are plugins"
    );
}

#[test]
fn every_builtin_radio_declares_how_it_is_reached() {
    let registry = builtin_registry(None);
    for spec in registry.specs() {
        assert!(!spec.label.is_empty(), "{} has no label", spec.backend);
        assert_eq!(
            spec.addressable(),
            spec.reach == sdrmm_wire::BackendReach::Network,
            "{}",
            spec.backend
        );
    }
    let virtual_spec = registry
        .spec(DeviceBackend::VIRTUAL)
        .expect("virtual is built in");
    assert!(!virtual_spec.is_radio());
    let recording = registry
        .spec(DeviceBackend::RECORDING)
        .expect("recording is built in");
    assert!(recording.node_owned());
}

#[tokio::test]
async fn a_radio_the_network_search_adopts_announces_itself() {
    let mut registry = DeviceRegistry::new();
    registry.register(50, Box::new(AdoptingDriver::default()));
    let engine = Engine::with_registry(registry, None);
    let mut events = engine.subscribe_events();

    assert!(engine.probe_devices().is_empty());

    let announced = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(ServerEvent::StateChanged {
                scope: StateScope::Devices,
            }) = events.recv().await
            {
                break;
            }
        }
    })
    .await;
    assert!(announced.is_ok(), "the adopted radio must announce itself");
    assert!(
        engine
            .probe_devices()
            .iter()
            .any(|device| device.id() == "rtlsdr:adopted")
    );
}
