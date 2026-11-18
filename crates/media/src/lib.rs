//! Plays a broadcast decoder's audio and video off the DSP thread: elementary streams through
//! `FFmpeg` or pure-Rust MP2, framed AAC and USAC through FDK, paced by presentation time.
//! A decoder hands it units and the codec they are in; nothing here knows which decoder.
//! Native only, as it links `FFmpeg`.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use ffmpeg_the_third::codec::Id;
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use sdrmm_decoder::{AUDIO_RATE, ChannelError, ChannelOutputs, VideoPicture};
use sdrmm_modem::mpegts::StreamKind;

mod clock;
mod decoder;

#[cfg(test)]
mod tests;

const CHUNK_BYTES: usize = 8192;
const INPUT_SLOTS: usize = 256;
const OUTPUT_SLOTS: usize = 32;
const OUTPUT_BYTES: usize = 64 * 1024 * 1024;

/// The codec a unit is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Codec {
    Mp2,
    Eac3,
    Aac,
    Latm,
    Ac3,
    Mpeg2,
    H264,
    H265,
    /// Raw AAC or USAC access units, decoded by FDK under the config the decoder supplies.
    Fdk(FdkConfig),
}

/// The `AudioSpecificConfig` raw access units decode under, and the codec name their errors
/// carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FdkConfig {
    pub config: Arc<[u8]>,
    pub name: Option<&'static str>,
}

impl Codec {
    /// Raw access units under `config`, their errors led by `name`.
    #[must_use]
    pub fn fdk(config: &[u8], name: Option<&'static str>) -> Self {
        Self::Fdk(FdkConfig {
            config: config.into(),
            name,
        })
    }

    fn id(&self) -> Id {
        match self {
            Self::Eac3 => Id::EAC3,
            Self::Mp2 => Id::None,
            Self::Aac | Self::Fdk(_) => Id::AAC,
            Self::Latm => Id::AAC_LATM,
            Self::Ac3 => Id::AC3,
            Self::Mpeg2 => Id::MPEG2VIDEO,
            Self::H264 => Id::H264,
            Self::H265 => Id::HEVC,
        }
    }

    fn video(&self) -> bool {
        matches!(self, Self::Mpeg2 | Self::H264 | Self::H265)
    }

    /// The codec of an MPEG-TS elementary stream, `None` for one nothing plays.
    #[must_use]
    pub const fn of_stream(kind: StreamKind) -> Option<Self> {
        match kind {
            StreamKind::Mpeg1Audio | StreamKind::Mpeg2Audio => Some(Self::Mp2),
            StreamKind::Eac3Audio => Some(Self::Eac3),
            StreamKind::AacAudio => Some(Self::Aac),
            StreamKind::LatmAudio => Some(Self::Latm),
            StreamKind::Ac3Audio => Some(Self::Ac3),
            StreamKind::Mpeg2Video => Some(Self::Mpeg2),
            StreamKind::H264Video => Some(Self::H264),
            StreamKind::H265Video => Some(Self::H265),
            StreamKind::Other(_) => None,
        }
    }

    fn error(&self, message: String) -> String {
        match self {
            Self::Fdk(FdkConfig {
                name: Some(name), ..
            }) => format!("{name}: {message}"),
            _ => message,
        }
    }
}

#[cfg(feature = "fuzzing")]
const FUZZED: [Codec; 8] = [
    Codec::Mp2,
    Codec::Aac,
    Codec::Latm,
    Codec::Ac3,
    Codec::Eac3,
    Codec::Mpeg2,
    Codec::H264,
    Codec::H265,
];

