//! The data a DAB service carries beside its audio: PAD in MP2 frames and DAB+ access units,
//! and packet-mode data services. A worker reads it off the DSP thread; dynamic labels become
//! the status text and MOT objects become `broadcast_data` events.

use std::thread;

use rtrb::{Consumer, Producer, RingBuffer};
use sdrmm_decoder::{ChannelError, ChannelOutputs};

use super::packet::Config as PacketConfig;

mod worker;

#[cfg(test)]
mod tests;

use worker::{CHUNK_BYTES, Chunk, Found, Lane, Read};

const INPUT_SLOTS: usize = 64;
const OUTPUT_SLOTS: usize = 64;

pub struct ServiceData {
    input: Producer<Chunk>,
    output: Consumer<Read>,
    epoch: u64,
    mot_app: Option<u8>,
    pub service_id: Option<u32>,
    pub dynamic_label: Option<String>,
    pub groups: u32,
    pub errors: u32,
    pub error: Option<String>,
}

impl ServiceData {
    pub fn new() -> Result<Self, ChannelError> {
        let (input, incoming) = RingBuffer::new(INPUT_SLOTS);
        let (outgoing, output) = RingBuffer::new(OUTPUT_SLOTS);
        thread::Builder::new()
            .name("dab-data".to_owned())
            .spawn(move || worker::run(incoming, outgoing))
            .map_err(|error| ChannelError::InvalidSettings(format!("DAB data worker: {error}")))?;
        Ok(Self {
            input,
            output,
            epoch: 0,
            mot_app: None,
            service_id: None,
            dynamic_label: None,
            groups: 0,
            errors: 0,
            error: None,
        })
    }

    /// Forgets the service read so far, as for a new one.
    pub fn reset(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.mot_app = None;
        self.service_id = None;
        self.dynamic_label = None;
        self.groups = 0;
        self.errors = 0;
        self.error = None;
    }

    pub fn select(&mut self, service_id: u32, mot_app: Option<u8>) {
        self.service_id = Some(service_id);
        self.mot_app = mot_app;
    }

    pub fn failed(&mut self, reason: String) {
        self.errors = self.errors.saturating_add(1);
        self.error = Some(reason);
    }

    /// Reads the PAD behind each MP2 frame of a logical frame of the audio stream.
    pub fn mp2(&mut self, logical: &[u8]) {
        self.send(Lane::Mp2, logical);
    }

    /// Reads the PAD in a DAB+ access unit.
    pub fn access_unit(&mut self, unit: &[u8]) {
        self.send(Lane::AccessUnit, unit);
    }

    /// Reads a logical frame of a packet-mode data service under `config`.
    pub fn packets(&mut self, config: PacketConfig, logical: &[u8]) {
        self.send(Lane::Packet(config), logical);
    }

    fn send(&mut self, lane: Lane, bytes: &[u8]) {
        if bytes.len() > CHUNK_BYTES {
            return self.failed("DAB data unit too long".to_owned());
        }
        let mut chunk = Chunk {
            epoch: self.epoch,
            lane,
            mot_app: self.mot_app,
            service_id: self.service_id,
            length: bytes.len(),
            bytes: [0; CHUNK_BYTES],
        };
        chunk.bytes[..bytes.len()].copy_from_slice(bytes);
        if self.input.push(chunk).is_err() {
            self.failed("DAB data input overflow".to_owned());
        }
    }

    /// Moves what the worker read into the status counters, and objects into `out` as events.
    pub fn drain(&mut self, out: &mut ChannelOutputs) {
        while let Ok(read) = self.output.pop() {
            if read.epoch != self.epoch {
                continue;
            }
            match read.found {
                Found::Label(text) => {
                    self.dynamic_label = Some(text);
                    self.groups = self.groups.saturating_add(1);
                }
                Found::Object(event) => {
                    self.groups = self.groups.saturating_add(1);
                    out.events.push(event);
                }
                Found::Error(error) => self.failed(error),
            }
        }
    }

    /// Whether the worker has taken every unit queued.
    #[cfg(test)]
    pub fn input_idle(&self) -> bool {
        self.input.slots() == INPUT_SLOTS
    }
}
