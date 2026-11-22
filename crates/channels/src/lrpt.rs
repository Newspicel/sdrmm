use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{Control, KindSpec, SettingField};
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;
mod demod;
pub(crate) mod image;
pub(crate) mod jpeg;
pub(crate) mod link;
pub(crate) fn lrpt_decoder() -> Native<LrptParams> {
    let decoder = Native::new::<LrptChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(137_900_000.0)
        .form(lrpt_form)
        .kinds(lrpt_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn lrpt_form() -> Vec<SettingField> {
    vec![
        SettingField::new("mode", "Mode", Control::choice::<LrptMode>())
            .title("Meteor-M downlink mode"),
    ]
}

fn lrpt_kinds() -> Vec<KindSpec> {
    vec![crate::lrpt::lrpt_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use demod::Demod;
use image::{Imagery, Placement};
use num_complex::Complex;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};

use link::{IDLE_APID, PacketHeader};
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, DecodedImage, NativeChannel,
    check_input_rate,
};
use sdrmm_modem::ccsds::{Deframer, Depacketizer, FrameOutcome};
use sdrmm_modem::constellation::{Constellation, tables};

pub(crate) const INPUT_RATE_HZ: f64 = 288_000.0;
const FILTER_TAPS: usize = 127;
const CHUNK: usize = 4_096;
const SIGNAL_LOSS_S: f64 = 2.0;
const MIN_COMPLETE_LINES: u16 = 16;
const SOURCE: &str = "lrpt";
/// One decision point in this many reaches the scope, some fifteen thousand a second.
const TAP_EVERY: usize = 5;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "lrpt".to_owned(),
    name: "Meteor LRPT".to_owned(),
    summary: "Meteor-M weather satellite pictures".to_owned(),
    family: DecoderFamily::WEATHER,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: 140_000.0,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    has_video: true,
    emits: vec![EventKind::named("lrpt")],
    ..ChannelDescriptor::default()
});

