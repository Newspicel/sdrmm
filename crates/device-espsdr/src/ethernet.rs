use std::sync::Arc;

use sdrmm_device::{
    Capture, CaptureConfig, DeviceError, RxSink, SdrDevice, lock, net::Endpoint, single_rx_sink,
};
use sdrmm_wire::{Capabilities, DeviceSettings};

use crate::{
    DRIVER_ID,
    caps::{Delivery, Profile, Remote},
    ethernet::{
        api::{PROTOCOL, Request, Status, Tuning, post},
        packet::RATES,
        stream::{NetRadio, converter},
    },
    proto::{Bits, Features, Limits},
};

mod api;
mod packet;
mod stream;

const RANGE_MHZ: (u32, u32) = (2150, 2800);
const BANDWIDTH_MHZ: (u32, u32) = (13, 54);

fn profile(gain_max: u32) -> Profile {
    Profile {
        chip: "ESP32-S31".to_string(),
        delivery: Delivery::Stream,
        features: Features::of(&["GAIN", "HWAGC"]),
        limits: Limits {
            gain_max,
            bandwidth_mhz: Some(BANDWIDTH_MHZ),
            rates: RATES.to_vec(),
            bits: vec![Bits::Eight],
        },
        range_mhz: RANGE_MHZ,
    }
}

fn remote(status: &Status) -> Remote {
    let tuning = status.tuning;
    Remote {
        center_mhz: Tuning::mhz(tuning.frequency),
        rate: tuning.rate,
        bandwidth_mhz: Tuning::mhz(tuning.bandwidth),
        agc: tuning.agc != 0,
        gain: tuning.gain,
        bits: Bits::Eight,
        burst: 0,
    }
}

pub(crate) use api::HTTP_PORT;

pub struct EspSdrNetDevice {
    radio: Arc<NetRadio>,
    profile: Profile,
    capabilities: Capabilities,
    settings: DeviceSettings,
    capture: Capture<NetRadio>,
}

impl EspSdrNetDevice {
    pub(crate) fn open(endpoint: Endpoint) -> Result<Self, DeviceError> {
        let status = post(&endpoint, &Request::status())?;
        if status.protocol != PROTOCOL {
            return Err(DeviceError::Unsupported(format!(
                "{endpoint}: ESP-SDR RX protocol {}, this build speaks {PROTOCOL}",
                status.protocol
            )));
        }
        let profile = profile(status.gain_max);
        let desired = remote(&status);
        tracing::info!(%endpoint, gain_max = status.gain_max, "opened ESP-SDR over Ethernet");
        Ok(Self {
            capabilities: profile.capabilities(),
            settings: profile.wire(&desired),
            radio: Arc::new(NetRadio::new(endpoint, desired)),
            profile,
            capture: Capture::new(),
        })
    }
}

impl SdrDevice for EspSdrNetDevice {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        let mut desired = lock(&self.radio.desired);
        let next = self
            .profile
            .validate(settings, &self.capabilities, *desired)?;
        if next != *desired {
            self.radio.retune(&next)?;
        }
        *desired = next;
        self.settings = self.profile.wire(&next);
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        self.capture.start(
            self.radio.clone(),
            converter(),
            single_rx_sink(sinks)?,
            CaptureConfig::new("sdrmm-espsdr-net-rx", DRIVER_ID)
                .with_sample_rate(self.settings.sample_rate),
        )
    }

    fn rx_stop(&mut self) {
        self.capture.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Write, net::TcpListener, thread};

    use sdrmm_wire::{BandwidthSetting, ExtraSetting};

    use super::*;
    use crate::ethernet::api::{
        HTTP_PORT,
        tests::{STATUS, read_request, reply},
    };

    fn board(status: &'static str) -> (Endpoint, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let at = listener.local_addr().expect("addr").to_string();
        let server = thread::spawn(move || {
            let (mut tcp, _) = listener.accept().expect("accept");
            read_request(&tcp);
            tcp.write_all(reply(200, status).as_bytes()).expect("reply");
        });
        (Endpoint::parse(&at, HTTP_PORT).expect("endpoint"), server)
    }

    #[test]
    fn a_board_opens_with_the_settings_it_reports() {
        let (at, server) = board(STATUS);
        let mut device = EspSdrNetDevice::open(at).expect("opens");
        server.join().expect("board");
        let caps = device.capabilities();
        assert_eq!(caps.freq_ranges[0].min, 2150e6);
        assert_eq!(caps.sample_rates, [4e6, 8e6, 16e6, 20e6, 40e6]);
        assert_eq!(caps.gains[0].range.max, 72.0);
        assert!(caps.extra.iter().map(ExtraSetting::name).next().is_none());
        assert_eq!(device.settings().center_hz, Some(2442e6));
        assert_eq!(device.settings().bandwidth, Some(BandwidthSetting::Auto));
        device
            .apply(&DeviceSettings {
                sample_rate: Some(40e6),
                ..DeviceSettings::default()
            })
            .expect("not streaming yet");
        assert_eq!(device.settings().sample_rate, Some(40e6));
        assert!(
            device
                .apply(&DeviceSettings {
                    center_hz: Some(433e6),
                    ..DeviceSettings::default()
                })
                .is_err()
        );
    }

    #[test]
    fn another_protocol_is_refused() {
        let (at, server) = board(
            r#"{"protocol":3,"gain_max":72,"frequency":2442000000,"rate":16000000,"gain":40,"agc":0,"bandwidth":0}"#,
        );
        assert!(matches!(
            EspSdrNetDevice::open(at),
            Err(DeviceError::Unsupported(_))
        ));
        server.join().expect("board");
    }
}
