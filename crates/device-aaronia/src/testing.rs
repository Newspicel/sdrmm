use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use sdrmm_device::DeviceError;

use crate::{
    api::{DevHandle, Rtsa},
    caps::{CENTER, CHANNEL, CLOCK, REFERENCE, SPAN, TX_GAIN},
    ffi,
    model::{Found, Item, Kind, Mode, Outgoing, PacketView},
};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Float(f64),
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sent {
    pub flags: u64,
    pub start_hz: f64,
    pub step_hz: f64,
    pub iq: Vec<f32>,
}

struct Queued {
    start: f64,
    end: f64,
    step_hz: f64,
    iq: Vec<f32>,
}

#[derive(Default)]
struct State {
    open: HashMap<usize, (String, Mode)>,
    next_handle: usize,
    opens: Vec<Mode>,
    rescans: usize,
    values: HashMap<(Mode, String), Value>,
    connected: bool,
    started: bool,
    queue: VecDeque<Queued>,
    sent: Vec<Sent>,
    unplugged: bool,
    busy: bool,
}

/// An RTSA API that keeps its configuration trees and packet queue in memory.
pub struct FakeApi {
    devices: Vec<Found>,
    items: HashMap<(Mode, &'static str), Item>,
    transmits: bool,
    epoch: Instant,
    state: Mutex<State>,
}

impl FakeApi {
    pub fn v6() -> Self {
        Self::new(Kind::V6, true)
    }

    pub fn receiver_only() -> Self {
        Self::new(Kind::V6Eco, false)
    }

