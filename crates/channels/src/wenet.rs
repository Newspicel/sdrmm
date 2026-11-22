//! Wenet: Project Horus balloon pictures. 2-FSK at 115177 baud (RS232-framed) or 96000 baud
//! (Wenet v2), LDPC-coded 256-byte packets carrying SSDV picture packets and telemetry. Both
//! framings are searched at once.

use sdrmm_decoder::registry::Native;
use sdrmm_wire::{FilterSpec, HzRange, KindSpec, Modulation, SettingField, SignalSignature};
mod code;
mod demod;
pub(crate) mod frame;
mod model;
pub(crate) mod packet;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use demod::{CHUNK, FrontEnd, symbol_clock};
use frame::{Deframer, Outcome, PACKET_LEN};
use num_complex::Complex;
use packet::Payload;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, DecodedImage, NativeChannel, VideoPicture,
    check_input_rate,
};
use sdrmm_dsp::demod::sync::CrossingClock;
use sdrmm_modem::cpm::Mapping;
use sdrmm_modem::ssdv::{Assembler, Header, PacketChecker};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventContract};

/// Eight samples a symbol at the legacy rate, as the Wenet receiver runs.
pub(crate) const INPUT_RATE_HZ: f64 = 921_416.0;
const FILTER_TAPS: usize = 127;
const SIGNAL_LOSS_S: f64 = 5.0;
const PROGRESS_S: f64 = 1.0;
const SOURCE: &str = "wenet";
const WENET_BAND: &[HzRange] = &[HzRange::mhz(430.0, 450.0)];
/// One symbol in this many reaches the scope, some fourteen thousand a second.
const TAP_EVERY: usize = 8;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "wenet".to_owned(),
    name: "Wenet".to_owned(),
    summary: "Balloon pictures and telemetry".to_owned(),
    family: DecoderFamily::AMATEUR,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: 300_000.0,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    has_video: true,
    emits: vec![WenetTelemetry::KIND, WenetImage::KIND],
    ..ChannelDescriptor::default()
});

