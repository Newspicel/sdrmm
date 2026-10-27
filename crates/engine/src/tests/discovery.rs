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
        DeviceBackend::Rtlsdr
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
    assert!(ids.contains(&DeviceBackend::Virtual), "{ids:?}");
    #[cfg(feature = "rtlsdr")]
    assert!(ids.contains(&DeviceBackend::Rtlsdr), "{ids:?}");
    #[cfg(feature = "hackrf")]
    assert!(ids.contains(&DeviceBackend::Hackrf), "{ids:?}");
    #[cfg(feature = "ad936x")]
    assert!(ids.contains(&DeviceBackend::Ad936x), "{ids:?}");
    #[cfg(all(
        feature = "funcube",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    assert!(ids.contains(&DeviceBackend::Funcube), "{ids:?}");
    #[cfg(feature = "soapy")]
    assert!(ids.contains(&DeviceBackend::Soapy), "{ids:?}");
}

#[test]
fn soapy_hides_exactly_the_radios_this_build_drives_over_usb() {
    let handled = soapy_handled_natively();
    assert_eq!(handled.contains(&"rtlsdr"), cfg!(feature = "rtlsdr"));
    assert_eq!(handled.contains(&"hackrf"), cfg!(feature = "hackrf"));
    assert_eq!(
        handled.contains(&"fcdpp"),
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
        handled.contains(&"plutosdr"),
        cfg!(feature = "ad936x"),
        "the AD936x boards Soapy calls plutosdr are driven over their own iiod connection"
    );
    assert!(
        !handled.contains(&"sdrplay"),
        "the SDRplay driver reports unique serials and settles its duplicate by priority instead"
    );
}

#[test]
fn every_native_backend_is_registered_and_names_itself_once() {
    let registered: Vec<DeviceBackend> = builtin_registry(None)
        .driver_ids()
        .into_iter()
        .map(|(_, id)| id)
        .collect();
    for backend in native_backends() {
        assert!(registered.contains(&backend), "{backend} is not registered");
        assert!(backend.is_radio(), "{backend} is not a radio");
    }
    for backend in registered {
        if backend.soapy_module().is_some() && backend != DeviceBackend::Soapy {
            assert!(
                native_backends().contains(&backend),
                "{backend} shadows a Soapy module but is not listed as native"
            );
        }
    }
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