/// Decodes `bytes` as the codec `selector` picks, checking that what comes out is finite.
#[cfg(feature = "fuzzing")]
pub fn decode_bitstream(selector: u8, bytes: &[u8]) {
    let codec = &FUZZED[usize::from(selector) % FUZZED.len()];
    let Ok(mut decoder) = decoder::Decoder::new(codec) else {
        return;
    };
    let mut decoded = Vec::new();
    if decoder.push(bytes, None, &mut decoded).is_err() {
        let _ = decoder.recover();
    }
    for (_, payload) in &decoded {
        if let Payload::Audio(pcm) = payload {
            assert!(
                pcm.iter().all(|sample| sample.is_finite()),
                "{codec:?} produced a non-finite sample"
            );
        }
    }
}

/// Decodes `units` of `codec` straight through, flushing at the end: the PCM of each audio
/// frame.
#[cfg(any(test, feature = "testing"))]
pub fn decode_audio<'a>(
    codec: &Codec,
    units: impl IntoIterator<Item = &'a [u8]>,
) -> Result<Vec<Vec<f32>>, String> {
    let mut decoder = decoder::Decoder::new(codec)?;
    let mut out = Vec::new();
    for unit in units {
        decoder.push(unit, None, &mut out)?;
    }
    decoder.finish(&mut out)?;
    Ok(out
        .into_iter()
        .filter_map(|(_, payload)| match payload {
            Payload::Audio(pcm) => Some(pcm),
            _ => None,
        })
        .collect())
}

/// The units of a fixture that frames each with a 16-bit big-endian length.
#[cfg(any(test, feature = "testing"))]
#[must_use]
pub fn length_prefixed(mut bytes: &[u8]) -> Vec<&[u8]> {
    let mut units = Vec::new();
    while let [high, low, rest @ ..] = bytes {
        let size = usize::from(u16::from_be_bytes([*high, *low]));
        let Some(unit) = rest.get(..size) else {
            break;
        };
        units.push(unit);
        bytes = &rest[size..];
    }
    units
}

/// A chunk of a unit in `codec`, or `None` for silence standing in for lost audio.
struct Input {
    epoch: u64,
    codec: Option<Codec>,
    pts: Option<i64>,
    length: usize,
    frames: usize,
    bytes: [u8; CHUNK_BYTES],
}

impl Input {
    fn new(epoch: u64, codec: Option<Codec>) -> Self {
        Self {
            epoch,
            codec,
            pts: None,
            length: 0,
            frames: 0,
            bytes: [0; CHUNK_BYTES],
        }
    }
}

enum Payload {
    Audio(Vec<f32>),
    Silence(usize),
    Video(VideoPicture),
    Error { video: bool, message: String },
}

struct Output {
    epoch: u64,
    pts: Option<i64>,
    payload: Payload,
}

impl Output {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match &self.payload {
                Payload::Audio(pcm) => pcm.capacity() * 4,
                Payload::Silence(_) => 0,
                Payload::Video(picture) => picture.rgb.capacity() + picture.luma.capacity(),
                Payload::Error { message, .. } => message.capacity(),
            }
    }
}

fn publish(output: &mut Producer<Output>, resident: &AtomicUsize, mut value: Output) -> bool {
    let bytes = value.bytes();
    while resident.load(Ordering::Acquire) + bytes > OUTPUT_BYTES {
        if output.is_abandoned() {
            return false;
        }
        thread::sleep(Duration::from_millis(1));
    }
    resident.fetch_add(bytes, Ordering::AcqRel);
    while let Err(PushError::Full(held)) = output.push(value) {
        if output.is_abandoned() {
            return false;
        }
        value = held;
        thread::sleep(Duration::from_millis(1));
    }
    true
}

/// The decoders a worker keeps open, one for audio and one for video, each with its codec.
#[derive(Default)]
struct Open {
    audio: Option<(Codec, decoder::Decoder)>,
    video: Option<(Codec, decoder::Decoder)>,
}