    fn new(kind: Kind, transmits: bool) -> Self {
        let mut items = HashMap::new();
        let rx = Mode::Receiver;
        let tx = Mode::Transmitter;
        items.insert((rx, CENTER), Item::number(1e6, 6e9, 0.0));
        items.insert((rx, SPAN), Item::number(10e3, 160e6, 0.0));
        items.insert((rx, REFERENCE), Item::number(-20.0, 10.0, 1.0));
        items.insert((tx, CENTER), Item::number(1e6, 6e9, 0.0));
        items.insert((tx, SPAN), Item::number(10e3, 100e6, 0.0));
        items.insert((tx, TX_GAIN), Item::number(-100.0, 0.0, 0.5));
        items.insert(
            (rx, "calibration/preamp"),
            Item::choice(&["Disabled", "Auto", "None", "Amp"]),
        );
        let mut values = HashMap::new();
        for mode in [rx, tx] {
            values.insert((mode, CENTER.to_string()), Value::Float(2.44e9));
            values.insert((mode, SPAN.to_string()), Value::Float(40e6));
        }
        values.insert((rx, REFERENCE.to_string()), Value::Float(0.0));
        values.insert((tx, TX_GAIN.to_string()), Value::Float(-20.0));
        values.insert(
            (rx, "calibration/preamp".to_string()),
            Value::Text("Auto".to_string()),
        );
        if kind == Kind::V6 {
            items.insert(
                (rx, CLOCK),
                Item::choice(&["92MHz", "122MHz", "245MHz", "61MHz", "46MHz"]),
            );
            items.insert(
                (rx, CHANNEL),
                Item::choice(&["Rx1", "Rx2", "Rx1+Rx2", "auto"]),
            );
            values.insert((rx, CLOCK.to_string()), Value::Text("92MHz".to_string()));
            values.insert((rx, CHANNEL.to_string()), Value::Text("auto".to_string()));
        }
        Self {
            devices: vec![Found {
                kind,
                serial: "1040000123".to_string(),
                ready: true,
                active: false,
            }],
            items,
            transmits,
            epoch: Instant::now(),
            state: Mutex::new(State {
                next_handle: 1,
                values,
                ..State::default()
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn mode_of(&self, dev: DevHandle) -> Result<Mode, DeviceError> {
        self.state()
            .open
            .get(&dev.0)
            .map(|(_, mode)| *mode)
            .ok_or_else(|| DeviceError::Io("not open".to_string()))
    }

    pub fn serial(&self) -> String {
        self.devices[0].serial.clone()
    }

    pub fn mark_busy(&self) {
        self.state().busy = true;
    }

    pub fn unplug(&self) {
        self.state().unplugged = true;
    }

    pub fn opens(&self) -> Vec<Mode> {
        self.state().opens.clone()
    }

    pub fn rescans(&self) -> usize {
        self.state().rescans
    }

    pub fn value(&self, mode: Mode, path: &str) -> Option<Value> {
        self.state().values.get(&(mode, path.to_string())).cloned()
    }

    pub fn float(&self, mode: Mode, path: &str) -> f64 {
        match self.value(mode, path) {
            Some(Value::Float(value)) => value,
            other => panic!("{path} is {other:?}"),
        }
    }

    pub fn text(&self, mode: Mode, path: &str) -> String {
        match self.value(mode, path) {
            Some(Value::Text(value)) => value,
            other => panic!("{path} is {other:?}"),
        }
    }

    pub fn is_streaming(&self) -> bool {
        let state = self.state();
        state.connected && state.started
    }

    pub fn queued(&self) -> usize {
        self.state().queue.len()
    }

    pub fn sent(&self) -> Vec<Sent> {
        self.state().sent.clone()
    }

    pub fn push(&self, start: f64, step_hz: f64, iq: Vec<f32>) {
        let end = start + (iq.len() / 2) as f64 / step_hz;
        self.state().queue.push_back(Queued {
            start,
            end,
            step_hz,
            iq,
        });
    }

    pub fn wait_until_drained(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if self.queued() == 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }
}

impl Rtsa for FakeApi {
    fn version(&self) -> u32 {
        (3 << 16) | 16745
    }

    fn library_path(&self) -> &Path {
        Path::new("/opt/fake/libAaroniaRTSAAPI.so")
    }

    fn open_count(&self) -> usize {
        self.state().open.len()
    }

    fn devices(&self, rescan: Option<Duration>) -> Result<Vec<Found>, DeviceError> {
        if rescan.is_some() {
            self.state().rescans += 1;
        }
        Ok(self.devices.clone())
    }

    fn open(&self, kind: Kind, mode: Mode, serial: &str) -> Result<DevHandle, DeviceError> {
        if !self
            .devices
            .iter()
            .any(|d| d.kind == kind && d.serial == serial)
        {
            return Err(DeviceError::NotFound(serial.to_string()));
        }
        if mode == Mode::Transmitter && !self.transmits {
            return Err(DeviceError::Unsupported("no transmitter".to_string()));
        }
        let mut state = self.state();
        if state.busy || state.open.values().any(|(open, _)| open == serial) {
            return Err(DeviceError::InUse(serial.to_string()));
        }
        let handle = state.next_handle;
        state.next_handle += 1;
        state.open.insert(handle, (serial.to_string(), mode));
        state.opens.push(mode);
        Ok(DevHandle(handle))
    }

    fn close(&self, dev: DevHandle) -> Result<(), DeviceError> {
        self.state()
            .open
            .remove(&dev.0)
            .map(|_| ())
            .ok_or_else(|| DeviceError::Io("closed twice".to_string()))
    }

    fn connect(&self, _dev: DevHandle) -> Result<(), DeviceError> {
        self.state().connected = true;
        Ok(())
    }

    fn disconnect(&self, _dev: DevHandle) -> Result<(), DeviceError> {
        self.state().connected = false;
        Ok(())
    }

    fn start(&self, _dev: DevHandle) -> Result<(), DeviceError> {
        self.state().started = true;
        Ok(())
    }

    fn stop(&self, _dev: DevHandle) -> Result<(), DeviceError> {
        self.state().started = false;
        Ok(())
    }

    fn running(&self, _dev: DevHandle) -> bool {
        self.state().started
    }

    fn item(&self, dev: DevHandle, path: &str) -> Result<Option<Item>, DeviceError> {
        let mode = self.mode_of(dev)?;
        Ok(self
            .items
            .iter()
            .find(|((m, p), _)| *m == mode && *p == path)
            .map(|(_, item)| item.clone()))
    }

    fn get_float(&self, dev: DevHandle, path: &str) -> Result<f64, DeviceError> {
        let mode = self.mode_of(dev)?;
        match self.value(mode, path) {
            Some(Value::Float(value)) => Ok(value),
            _ => Err(DeviceError::Unsupported(path.to_string())),
        }
    }

    fn set_float(&self, dev: DevHandle, path: &str, value: f64) -> Result<(), DeviceError> {
        let mode = self.mode_of(dev)?;
        self.state()
            .values
            .insert((mode, path.to_string()), Value::Float(value));
        Ok(())
    }

    fn get_string(&self, dev: DevHandle, path: &str) -> Result<String, DeviceError> {
        let mode = self.mode_of(dev)?;
        match self.value(mode, path) {
            Some(Value::Text(value)) => Ok(value),
            _ => Err(DeviceError::Unsupported(path.to_string())),
        }
    }

    fn set_string(&self, dev: DevHandle, path: &str, value: &str) -> Result<(), DeviceError> {
        let mode = self.mode_of(dev)?;
        self.state()
            .values
            .insert((mode, path.to_string()), Value::Text(value.to_string()));
        Ok(())
    }

    fn health(&self, _dev: DevHandle, path: &str) -> Result<Option<f64>, DeviceError> {
        Ok((path == "fpgatemp").then_some(48.5))
    }

    fn packet(&self, _dev: DevHandle) -> Result<Option<PacketView>, DeviceError> {
        let state = self.state();
        if state.unplugged {
            return Err(DeviceError::Disconnected("unplugged".to_string()));
        }
        Ok(state.queue.front().map(|queued| PacketView {
            flags: 0,
            start_time: queued.start,
            end_time: queued.end,
            step_hz: queued.step_hz,
            num: (queued.iq.len() / 2) as i64,
            size: 2,
            stride: 2,
            data: queued.iq.as_ptr(),
        }))
    }

    fn consume(&self, _dev: DevHandle) -> Result<(), DeviceError> {
        self.state().queue.pop_front();
        Ok(())
    }

    fn send(&self, dev: DevHandle, packet: &Outgoing<'_>) -> Result<(), DeviceError> {
        if self.mode_of(dev)? != Mode::Transmitter {
            return Err(DeviceError::Io("sent to a receiver".to_string()));
        }
        assert!(packet.end_time > packet.start_time);
        self.state().sent.push(Sent {
            flags: packet.flags,
            start_hz: packet.start_hz,
            step_hz: packet.step_hz,
            iq: packet.iq.to_vec(),
        });
        Ok(())
    }

    fn stream_time(&self, _dev: DevHandle) -> Result<f64, DeviceError> {
        Ok(1_000.0 + self.epoch.elapsed().as_secs_f64())
    }
}

pub const SEGMENT: u64 = ffi::PACKET_STREAM_START | ffi::PACKET_SEGMENT_START;
pub const SEGMENT_END: u64 = ffi::PACKET_STREAM_END | ffi::PACKET_SEGMENT_END;
