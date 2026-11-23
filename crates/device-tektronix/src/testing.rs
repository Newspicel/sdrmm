use std::{
    collections::VecDeque,
    path::Path,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use sdrmm_device::DeviceError;

use crate::{
    api::Rsa,
    model::{Chunk, Fitted, Found, FrequencySource, Limits},
};

pub const TIMESTAMP_RATE: u64 = 112_000_000;
const QUIET_POLL: Duration = Duration::from_millis(2);

pub const RSA306: Limits = Limits {
    center_min: 9e3,
    center_max: 6.2e9,
    bandwidth_min: 9765.625,
    bandwidth_max: 40e6,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Call {
    Run,
    Stop,
    StreamStart,
    StreamStop,
    Center(f64),
    Reference(f64),
    Bandwidth(f64),
    Preamp(bool),
    AutoAttenuation(bool),
    Attenuation(f64),
    Source(FrequencySource),
}

struct Queued {
    timestamp: u64,
    status: u32,
    iq: Vec<f32>,
}

struct State {
    connected: bool,
    center: f64,
    reference: f64,
    preamp: bool,
    auto_attenuation: bool,
    attenuation: f64,
    source: FrequencySource,
    external_absent: bool,
    bandwidth: f64,
    running: bool,
    streaming: bool,
    queue: VecDeque<Queued>,
    calls: Vec<Call>,
    unplugged: bool,
}

/// An RSA API that keeps its one analyser's settings and block queue in memory.
pub struct FakeApi {
    found: Found,
    fitted: Fitted,
    state: Mutex<State>,
}

impl FakeApi {
    pub fn rsa306() -> Self {
        Self::new("RSA306", Fitted::default())
    }

    pub fn rsa507a() -> Self {
        Self::new(
            "RSA507A",
            Fitted {
                preamp: true,
                attenuator: true,
                gnss: true,
            },
        )
    }

    fn new(model: &str, fitted: Fitted) -> Self {
        Self {
            found: Found {
                id: 0,
                serial: "B012345".to_string(),
                model: model.to_string(),
            },
            fitted,
            state: Mutex::new(State {
                connected: false,
                center: 1.5e9,
                reference: 0.0,
                preamp: false,
                auto_attenuation: fitted.attenuator,
                attenuation: 0.0,
                source: FrequencySource::Internal,
                external_absent: false,
                bandwidth: 40e6,
                running: false,
                streaming: false,
                queue: VecDeque::new(),
                calls: Vec::new(),
                unplugged: false,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn record(&self, call: Call) {
        self.state().calls.push(call);
    }

    pub fn serial(&self) -> String {
        self.found.serial.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.state().connected
    }

    pub fn is_streaming(&self) -> bool {
        let state = self.state();
        state.running && state.streaming
    }

    pub fn leave_the_external_reference_open(&self) {
        self.state().external_absent = true;
    }

    pub fn unplug(&self) {
        self.state().unplugged = true;
    }

    pub fn take_calls(&self) -> Vec<Call> {
        std::mem::take(&mut self.state().calls)
    }

    pub fn push(&self, timestamp: u64, iq: Vec<f32>) {
        self.state().queue.push_back(Queued {
            timestamp,
            status: 0,
            iq,
        });
    }

    pub fn wait_until_drained(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if self.state().queue.is_empty() {
                return true;
            }
            std::thread::sleep(QUIET_POLL);
        }
        false
    }
}

impl Rsa for FakeApi {
    fn version(&self) -> String {
        "5.6.0295.0".to_string()
    }

    fn library_path(&self) -> &Path {
        Path::new("C:/Tektronix/RSA_API/lib/x64/RSA_API.dll")
    }

    fn devices(&self) -> Result<Vec<Found>, DeviceError> {
        Ok(vec![self.found.clone()])
    }

    fn connect(&self, found: &Found) -> Result<(), DeviceError> {
        let mut state = self.state();
        if state.connected {
            return Err(DeviceError::InUse(found.serial.clone()));
        }
        state.connected = true;
        Ok(())
    }

    fn disconnect(&self) -> Result<(), DeviceError> {
        self.state().connected = false;
        Ok(())
    }

    fn limits(&self) -> Result<Limits, DeviceError> {
        Ok(RSA306)
    }

    fn center(&self) -> Result<f64, DeviceError> {
        Ok(self.state().center)
    }

    fn set_center(&self, hz: f64) -> Result<(), DeviceError> {
        self.record(Call::Center(hz));
        self.state().center = hz;
        Ok(())
    }

    fn reference(&self) -> Result<f64, DeviceError> {
        Ok(self.state().reference)
    }

    fn set_reference(&self, dbm: f64) -> Result<(), DeviceError> {
        self.record(Call::Reference(dbm));
        self.state().reference = dbm;
        Ok(())
    }

    fn preamp(&self) -> Result<bool, DeviceError> {
        Ok(self.state().preamp)
    }

    fn set_preamp(&self, on: bool) -> Result<(), DeviceError> {
        self.record(Call::Preamp(on));
        self.state().preamp = on && self.fitted.preamp;
        Ok(())
    }

    fn auto_attenuation(&self) -> Result<bool, DeviceError> {
        Ok(self.state().auto_attenuation)
    }

    fn set_auto_attenuation(&self, on: bool) -> Result<(), DeviceError> {
        self.record(Call::AutoAttenuation(on));
        self.state().auto_attenuation = on && self.fitted.attenuator;
        Ok(())
    }

    fn attenuation(&self) -> Result<f64, DeviceError> {
        Ok(self.state().attenuation)
    }

    fn set_attenuation(&self, db: f64) -> Result<(), DeviceError> {
        self.record(Call::Attenuation(db));
        if self.fitted.attenuator {
            self.state().attenuation = db;
        }
        Ok(())
    }

    fn frequency_source(&self) -> Result<FrequencySource, DeviceError> {
        Ok(self.state().source)
    }

    fn set_frequency_source(&self, source: FrequencySource) -> Result<(), DeviceError> {
        self.record(Call::Source(source));
        let mut state = self.state();
        if source == FrequencySource::External && state.external_absent {
            return Err(DeviceError::Io(
                "Local oscillator did not achieve lock (3009)".to_string(),
            ));
        }
        state.source = source;
        Ok(())
    }

    fn gnss_fitted(&self) -> Result<bool, DeviceError> {
        Ok(self.fitted.gnss)
    }

    fn set_bandwidth(&self, hz: f64) -> Result<(), DeviceError> {
        self.record(Call::Bandwidth(hz));
        self.state().bandwidth = hz;
        Ok(())
    }

    fn rate(&self) -> Result<f64, DeviceError> {
        let bandwidth = self.state().bandwidth;
        let mut rate = 56e6;
        while rate / 2.8 >= bandwidth && rate > 13_671.875 {
            rate /= 2.0;
        }
        Ok(rate)
    }

    fn timestamp_rate(&self) -> Result<u64, DeviceError> {
        Ok(TIMESTAMP_RATE)
    }

    fn run(&self) -> Result<(), DeviceError> {
        self.record(Call::Run);
        self.state().running = true;
        Ok(())
    }

    fn stop(&self) -> Result<(), DeviceError> {
        self.record(Call::Stop);
        self.state().running = false;
        Ok(())
    }

    fn stream_start(&self) -> Result<(), DeviceError> {
        self.record(Call::StreamStart);
        self.state().streaming = true;
        Ok(())
    }

    fn stream_stop(&self) -> Result<(), DeviceError> {
        self.record(Call::StreamStop);
        self.state().streaming = false;
        Ok(())
    }

    fn wait(&self, timeout: Duration) -> Result<bool, DeviceError> {
        {
            let state = self.state();
            if state.unplugged {
                return Err(DeviceError::Disconnected("unplugged".to_string()));
            }
            if state.running && state.streaming && !state.queue.is_empty() {
                return Ok(true);
            }
        }
        std::thread::sleep(timeout.min(QUIET_POLL));
        Ok(false)
    }

    fn read(&self, out: &mut [f32]) -> Result<Option<Chunk>, DeviceError> {
        let Some(queued) = self.state().queue.pop_front() else {
            return Ok(None);
        };
        out[..queued.iq.len()].copy_from_slice(&queued.iq);
        Ok(Some(Chunk {
            pairs: queued.iq.len() / 2,
            timestamp: queued.timestamp,
            status: queued.status,
        }))
    }
}
