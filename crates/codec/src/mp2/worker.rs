use std::{thread, time::Duration};

use rtrb::{Consumer, Producer, PushError, RingBuffer};

use super::{FRAME_BYTES, Framed, Framer, Header, Mp2Decoder, PCM_SAMPLES};

const QUEUE_FRAMES: usize = 32;

struct Encoded {
    epoch: u64,
    discontinuity: bool,
    header: Header,
    bytes: [u8; FRAME_BYTES],
}

struct Decoded {
    epoch: u64,
    length: usize,
    conceal: usize,
    pcm: [f32; PCM_SAMPLES],
    error: Option<&'static str>,
}

#[expect(
    clippy::large_stack_arrays,
    reason = "PCM frame lives on the stack, no allocation"
)]
fn run(mut decoder: Mp2Decoder, mut input: Consumer<Encoded>, mut output: Producer<Decoded>) {
    let mut epoch = 0;
    loop {
        if output.is_abandoned() {
            return;
        }
        let encoded = match input.pop() {
            Ok(encoded) => encoded,
            Err(_) if input.is_abandoned() => return,
            Err(_) => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
        };
        if encoded.epoch != epoch || encoded.discontinuity {
            decoder.reset();
            epoch = encoded.epoch;
        }
        let mut decoded = Decoded {
            epoch,
            length: 0,
            conceal: 0,
            pcm: [0.0; PCM_SAMPLES],
            error: None,
        };
        let frame = &encoded.bytes[..encoded.header.length];
        match decoder.decode(frame, encoded.header, &mut decoded.pcm) {
            Ok(length) => decoded.length = length,
            Err(error) => {
                decoded.error = Some(error);
                decoded.conceal = encoded.header.output_samples();
                decoder.reset();
            }
        }
        while let Err(PushError::Full(held)) = output.push(decoded) {
            if output.is_abandoned() {
                return;
            }
            decoded = held;
            thread::sleep(Duration::from_millis(1));
        }
    }
}

/// MP2 decoded on a worker thread, so a DSP thread only pushes bytes and drains audio.
pub struct LayerTwoAudio {
    input: Producer<Encoded>,
    output: Consumer<Decoded>,
    framer: Framer,
    epoch: u64,
    lost_sync: bool,
    discontinuity: bool,
    pub frames_ok: u32,
    pub frames_bad: u32,
    pub error: Option<&'static str>,
}

impl LayerTwoAudio {
    pub fn new() -> Result<Self, String> {
        let decoder = Mp2Decoder::new()?;
        let (input, incoming) = RingBuffer::new(QUEUE_FRAMES);
        let (outgoing, output) = RingBuffer::new(QUEUE_FRAMES);
        thread::Builder::new()
            .name("broadcast-audio".to_owned())
            .spawn(move || run(decoder, incoming, outgoing))
            .map_err(|error| format!("Broadcast audio worker: {error}"))?;
        Ok(Self {
            input,
            output,
            framer: Framer::default(),
            epoch: 0,
            lost_sync: false,
            discontinuity: false,
            frames_ok: 0,
            frames_bad: 0,
            error: None,
        })
    }

    pub fn reset(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.framer.reset();
        self.lost_sync = false;
        self.discontinuity = false;
        self.frames_ok = 0;
        self.frames_bad = 0;
        self.error = None;
    }

    fn failed(&mut self, reason: &'static str) {
        self.frames_bad = self.frames_bad.saturating_add(1);
        self.error = Some(reason);
    }

    pub fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match self.framer.push(byte) {
                Framed::Pending => {}
                Framed::Skipped => {
                    if !self.lost_sync {
                        self.failed("Lost MPEG Layer II frame synchronization");
                    }
                    self.lost_sync = true;
                    self.discontinuity = true;
                }
                Framed::Frame(header) => {
                    self.lost_sync = false;
                    self.send(header);
                }
            }
        }
    }

    fn send(&mut self, header: Header) {
        let mut bytes = [0; FRAME_BYTES];
        bytes[..header.length].copy_from_slice(self.framer.frame(header));
        let frame = Encoded {
            epoch: self.epoch,
            discontinuity: self.discontinuity,
            header,
            bytes,
        };
        if self.input.push(frame).is_err() {
            self.failed("Broadcast audio input queue overflow");
            self.discontinuity = true;
        } else {
            self.discontinuity = false;
        }
    }

    /// Appends the audio decoded since, lost frames as silence; whether any was appended.
    pub fn drain(&mut self, pcm: &mut Vec<f32>) -> bool {
        let mut produced = false;
        while let Ok(decoded) = self.output.pop() {
            if decoded.epoch != self.epoch {
                continue;
            }
            produced = true;
            if let Some(error) = decoded.error {
                self.failed(error);
                pcm.resize(pcm.len() + decoded.conceal, 0.0);
                continue;
            }
            pcm.extend_from_slice(&decoded.pcm[..decoded.length]);
            self.frames_ok = self.frames_ok.saturating_add(1);
        }
        if self.input.is_abandoned() && self.error != Some("Broadcast audio worker stopped") {
            self.failed("Broadcast audio worker stopped");
        }
        produced
    }

    /// Bytes held towards the next frame.
    #[must_use]
    pub const fn buffered(&self) -> usize {
        self.framer.buffered()
    }
}
