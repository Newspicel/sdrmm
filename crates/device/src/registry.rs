use std::collections::HashMap;

use sdrmm_wire::{DeviceBackend, DeviceInfo, DoctorCheck};

use crate::{DeviceDriver, DeviceError, SdrDevice};

#[derive(Default)]
pub struct DeviceRegistry {
    drivers: Vec<(u8, Box<dyn DeviceDriver>)>,
}

impl DeviceRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, priority: u8, driver: Box<dyn DeviceDriver>) {
        self.drivers.push((priority, driver));
    }

    #[must_use]
    pub fn driver_ids(&self) -> Vec<(u8, DeviceBackend)> {
        self.drivers.iter().map(|(p, d)| (*p, d.id())).collect()
    }

    #[must_use]
    pub fn health(&self) -> Vec<DoctorCheck> {
        self.drivers
            .iter()
            .filter_map(|(_, driver)| driver.health())
            .collect()
    }

    #[must_use]
    pub fn probe_all(&self) -> Vec<DeviceInfo> {
        self.collect(|driver| driver.probe())
    }

    /// Searches every driver as far as it reaches, network radios included. Costs seconds.
    #[must_use]
    pub fn probe_all_deep(&self) -> Vec<DeviceInfo> {
        self.collect(|driver| driver.probe_deep())
    }

    /// What each driver finds and how long it takes, for reporting where a slow search goes.
    #[must_use]
    pub fn probe_timings(&self) -> Vec<(DeviceBackend, usize, std::time::Duration)> {
        self.drivers
            .iter()
            .map(|(_, driver)| {
                let started = std::time::Instant::now();
                let found = driver.probe_deep().len();
                (driver.id(), found, started.elapsed())
            })
            .collect()
    }

    fn collect(&self, probe: impl Fn(&dyn DeviceDriver) -> Vec<DeviceInfo>) -> Vec<DeviceInfo> {
        let mut by_serial: HashMap<String, (u8, DeviceBackend, Vec<DeviceInfo>)> = HashMap::new();
        let mut serialless: Vec<DeviceInfo> = Vec::new();

        for (priority, driver) in &self.drivers {
            for info in probe(&**driver) {
                match &info.serial {
                    Some(serial) => {
                        let entry = by_serial.entry(serial.clone());
                        match entry {
                            std::collections::hash_map::Entry::Occupied(mut e) => {
                                let (winning_priority, winning_driver, variants) = e.get_mut();
                                if *priority > *winning_priority {
                                    *winning_priority = *priority;
                                    *winning_driver = driver.id();
                                    variants.clear();
                                    variants.push(info);
                                } else if *priority == *winning_priority
                                    && driver.id() == *winning_driver
                                    && !variants.iter().any(|known| known.key == info.key)
                                {
                                    variants.push(info);
                                }
                            }
                            std::collections::hash_map::Entry::Vacant(e) => {
                                e.insert((*priority, driver.id(), vec![info]));
                            }
                        }
                    }
                    None => serialless.push(info),
                }
            }
        }

        let mut out: Vec<DeviceInfo> = by_serial
            .into_values()
            .flat_map(|(_, _, variants)| variants)
            .collect();
        out.extend(serialless);
        out.sort_by_key(DeviceInfo::id);
        out
    }

    pub fn open(&self, device_id: &str) -> Result<(DeviceInfo, Box<dyn SdrDevice>), DeviceError> {
        let (driver, info) = self
            .find(device_id)
            .ok_or_else(|| DeviceError::NotFound(device_id.to_string()))?;
        let device = driver.open(&info)?;
        Ok((info, device))
    }

    #[must_use]
    pub fn resolve(&self, device_id: &str) -> Option<DeviceInfo> {
        let (driver_id, key) = split_device_id(device_id)?;
        self.drivers
            .iter()
            .filter(|(_, driver)| driver.id() == driver_id)
            .find_map(|(_, driver)| driver.resolve(key))
    }

    pub fn write_serial(
        &self,
        device_id: &str,
        serial: Option<&str>,
    ) -> Result<String, DeviceError> {
        let not_found = || DeviceError::NotFound(device_id.to_string());
        let (driver_id, key) = split_device_id(device_id).ok_or_else(not_found)?;
        self.drivers
            .iter()
            .find(|(_, driver)| driver.id() == driver_id)
            .ok_or_else(not_found)?
            .1
            .write_serial(key, serial)
    }

    fn find(&self, device_id: &str) -> Option<(&dyn DeviceDriver, DeviceInfo)> {
        let (driver_id, key) = split_device_id(device_id)?;
        let matching = || {
            self.drivers
                .iter()
                .filter(move |(_, driver)| driver.id() == driver_id)
                .map(|(_, driver)| &**driver)
        };
        matching()
            .find_map(|driver| {
                driver
                    .probe_deep()
                    .into_iter()
                    .find(|d| d.key == key)
                    .map(|info| (driver, info))
            })
            .or_else(|| matching().find_map(|driver| Some((driver, driver.resolve(key)?))))
    }
}