impl Open {
    fn decode(
        &mut self,
        codec: &Codec,
        bytes: &[u8],
        pts: Option<i64>,
        frames: &mut Vec<(Option<i64>, Payload)>,
    ) -> Result<(), String> {
        let state = if codec.video() {
            &mut self.video
        } else {
            &mut self.audio
        };
        if state.as_ref().is_none_or(|(current, _)| current != codec) {
            *state = Some((codec.clone(), decoder::Decoder::new(codec)?));
        }
        let Some((_, decoder)) = state else {
            return Err("Broadcast decoder missing".to_owned());
        };
        let result = decoder.push(bytes, pts, frames);
        if let Err(mut message) = result {
            if let Err(error) = decoder.recover() {
                message.push_str(&format!("; {error}"));
                *state = None;
            }
            return Err(message);
        }
        Ok(())
    }
}

fn run(mut input: Consumer<Input>, mut output: Producer<Output>, resident: &AtomicUsize) {
    let mut epoch = 0;
    let mut open = Open::default();
    let mut frames = Vec::new();
    loop {
        if output.is_abandoned() {
            return;
        }
        let item = match input.pop() {
            Ok(item) => item,
            Err(_) if input.is_abandoned() => return,
            Err(_) => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
        };
        if epoch != item.epoch {
            open = Open::default();
            epoch = item.epoch;
        }
        let Some(codec) = &item.codec else {
            let silence = Output {
                epoch,
                pts: None,
                payload: Payload::Silence(item.frames),
            };
            if !publish(&mut output, resident, silence) {
                return;
            }
            continue;
        };
        let result = open
            .decode(codec, &item.bytes[..item.length], item.pts, &mut frames)
            .map_err(|message| codec.error(message));
        for (pts, payload) in frames.drain(..) {
            if !publish(
                &mut output,
                resident,
                Output {
                    epoch,
                    pts,
                    payload,
                },
            ) {
                return;
            }
        }
        if let Err(message) = result {
            let failure = Output {
                epoch,
                pts: item.pts,
                payload: Payload::Error {
                    video: codec.video(),
                    message,
                },
            };
            if !publish(&mut output, resident, failure) {
                return;
            }
        }
    }
}

/// A broadcast decoder's media worker and what came back from it: the frames played and the
/// errors met, for the decoder's status.
pub struct Media {
    input: Producer<Input>,
    resident: Arc<AtomicUsize>,
    output: Consumer<Output>,
    epoch: u64,
    clock: Option<clock::Clock>,
    pending: VecDeque<Output>,
    pub audio_frames: u32,
    pub video_frames: u32,
    pub audio_errors: u32,
    pub video_errors: u32,
    pub audio_error: Option<String>,
    pub video_error: Option<String>,
}

impl Media {
    pub fn new() -> Result<Self, ChannelError> {
        let (input, incoming) = RingBuffer::new(INPUT_SLOTS);
        let (outgoing, output) = RingBuffer::new(OUTPUT_SLOTS);
        let resident = Arc::new(AtomicUsize::new(0));
        let worker_resident = resident.clone();
        thread::Builder::new()
            .name("broadcast-media".to_owned())
            .spawn(move || run(incoming, outgoing, &worker_resident))
            .map_err(|error| ChannelError::InvalidSettings(format!("Media worker: {error}")))?;
        Ok(Self {
            input,
            resident,
            output,
            epoch: 0,
            clock: None,
            pending: VecDeque::with_capacity(OUTPUT_SLOTS),
            audio_frames: 0,
            video_frames: 0,
            audio_errors: 0,
            video_errors: 0,
            audio_error: None,
            video_error: None,
        })
    }

    /// Holds each frame back until the samples played reach its presentation time.
    pub fn enable_clock(&mut self) {
        self.clock = Some(clock::Clock::default());
    }

    pub fn advance(&mut self, samples: usize, rate: f64) {
        if let Some(clock) = &mut self.clock {
            clock.advance(samples, rate);
        }
    }

