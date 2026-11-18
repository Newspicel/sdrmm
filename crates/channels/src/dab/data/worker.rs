use std::{thread, time::Duration};

use rtrb::{Consumer, Producer, PushError};
use sdrmm_codec::mp2::Header as Mp2Header;
use sdrmm_wire::DecoderEvent;

use crate::dab::{
    packet::{Config as PacketConfig, PacketData},
    pad::{Event, Pad},
};

/// Room for a whole CIF, the most one logical frame can carry.
pub const CHUNK_BYTES: usize = 8192;

#[derive(Clone, Copy)]
pub enum Lane {
    Mp2,
    AccessUnit,
    Packet(PacketConfig),
}

/// One logical frame or access unit, with the service it belongs to.
pub struct Chunk {
    pub epoch: u64,
    pub lane: Lane,
    pub mot_app: Option<u8>,
    pub service_id: Option<u32>,
    pub length: usize,
    pub bytes: [u8; CHUNK_BYTES],
}

pub enum Found {
    Label(String),
    Object(DecoderEvent),
    Error(String),
}

pub struct Read {
    pub epoch: u64,
    pub found: Found,
}

#[derive(Default)]
struct Reader {
    epoch: u64,
    pad: Pad,
    mp2: Vec<u8>,
    packet: Option<(PacketConfig, PacketData)>,
    events: Vec<Event>,
}

impl Reader {
    fn read(&mut self, chunk: &Chunk) {
        if chunk.epoch != self.epoch {
            *self = Self {
                epoch: chunk.epoch,
                ..Self::default()
            };
        }
        self.pad.mot_app = chunk.mot_app;
        let bytes = &chunk.bytes[..chunk.length];
        match chunk.lane {
            Lane::Mp2 => self.mp2(bytes),
            Lane::AccessUnit => self.pad.access_unit(bytes, &mut self.events),
            Lane::Packet(config) => self.packets(config, bytes),
        }
    }

    fn mp2(&mut self, logical: &[u8]) {
        self.mp2.extend_from_slice(logical);
        while self.mp2.len() >= 4 {
            let Some(header) = Mp2Header::read(&self.mp2) else {
                self.mp2.remove(0);
                continue;
            };
            if self.mp2.len() < header.length {
                break;
            }
            let n = header.length;
            let crc_length = header.scf_crc_bytes();
            if n >= crc_length + 2 {
                self.pad.process(
                    &self.mp2[..n - crc_length - 2],
                    [self.mp2[n - 2], self.mp2[n - 1]],
                    false,
                    &mut self.events,
                );
            }
            self.mp2.drain(..n);
        }
    }

    fn packets(&mut self, config: PacketConfig, logical: &[u8]) {
        if self
            .packet
            .as_ref()
            .is_none_or(|(current, _)| *current != config)
        {
            self.packet = Some((config, PacketData::new(config)));
        }
        if let Some((_, decoder)) = &mut self.packet {
            decoder.push(logical, &mut self.events);
        }
    }
}

fn found(event: Event, service_id: Option<u32>) -> Found {
    match event {
        Event::Label(text) => Found::Label(text),
        Event::Object(mut object) => {
            object.service_id = service_id;
            Found::Object(DecoderEvent::of(&object))
        }
        Event::Error(message) => Found::Error(message.to_owned()),
    }
}

fn publish(output: &mut Producer<Read>, mut read: Read) -> bool {
    while let Err(PushError::Full(held)) = output.push(read) {
        if output.is_abandoned() {
            return false;
        }
        read = held;
        thread::sleep(Duration::from_millis(1));
    }
    true
}

/// Reads chunks until the service data that feeds it is dropped.
pub fn run(mut input: Consumer<Chunk>, mut output: Producer<Read>) {
    let mut reader = Reader::default();
    loop {
        if output.is_abandoned() {
            return;
        }
        let chunk = match input.pop() {
            Ok(chunk) => chunk,
            Err(_) if input.is_abandoned() => return,
            Err(_) => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
        };
        reader.read(&chunk);
        for event in reader.events.drain(..) {
            let read = Read {
                epoch: chunk.epoch,
                found: found(event, chunk.service_id),
            };
            if !publish(&mut output, read) {
                return;
            }
        }
    }
}
