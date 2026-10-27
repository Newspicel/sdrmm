use std::sync::Arc;

use sdrmm_device::{
    Capture, CaptureConfig, DeviceDriver, DeviceError, RxSink, SdrDevice, lock,
    net::{Adopted, Endpoint},
    single_rx_sink,
};
use sdrmm_wire::{Capabilities, DeviceBackend, DeviceInfo, DeviceSettings};

pub use crate::ethernet::EspSdrNetDevice;
use crate::{
    ethernet::HTTP_PORT,
    link::{Link, Stop, SystemLink},
    radio::{Converter, EspRadio},
    session::Session,
};

mod caps;
mod ethernet;
#[cfg(test)]
mod hardware;
mod link;
mod proto;
mod radio;
mod session;

const DRIVER_ID: DeviceBackend = DeviceBackend::Espsdr;
const OPEN_BAUD: u32 = 115_200;
const ESPRESSIF_VID: u16 = 0x303A;
const BRIDGES: [(u16, u16); 4] = [
    (0x10C4, 0xEA60),
    (0x1A86, 0x7523),
    (0x1A86, 0x55D3),
    (0x1A86, 0x55D4),
];

#[derive(Debug, Default)]
pub struct EspSdrDriver {
    adopted: Adopted,
}

impl EspSdrDriver {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

fn is_serial(key: &str) -> bool {
    let windows = key
        .get(..3)
        .is_some_and(|head| head.eq_ignore_ascii_case("COM"))
        && key.len() > 3
        && key[3..].bytes().all(|b| b.is_ascii_digit());
    key.starts_with('/') || key.starts_with('\\') || windows
}

fn net_info(endpoint: &Endpoint) -> DeviceInfo {
    DeviceInfo {
        driver: DRIVER_ID,
        key: endpoint.to_string(),
        label: format!("ESP-SDR {endpoint}"),
        serial: None,
        profile: None,
    }
}

fn device_info(port: &str) -> DeviceInfo {
    let short = port.rsplit('/').next().unwrap_or(port);
    DeviceInfo {
        driver: DRIVER_ID,
        key: port.to_string(),
        label: format!("ESP-SDR {}", short.trim_start_matches("cu.")),
        serial: None,
        profile: None,
    }
}

fn may_be_esp(info: &sdrmm_serial::SerialPortInfo) -> bool {
    let sdrmm_serial::SerialPortType::UsbPort(usb) = &info.port_type else {
        return false;
    };
    usb.vid == ESPRESSIF_VID || BRIDGES.contains(&(usb.vid, usb.pid))
}

fn system_ports() -> Vec<sdrmm_serial::SerialPortInfo> {
    sdrmm_serial::ports()
        .inspect_err(|e| tracing::debug!("serial ports: {e}"))
        .unwrap_or_default()
}

impl DeviceDriver for EspSdrDriver {
    fn id(&self) -> DeviceBackend {
        DRIVER_ID
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        system_ports()
            .iter()
            .filter(|info| may_be_esp(info))
            .map(|info| device_info(&info.port_name))
            .chain(self.adopted.list().iter().map(net_info))
            .collect()
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        if !is_serial(&info.key) {
            let endpoint = Endpoint::parse(&info.key, HTTP_PORT)?;
            return Ok(Box::new(EspSdrNetDevice::open(endpoint)?));
        }
        let link = SystemLink::open(&info.key, OPEN_BAUD)?;
        Ok(Box::new(EspSdrDevice::open(&info.key, Box::new(link))?))
    }