fn split_device_id(device_id: &str) -> Option<(DeviceBackend, &str)> {
    let (driver, key) = device_id.split_once(':')?;
    Some((DeviceBackend::parse(driver)?, key))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use sdrmm_wire::{Capabilities, DcArtifact, DeviceSettings, Duplex, StreamScope};

    use super::*;
    use crate::{RxSink, lock};

    struct Fake {
        id: DeviceBackend,
        probed: Mutex<Vec<DeviceInfo>>,
        adopts: bool,
    }

    impl Fake {
        fn new(id: DeviceBackend, keys: &[&str]) -> Self {
            Self {
                id,
                probed: Mutex::new(keys.iter().map(|key| info(id, key)).collect()),
                adopts: false,
            }
        }

        fn adopting(id: DeviceBackend) -> Self {
            Self {
                id,
                probed: Mutex::new(Vec::new()),
                adopts: true,
            }
        }
    }

    fn info(driver: DeviceBackend, key: &str) -> DeviceInfo {
        DeviceInfo {
            driver,
            key: key.to_string(),
            label: format!("{driver} {key}"),
            serial: None,
            profile: None,
        }
    }

    impl DeviceDriver for Fake {
        fn id(&self) -> DeviceBackend {
            self.id
        }

        fn probe(&self) -> Vec<DeviceInfo> {
            lock(&self.probed).clone()
        }

        fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
            Ok(Box::new(FakeDevice))
        }

        fn resolve(&self, key: &str) -> Option<DeviceInfo> {
            if !self.adopts {
                return None;
            }
            let canonical = info(self.id, &key.to_ascii_lowercase());
            let mut probed = lock(&self.probed);
            if !probed.iter().any(|d| d.key == canonical.key) {
                probed.push(canonical.clone());
            }
            Some(canonical)
        }
    }

    struct FakeDevice;

    impl SdrDevice for FakeDevice {
        fn capabilities(&self) -> &Capabilities {
            static CAPS: std::sync::OnceLock<Capabilities> = std::sync::OnceLock::new();
            CAPS.get_or_init(|| Capabilities {
                freq_ranges: Vec::new(),
                sample_rates: Vec::new(),
                sample_rate_ranges: Vec::new(),
                gains: Vec::new(),
                antennas: Vec::new(),
                bandwidths: Vec::new(),
                bandwidth_ranges: Vec::new(),
                bandwidth_auto: false,
                bias_tee: false,
                agc: sdrmm_wire::Agc::None,
                extra: Vec::new(),
                ppm: false,
                duplex: Duplex::RxOnly,
                rx_streams: 1,
                tx_streams: 0,
                per_stream: StreamScope::default(),
                directional: None,
                dc_artifact: DcArtifact::Operator,
                hardware_sweep: false,
                coherence: sdrmm_wire::Coherence::None,
                noise_source: sdrmm_wire::NoiseSource::None,
                retune_keeps_phase: false,
                rx_inputs: Vec::new(),
                clean_fraction: None,
                bank: None,
            })
        }

        fn settings(&self) -> &DeviceSettings {
            static SETTINGS: std::sync::OnceLock<DeviceSettings> = std::sync::OnceLock::new();
            SETTINGS.get_or_init(DeviceSettings::default)
        }

        fn apply(&mut self, _settings: &DeviceSettings) -> Result<(), DeviceError> {
            Ok(())
        }

        fn rx_start(&mut self, _sinks: Vec<RxSink>) -> Result<(), DeviceError> {
            Ok(())
        }

        fn rx_stop(&mut self) {}
    }

    fn registry(drivers: impl IntoIterator<Item = Fake>) -> DeviceRegistry {
        let mut registry = DeviceRegistry::new();
        for driver in drivers {
            registry.register(10, Box::new(driver));
        }
        registry
    }

    fn serial_info(driver: DeviceBackend, key: &str, serial: &str) -> DeviceInfo {
        DeviceInfo {
            serial: Some(serial.to_string()),
            ..info(driver, key)
        }
    }

    #[test]
    fn a_serial_goes_to_the_named_driver_which_may_refuse_it() {
        let registry = registry([Fake::new(DeviceBackend::Rtlsdr, &["one"])]);
        assert!(matches!(
            registry.write_serial("rtlsdr:one", None),
            Err(DeviceError::Unsupported(text))
                if text == "rtlsdr radios keep their factory serial"
        ));
        for missing in ["hackrf:one", "no-colon", "nosuch:one"] {
            assert!(matches!(
                registry.write_serial(missing, Some("7")),
                Err(DeviceError::NotFound(_))
            ));
        }
    }

    #[test]
    fn health_gathers_what_the_drivers_report_and_skips_the_quiet_ones() {
        struct Ailing;

        impl DeviceDriver for Ailing {
            fn id(&self) -> DeviceBackend {
                DeviceBackend::Sdrplay
            }

            fn probe(&self) -> Vec<DeviceInfo> {
                Vec::new()
            }

            fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
                Err(DeviceError::NotFound("ailing".to_string()))
            }

            fn health(&self) -> Option<DoctorCheck> {
                Some(DoctorCheck {
                    id: "ailing.library".to_string(),
                    name: "Ailing SDK".to_string(),
                    status: sdrmm_wire::CheckStatus::Warn,
                    detail: "not installed".to_string(),
                    hint: None,
                })
            }
        }

        let mut registry = DeviceRegistry::new();
        registry.register(1, Box::new(Fake::new(DeviceBackend::Rtlsdr, &["one"])));
        registry.register(2, Box::new(Ailing));
        let reported: Vec<String> = registry
            .health()
            .into_iter()
            .map(|check| check.id)
            .collect();
        assert_eq!(reported, ["ailing.library"]);
    }

    #[test]
    fn open_finds_a_probed_device() {
        let registry = registry([Fake::new(DeviceBackend::Rtlsdr, &["one", "two"])]);
        let (info, _) = registry.open("rtlsdr:two").expect("probed device");
        assert_eq!(info.id(), "rtlsdr:two");
    }

    #[test]
    fn a_key_no_probe_reports_is_not_found_unless_the_driver_adopts_it() {
        let registry = registry([Fake::new(DeviceBackend::Rtlsdr, &["one"])]);
        assert!(matches!(
            registry.open("rtlsdr:missing"),
            Err(DeviceError::NotFound(_))
        ));
        assert!(registry.resolve("rtlsdr:missing").is_none());
    }

    #[test]
    fn an_adopted_key_opens_and_then_appears_in_the_probe() {
        let registry = registry([Fake::adopting(DeviceBackend::Rtltcp)]);
        assert!(registry.probe_all().is_empty());
        let (info, _) = registry.open("rtltcp:Host:1234").expect("adopted endpoint");
        assert_eq!(
            info.id(),
            "rtltcp:host:1234",
            "the canonical key comes back"
        );
        assert_eq!(
            registry
                .probe_all()
                .iter()
                .map(DeviceInfo::id)
                .collect::<Vec<_>>(),
            vec!["rtltcp:host:1234".to_string()]
        );
        registry.open("rtltcp:host:1234").expect("now probed");
    }

    #[test]
    fn resolve_adopts_without_opening() {
        let registry = registry([Fake::adopting(DeviceBackend::Rtltcp)]);
        let info = registry.resolve("rtltcp:HOST:1234").expect("adopted");
        assert_eq!(info.id(), "rtltcp:host:1234");
        assert_eq!(registry.probe_all().len(), 1);
        assert!(registry.resolve("hackrf:host:1234").is_none());
        assert!(registry.resolve("no-colon").is_none());
        assert!(registry.resolve("nosuch:host").is_none());
    }

    #[test]
    fn a_probed_device_wins_over_an_adopting_driver_with_the_same_id() {
        let registry = registry([
            Fake::adopting(DeviceBackend::Rtlsdr),
            Fake::new(DeviceBackend::Rtlsdr, &["real"]),
        ]);
        let (info, _) = registry.open("rtlsdr:real").expect("the probed one");
        assert_eq!(info.label, "rtlsdr real");
    }

    #[test]
    fn one_driver_can_expose_multiple_addresses_for_a_serial() {
        let variants = Fake {
            id: DeviceBackend::Soapy,
            probed: Mutex::new(vec![
                serial_info(DeviceBackend::Soapy, "123456@ST", "123456"),
                serial_info(DeviceBackend::Soapy, "123456@DT", "123456"),
            ]),
            adopts: false,
        };
        let registry = registry([variants]);

        assert_eq!(
            registry
                .probe_all()
                .iter()
                .map(DeviceInfo::id)
                .collect::<Vec<_>>(),
            vec!["soapy:123456@DT", "soapy:123456@ST"]
        );
    }

    #[test]
    fn a_higher_priority_backend_still_wins_a_shared_serial() {
        let soapy = Fake {
            id: DeviceBackend::Soapy,
            probed: Mutex::new(vec![
                serial_info(DeviceBackend::Soapy, "123456@ST", "123456"),
                serial_info(DeviceBackend::Soapy, "123456@DT", "123456"),
            ]),
            adopts: false,
        };
        let native = Fake {
            id: DeviceBackend::Hackrf,
            probed: Mutex::new(vec![serial_info(DeviceBackend::Hackrf, "123456", "123456")]),
            adopts: false,
        };
        let mut registry = DeviceRegistry::new();
        registry.register(10, Box::new(soapy));
        registry.register(20, Box::new(native));

        assert_eq!(
            registry
                .probe_all()
                .iter()
                .map(DeviceInfo::id)
                .collect::<Vec<_>>(),
            vec!["hackrf:123456"]
        );
    }
}