pub(crate) fn occupied_band() -> (f64, f64) {
    let half = DESCRIPTOR.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter() -> FilterSpec {
    let (_, half) = occupied_band();
    FilterSpec::lowpass(FILTER_TAPS, half)
}

fn params(settings: &ChannelSettings) -> Result<LrptParams, ChannelError> {
    sdrmm_decoder::settings_of::<LrptParams>(settings)
}

#[derive(Clone, Copy, Debug, Default)]
struct FrameStats {
    frames: u32,
    corrected: u32,
    failed: u32,
}

impl FrameStats {
    fn count(&mut self, outcome: FrameOutcome) {
        self.frames = self.frames.saturating_add(1);
        if outcome.failed {
            self.failed = self.failed.saturating_add(1);
        } else if outcome.corrected > 0 {
            self.corrected = self.corrected.saturating_add(1);
        }
    }
}

struct Picture {
    imagery: Imagery,
    stats: FrameStats,
    seq: u32,
    started: u64,
    last_frame: u64,
    shown_rows: usize,
}

impl Picture {
    fn new() -> Self {
        Self {
            imagery: Imagery::new(),
            stats: FrameStats::default(),
            seq: 0,
            started: 0,
            last_frame: 0,
            shown_rows: 0,
        }
    }

    fn clear(&mut self) {
        self.imagery.reset();
        self.stats = FrameStats::default();
        self.shown_rows = 0;
    }

    fn place(&mut self, packet: &[u8], now: u64, mode: LrptMode, out: &mut ChannelOutputs) {
        let Some(header) = PacketHeader::parse(packet).filter(|header| header.apid != IDLE_APID)
        else {
            return;
        };
        let was_empty = self.imagery.is_empty();
        if self.imagery.place(packet, header.apid, header.sequence) == Placement::Full {
            self.finish(mode, true, out);
            self.imagery.place(packet, header.apid, header.sequence);
        }
        if was_empty && !self.imagery.is_empty() {
            self.started = now;
        }
    }

    fn progress(&mut self, out: &mut ChannelOutputs) {
        if self.imagery.rows() > self.shown_rows {
            self.shown_rows = self.imagery.rows();
            out.video.push(self.imagery.snapshot());
        }
    }

    fn finish(&mut self, mode: LrptMode, complete: bool, out: &mut ChannelOutputs) {
        if self.imagery.is_empty() {
            self.clear();
            return;
        }
        self.seq = self.seq.wrapping_add(1);
        let lines = self.imagery.lines();
        let elapsed = self.last_frame.saturating_sub(self.started);
        let picture = self.imagery.snapshot();
        out.events.push(DecoderEvent::of(&LrptImage {
            seq: self.seq,
            mode,
            width: picture.width,
            lines,
            complete,
            duration_ms: (elapsed as f64 * 1_000.0 / INPUT_RATE_HZ) as u32,
            apids: self.imagery.apids(),
            frames: self.stats.frames,
            frames_corrected: self.stats.corrected,
            frames_failed: self.stats.failed,
            packets_lost: self.imagery.packets_lost(),
        }));
        out.video.push(picture.clone());
        out.images.push(DecodedImage {
            source: SOURCE.into(),
            mode: self.imagery.label(),
            complete,
            lines,
            picture,
        });
        self.clear();
    }
}

pub struct LrptChannel {
    mode: LrptMode,
    demod: Demod,
    qpsk: Constellation,
    deframer: Deframer,
    depacketizer: Depacketizer,
    picture: Picture,
    soft: Vec<i16>,
    samples: u64,
    closing: bool,
    /// Frames Reed-Solomon could not repair, over the channel's life.
    frames_failed: u32,
}

impl LrptChannel {
    fn restart(&mut self) {
        self.demod.reset();
        self.deframer.reset();
        self.depacketizer.reset();
        self.closing = true;
    }

    fn on_frame(&mut self, outcome: FrameOutcome, out: &mut ChannelOutputs) {
        let picture = &mut self.picture;
        picture.stats.count(outcome);
        if outcome.failed {
            self.frames_failed = self.frames_failed.saturating_add(1);
            self.depacketizer.reset();
            return;
        }
        picture.last_frame = self.samples;
        self.depacketizer.load(self.deframer.vcdu());
        while let Some(packet) = self.depacketizer.next_packet() {
            picture.place(packet, self.samples, self.mode, out);
        }
    }

    fn tap_symbols(&self, out: &mut ChannelOutputs) {
        out.symbols.linear(
            self.demod.points(),
            &self.qpsk,
            self.mode.symbol_rate(),
            self.demod.frequency_offset_cycles_per_sample() * INPUT_RATE_HZ,
        );
    }

    fn check_signal(&mut self, out: &mut ChannelOutputs) {
        let silent = self.samples.saturating_sub(self.picture.last_frame);
        if (silent as f64) < SIGNAL_LOSS_S * INPUT_RATE_HZ {
            return;
        }
        let complete = self.picture.imagery.lines() >= MIN_COMPLETE_LINES;
        self.picture.finish(self.mode, complete, out);
        self.picture.last_frame = self.samples;
    }

    fn run_chunk(&mut self, chunk: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.soft.clear();
        let wanted = out.symbols.wanted();
        self.demod.tap(if wanted { TAP_EVERY } else { 0 });
        self.demod.process(chunk, &mut self.soft);
        if wanted {
            self.tap_symbols(out);
        }
        self.samples += chunk.len() as u64;
        self.deframer.push(&self.soft);
        while let Some(outcome) = self.deframer.next_frame() {
            self.on_frame(outcome, out);
        }
        self.picture.progress(out);
        self.check_signal(out);
    }
}

impl NativeChannel for LrptChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let mode = params(&settings)?.mode;
        Ok(Self {
            mode,
            demod: Demod::new(mode, INPUT_RATE_HZ),
            qpsk: tables::psk_rotated(4, std::f64::consts::FRAC_PI_4)
                .map_err(|error| ChannelError::InvalidSettings(format!("LRPT QPSK: {error}")))?,
            deframer: link::deframer(),
            depacketizer: link::depacketizer(),
            picture: Picture::new(),
            soft: Vec::with_capacity(2 * CHUNK),
            samples: 0,
            closing: false,
            frames_failed: 0,
        })
    }
}

impl Channel for LrptChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let mode = params(&settings)?.mode;
        if mode != self.mode {
            self.mode = mode;
            self.demod = Demod::new(mode, INPUT_RATE_HZ);
            self.restart();
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.restart();
    }

    fn rejected(&self) -> u32 {
        self.frames_failed
            .saturating_add(self.depacketizer.dropped())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        if self.closing {
            self.closing = false;
            self.picture.finish(self.mode, false, out);
        }
        for chunk in iq.chunks(CHUNK) {
            self.run_chunk(chunk, out);
        }
    }
}