    fn resolve(&self, key: &str) -> Option<DeviceInfo> {
        if is_serial(key) {
            return system_ports()
                .iter()
                .any(|info| info.port_name == key)
                .then(|| device_info(key));
        }
        let endpoint = Endpoint::parse(key, HTTP_PORT)
            .inspect_err(|e| tracing::warn!("espsdr endpoint: {e}"))
            .ok()?;
        if !self.adopted.adopt(endpoint.clone()) {
            tracing::warn!(%endpoint, "too many espsdr endpoints; refusing to adopt another");
            return None;
        }
        Some(net_info(&endpoint))
    }
}

pub struct EspSdrDevice {
    radio: Arc<EspRadio>,
    capabilities: Capabilities,
    settings: DeviceSettings,
    capture: Capture<EspRadio>,
}

impl EspSdrDevice {
    fn open(port: &str, link: Box<dyn Link>) -> Result<Self, DeviceError> {
        let (session, profile) = Session::connect(link, Stop::default())?;
        tracing::info!(
            port,
            chip = profile.chip,
            baud = session.baud(),
            "opened ESP-SDR"
        );
        let radio = EspRadio::new(profile, session);
        let settings = radio.0.profile.wire(&lock(&radio.0.desired));
        Ok(Self {
            capabilities: radio.0.profile.capabilities(),
            settings,
            radio: Arc::new(radio),
            capture: Capture::new(),
        })
    }
}

impl SdrDevice for EspSdrDevice {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        let inner = &self.radio.0;
        let mut desired = lock(&inner.desired);
        let next = inner
            .profile
            .validate(settings, &self.capabilities, *desired)?;
        *desired = next;
        self.settings = inner.profile.wire(&next);
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        self.capture.start(
            self.radio.clone(),
            Converter::new(self.radio.0.profile.max_burst().unwrap_or(0)),
            single_rx_sink(sinks)?,
            CaptureConfig::new("sdrmm-espsdr-rx", DRIVER_ID.as_str())
                .with_sample_rate(self.settings.sample_rate),
        )
    }