    /// Drops whatever is queued and starts the counts over, as for a new stream.
    pub fn reset(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.clock.is_some() {
            self.enable_clock();
        }
        for item in self.pending.drain(..) {
            self.resident.fetch_sub(item.bytes(), Ordering::AcqRel);
        }
        self.audio_frames = 0;
        self.video_frames = 0;
        self.audio_errors = 0;
        self.video_errors = 0;
        self.audio_error = None;
        self.video_error = None;
    }

    /// Whether the worker has taken every unit queued.
    #[must_use]
    pub fn input_idle(&self) -> bool {
        self.input.slots() == INPUT_SLOTS
    }

    /// Counts `count` lost audio units for `reason` and fills their `frames` with silence.
    pub fn audio_gap(&mut self, frames: usize, count: u32, reason: &str) {
        self.audio_errors = self.audio_errors.saturating_add(count);
        self.audio_error = Some(reason.to_owned());
        self.conceal(frames);
    }

    fn conceal(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        let item = Input {
            frames,
            ..Input::new(self.epoch, None)
        };
        if self.input.slots() == 0 || self.input.push(item).is_err() {
            self.failed(false, "Broadcast media input overflow".to_owned());
        }
    }

    fn failed(&mut self, video: bool, error: String) {
        if video {
            self.video_errors = self.video_errors.saturating_add(1);
            self.video_error = Some(error);
        } else {
            self.audio_errors = self.audio_errors.saturating_add(1);
            self.audio_error = Some(error);
        }
    }

    /// Queues one unit of `codec`, `pts` its presentation time on the 90 kHz clock. An
    /// access unit for FDK must fit one chunk; a stream may run over several.
    pub fn push(&mut self, codec: &Codec, bytes: &[u8], pts: Option<u64>) {
        let framed = matches!(codec, Codec::Fdk(_));
        if self.input.slots() < bytes.len().div_ceil(CHUNK_BYTES)
            || (framed && bytes.len() > CHUNK_BYTES)
        {
            self.failed(codec.video(), "Broadcast media input overflow".to_owned());
            self.epoch = self.epoch.wrapping_add(1);
            return;
        }
        for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            let mut item = Input {
                pts: pts.filter(|_| index == 0).map(|v| v as i64),
                length: chunk.len(),
                ..Input::new(self.epoch, Some(codec.clone()))
            };
            item.bytes[..chunk.len()].copy_from_slice(chunk);
            if self.input.push(item).is_err() {
                self.failed(codec.video(), "Broadcast media worker stopped".to_owned());
                return;
            }
        }
    }

    /// Moves what the worker played, once due, into `out`.
    pub fn drain(&mut self, out: &mut ChannelOutputs) {
        while self.pending.len() < OUTPUT_SLOTS {
            let Ok(item) = self.output.pop() else { break };
            self.pending.push_back(item);
        }
        for _ in 0..self.pending.len() {
            let Some(item) = self.pending.pop_front() else {
                break;
            };
            if item.epoch != self.epoch {
                self.resident.fetch_sub(item.bytes(), Ordering::AcqRel);
                continue;
            }
            if let (Some(clock), Some(pts)) = (&mut self.clock, item.pts)
                && !clock.due(pts)
            {
                self.pending.push_back(item);
                continue;
            }
            self.resident.fetch_sub(item.bytes(), Ordering::AcqRel);
            self.deliver(item.payload, out);
        }
    }

    fn deliver(&mut self, payload: Payload, out: &mut ChannelOutputs) {
        match payload {
            Payload::Audio(pcm) => {
                out.audio_rate = AUDIO_RATE;
                out.audio_pcm.extend_from_slice(&pcm);
                self.audio_frames = self.audio_frames.saturating_add(1);
            }
            Payload::Silence(frames) => {
                out.audio_rate = AUDIO_RATE;
                out.audio_pcm.resize(out.audio_pcm.len() + frames * 2, 0.0);
            }
            Payload::Video(picture) => {
                out.video.push(picture);
                self.video_frames = self.video_frames.saturating_add(1);
            }
            Payload::Error { video, message } => self.failed(video, message),
        }
    }
}
