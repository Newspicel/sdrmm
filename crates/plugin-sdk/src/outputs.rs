use sdrmm_wire::{ChannelStatus, DecoderEvent, EventContract, EventKind};
use serde::Serialize;

/// What one block of input produced.
#[derive(Debug, Default)]
pub struct Outputs {
    /// Interleaved samples at [`Outputs::audio_rate`].
    pub audio: Vec<f32>,
    pub audio_rate: u32,
    pub events: Vec<DecoderEvent>,
    pub status: Option<ChannelStatus>,
    /// Frames heard but failing their checks, counted up over the channel's life.
    pub rejected: u32,
    /// Blocks dropped unprocessed, counted up over the channel's life.
    pub skipped: u32,
}

impl Outputs {
    pub fn emit<T: EventContract>(&mut self, payload: &T) {
        self.events.push(DecoderEvent::of(payload));
    }

    pub fn emit_as(&mut self, kind: EventKind, payload: &impl Serialize) {
        self.events.push(DecoderEvent::tagged(kind, payload));
    }

    pub fn clear(&mut self) {
        self.audio.clear();
        self.events.clear();
        self.status = None;
    }
}