    fn rx_stop(&mut self) {
        self.capture.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    use sdrmm_device::{LaneEvent, SinkItem};
    use sdrmm_wire::GainValue;

    use super::*;
    use crate::{link::fake::FakeLink, session::tests::firmware};

    fn open(link: &FakeLink) -> EspSdrDevice {
        EspSdrDevice::open("/dev/cu.fake", Box::new(link.clone())).expect("opens")
    }

    fn usb(vid: u16, pid: u16, name: &str) -> sdrmm_serial::SerialPortInfo {
        sdrmm_serial::SerialPortInfo {
            port_name: name.to_string(),
            port_type: sdrmm_serial::SerialPortType::UsbPort(sdrmm_serial::UsbPortInfo {
                vid,
                pid,
                serial_number: Some("0001".to_string()),
                manufacturer: None,
                product: None,
            }),
        }
    }

    #[test]
    fn probing_lists_each_esp_bridge_once_and_skips_other_serial_ports() {
        let ports = vec![
            usb(0x10C4, 0xEA60, "/dev/cu.usbserial-0001"),
            usb(0x10C4, 0xEA60, "/dev/tty.usbserial-0001"),
            usb(0x10C4, 0xEA60, "/dev/cu.SLAB_USBtoUART"),
            usb(0x303A, 0x1001, "/dev/cu.usbmodem1101"),
            usb(0x0483, 0x5740, "/dev/cu.usbmodem400"),
        ];
        let found: Vec<String> = sdrmm_serial::usable(ports)
            .iter()
            .filter(|info| may_be_esp(info))
            .map(|info| device_info(&info.port_name).label)
            .collect();
        assert_eq!(found, ["ESP-SDR usbserial-0001", "ESP-SDR usbmodem1101"]);
        assert_eq!(device_info("COM4").id(), "espsdr:COM4");
    }

    #[test]
    fn a_typed_address_is_an_ethernet_radio_and_a_port_is_not() {
        for port in ["/dev/cu.usbserial-0001", "COM4", "com12", r"\\.\COM10"] {
            assert!(is_serial(port), "{port}");
        }
        for address in ["192.168.1.5", "esp-sdr.local:80", "[::1]:80", "COMPUTER"] {
            assert!(!is_serial(address), "{address}");
        }
        let driver = EspSdrDriver::new();
        let info = driver.resolve("ESP-SDR.local").expect("addressable");
        assert_eq!(info.id(), "espsdr:esp-sdr.local:80");
        assert_eq!(info.label, "ESP-SDR esp-sdr.local:80");
        assert!(driver.probe().contains(&info));
        assert!(driver.resolve("esp-sdr.local:no-port").is_none());
    }

    #[test]
    fn opening_reports_what_the_firmware_offers() {
        let device = open(&FakeLink::new(firmware(2_000_000)));
        assert_eq!(device.capabilities().sample_rates, [80e6, 40e6, 16e6]);
        assert_eq!(device.settings().center_hz, Some(2437e6));
    }

    #[test]
    fn settings_reach_the_radio_before_the_next_burst() {
        let link = FakeLink::new(firmware(2_000_000));
        let mut device = open(&link);
        device
            .apply(&DeviceSettings {
                center_hz: Some(2_412_000_000.0),
                gains: vec![GainValue {
                    stage: "RX".into(),
                    value_db: 20.0,
                }],
                extra: vec![sdrmm_wire::ExtraValue {
                    name: caps::BURST.into(),
                    value: 1024.into(),
                }],
                ..DeviceSettings::default()
            })
            .expect("valid");
        assert!(
            !link.sent().iter().any(|line| line.starts_with("FREQ")),
            "nothing is sent while idle"
        );
        let (tx, rx) = mpsc::channel();
        device
            .rx_start(vec![RxSink::new(move |samples, index| {
                let _ = tx.send((samples.len(), index));
            })])
            .expect("starts");
        let first = rx.recv_timeout(Duration::from_secs(5)).expect("burst");
        let second = rx.recv_timeout(Duration::from_secs(5)).expect("burst");
        device.rx_stop();
        assert_eq!(first, (1024, 0));
        assert_eq!(second.0, 1024);
        assert!(second.1 >= 1024, "bursts sit apart on the timeline");
        let sent = link.sent();
        let tuned = sent.iter().position(|l| l == "FREQ 2412").expect("tuned");
        let gained = sent
            .iter()
            .position(|l| l == "GAIN MANUAL 20")
            .expect("gain");
        let burst = sent
            .iter()
            .position(|l| l == "CAP16 1024 0")
            .expect("burst");
        assert!(tuned < burst && gained < burst);
    }

    #[test]
    fn a_retune_while_streaming_lands_between_bursts() {
        let link = FakeLink::new(firmware(2_000_000));
        let mut device = open(&link);
        let (tx, rx) = mpsc::channel();
        device
            .rx_start(vec![RxSink::new(move |samples, _| {
                let _ = tx.send(samples.len());
            })])
            .expect("starts");
        rx.recv_timeout(Duration::from_secs(5)).expect("burst");
        device
            .apply(&DeviceSettings {
                center_hz: Some(2_462_000_000.0),
                ..DeviceSettings::default()
            })
            .expect("valid");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !link.sent().iter().any(|l| l == "FREQ 2462") {
            assert!(Instant::now() < deadline, "retune never sent");
            rx.recv_timeout(Duration::from_secs(5)).expect("burst");
        }
        device.rx_stop();
    }

    #[test]
    fn gaps_between_bursts_are_flagged_as_estimates() {
        let link = FakeLink::new(firmware(2_000_000));
        let mut device = open(&link);
        let (tx, rx) = mpsc::channel();
        device
            .rx_start(vec![RxSink::with_items(
                move |item| {
                    if let SinkItem::Event(LaneEvent::Uncertain { .. }) = item {
                        let _ = tx.send(());
                    }
                },
                |_| {},
            )])
            .expect("starts");
        rx.recv_timeout(Duration::from_secs(5))
            .expect("a gap is marked");
        device.rx_stop();
    }

    #[test]
    fn an_unplugged_radio_fails_the_stream_as_gone() {
        let link = FakeLink::new(firmware(2_000_000));
        let mut device = open(&link);
        let (tx, rx) = mpsc::channel();
        device
            .rx_start(vec![RxSink::with_fatal_handler(
                |_, _| {},
                move |error| {
                    let _ = tx.send(error);
                },
            )])
            .expect("starts");
        lock(&link.wire).unplugged = true;
        let error = rx.recv_timeout(Duration::from_secs(5)).expect("fails");
        assert!(matches!(error, DeviceError::Disconnected(_)), "{error}");
        device.rx_stop();
    }
}