pub(crate) fn wenet_decoder() -> Native<WenetParams> {
    let decoder = Native::new::<WenetChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(443_500_000.0)
        .signatures(wenet_signatures)
        .form(wenet_form)
        .kinds(wenet_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn wenet_form() -> Vec<SettingField> {
    Vec::new()
}

fn wenet_kinds() -> Vec<KindSpec> {
    model::wenet_kinds()
}

fn wenet_signatures() -> Vec<SignalSignature> {
    WenetMode::ALL
        .iter()
        .map(|&mode| {
            SignalSignature::new(
                "Wenet",
                &[Modulation::Fsk2],
                HzRange::new(180_000.0, 330_000.0),
                "Balloon picture downlink, 2-FSK near 100 kbaud",
            )
            .symbol_rate(HzRange::about(mode.baud(), 0.03))
            .on(WENET_BAND)
        })
        .collect()
}

pub(crate) fn occupied_band() -> (f64, f64) {
    let half = DESCRIPTOR.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter() -> FilterSpec {
    let (_, half) = occupied_band();
    FilterSpec::lowpass(FILTER_TAPS, half)
}

struct Lane {
    mode: WenetMode,
    clock: CrossingClock,
    deframer: Deframer,
}

/// The picture being drawn and what is known of it.
struct Picture {
    assembler: Assembler,
    mode: WenetMode,
    seq: u32,
    started: u64,
    last_packet: u64,
    last_shown: u64,
    shown_lines: u16,
    frames_failed: u32,
    finished: Option<(u32, u8)>,
}

impl Picture {
    fn new() -> Self {
        Self {
            assembler: Assembler::new(),
            mode: WenetMode::Uart,
            seq: 0,
            started: 0,
            last_packet: 0,
            last_shown: 0,
            shown_lines: 0,
            frames_failed: 0,
            finished: None,
        }
    }

    fn place(
        &mut self,
        packet: &[u8],
        header: &Header,
        mode: WenetMode,
        now: u64,
        out: &mut ChannelOutputs,
    ) {
        if self.finished == Some((header.callsign, header.image_id)) {
            return;
        }
        let current = self.assembler.header().copied();
        if current.is_none_or(|image| !image.same_image(header)) {
            self.finish(out);
            self.assembler.start(header);
            self.started = now;
            self.shown_lines = 0;
        }
        self.mode = mode;
        self.last_packet = now;
        self.assembler.feed(packet, header);
        if header.eoi {
            self.finish(out);
        }
    }

    fn snapshot(&self) -> Option<VideoPicture> {
        let header = self.assembler.header()?;
        Some(VideoPicture {
            width: header.width,
            height: header.height,
            luma: self.assembler.luma().to_vec(),
            rgb: self.assembler.rgb().to_vec(),
        })
    }

    fn progress(&mut self, now: u64, out: &mut ChannelOutputs) {
        let lines = self.assembler.lines();
        let due = (now.saturating_sub(self.last_shown) as f64) >= PROGRESS_S * INPUT_RATE_HZ;
        if lines > self.shown_lines
            && due
            && let Some(picture) = self.snapshot()
        {
            self.shown_lines = lines;
            self.last_shown = now;
            out.video.push(picture);
        }
    }

    fn finish(&mut self, out: &mut ChannelOutputs) {
        let Some(header) = self.assembler.header().copied() else {
            return;
        };
        let progress = self.assembler.progress();
        if progress.drawn > 0
            && let Some(picture) = self.snapshot()
        {
            self.seq = self.seq.wrapping_add(1);
            let complete = self.assembler.is_complete();
            let lines = self.assembler.lines();
            let elapsed = self.last_packet.saturating_sub(self.started);
            out.events.push(DecoderEvent::of(&WenetImage {
                seq: self.seq,
                mode: self.mode,
                callsign: header.callsign(),
                image_id: header.image_id,
                width: header.width,
                height: header.height,
                lines,
                complete,
                duration_ms: (elapsed as f64 * 1_000.0 / INPUT_RATE_HZ) as u32,
                quality: header.quality,
                sampling: header.mcu_mode.label().to_owned(),
                packets: progress.packets,
                packets_lost: progress.lost,
                frames_failed: self.frames_failed,
            }));
            out.video.push(picture.clone());
            out.images.push(DecodedImage {
                source: SOURCE.into(),
                mode: format!("SSDV {}", header.mcu_mode.label()),
                complete,
                lines,
                picture,
            });
        }
        self.finished = Some((header.callsign, header.image_id));
        self.assembler.clear();
        self.frames_failed = 0;
    }
}

/// Where decoded frames go.
struct Sink {
    checker: PacketChecker,
    picture: Picture,
    callsign: Option<String>,
    ssdv_callsign: Option<u32>,
}

impl Sink {
    fn on_packet(
        &mut self,
        mode: WenetMode,
        frame: &[u8; PACKET_LEN],
        now: u64,
        out: &mut ChannelOutputs,
    ) {
        match packet::parse(frame, mode) {
            Payload::Telemetry(mut telemetry) => {
                if let Some(callsign) = &telemetry.callsign {
                    self.callsign = Some(callsign.clone());
                } else {
                    telemetry.callsign.clone_from(&self.callsign);
                }
                out.events.push(DecoderEvent::of(&telemetry));
            }
            Payload::Ssdv => {
                let mut packet = *frame;
                match self.checker.check(&mut packet) {
                    Ok(checked) => {
                        if self.ssdv_callsign != Some(checked.header.callsign) {
                            self.ssdv_callsign = Some(checked.header.callsign);
                            self.callsign = Some(checked.header.callsign());
                        }
                        self.picture.place(&packet, &checked.header, mode, now, out);
                    }
                    Err(_) => self.picture.frames_failed += 1,
                }
            }
            Payload::Idle | Payload::Other => {}
        }
    }
}

/// The symbols of the lane that last decoded a packet, for the scope.
struct Scope {
    heard: usize,
    count: usize,
    tones: Mapping,
    levels: Vec<f32>,
}

impl Scope {
    fn hear(&mut self, lane: usize) {
        if lane != self.heard {
            self.heard = lane;
            self.levels.clear();
        }
    }

    fn keep(&mut self, lane: usize, symbol: f32) {
        if lane != self.heard || self.levels.len() == self.levels.capacity() {
            return;
        }
        self.count += 1;
        if self.count >= TAP_EVERY {
            self.count = 0;
            self.levels.push(symbol);
        }
    }
}

pub struct WenetChannel {
    front: FrontEnd,
    lanes: [Lane; 2],
    sink: Sink,
    scope: Scope,
    samples: u64,
    closing: bool,
}

impl WenetChannel {
    fn restart(&mut self) {
        self.front.reset();
        for lane in &mut self.lanes {
            lane.deframer.reset();
        }
        self.closing = true;
    }

    fn run_chunk(&mut self, chunk: &[Complex<f32>], out: &mut ChannelOutputs) {
        let wanted = out.symbols.wanted();
        self.scope.levels.clear();
        let audio = self.front.demodulate(chunk);
        for (offset, &sample) in audio.iter().enumerate() {
            for (index, lane) in self.lanes.iter_mut().enumerate() {
                let Some(symbol) = lane.clock.push(sample) else {
                    continue;
                };
                if wanted {
                    self.scope.keep(index, symbol);
                }
                if let Some(outcome) = lane.deframer.push(symbol) {
                    let now = self.samples + offset as u64;
                    match outcome {
                        Outcome::Decoded { .. } => {
                            self.scope.hear(index);
                            self.sink
                                .on_packet(lane.mode, lane.deframer.packet(), now, out);
                        }
                        Outcome::Failed => self.sink.picture.frames_failed += 1,
                    }
                }
            }
        }
        if wanted {
            self.tap_symbols(out);
        }
        self.samples += chunk.len() as u64;
        self.sink.picture.progress(self.samples, out);
        self.check_signal(out);
    }

    fn tap_symbols(&self, out: &mut ChannelOutputs) {
        let scope = &self.scope;
        out.symbols.levels(
            &scope.levels,
            &[],
            &scope.tones,
            self.lanes[scope.heard].mode.baud(),
            self.front.offset_hz(),
        );
    }

    fn check_signal(&mut self, out: &mut ChannelOutputs) {
        let picture = &mut self.sink.picture;
        let silent = self.samples.saturating_sub(picture.last_packet);
        if picture.assembler.header().is_some() && (silent as f64) >= SIGNAL_LOSS_S * INPUT_RATE_HZ
        {
            picture.finish(out);
        }
    }
}

impl NativeChannel for WenetChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        sdrmm_decoder::settings_of::<WenetParams>(&settings)?;
        let code = frame::code();
        let lane = |mode: WenetMode| Lane {
            mode,
            clock: symbol_clock(INPUT_RATE_HZ, mode),
            deframer: Deframer::new(mode, code.parity_check()),
        };
        Ok(Self {
            front: FrontEnd::new(INPUT_RATE_HZ),
            lanes: [lane(WenetMode::Uart), lane(WenetMode::I2s)],
            sink: Sink {
                checker: PacketChecker::new(),
                picture: Picture::new(),
                callsign: None,
                ssdv_callsign: None,
            },
            scope: Scope {
                heard: 0,
                count: 0,
                tones: Mapping::natural(2),
                levels: Vec::with_capacity(CHUNK),
            },
            samples: 0,
            closing: false,
        })
    }
}

impl Channel for WenetChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        sdrmm_decoder::settings_of::<WenetParams>(&settings)?;
        Ok(())
    }

    fn retuned(&mut self) {
        self.restart();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        if self.closing {
            self.closing = false;
            self.sink.picture.finish(out);
        }
        for chunk in iq.chunks(CHUNK) {
            self.run_chunk(chunk, out);
        }
    }
}
