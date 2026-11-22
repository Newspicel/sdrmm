//! HRPT in L band. Meteor-M and NOAA send residual-carrier PM at 665.4 kbit/s split-phase:
//! Meteor-M carries the MSU-MR imager in CADUs, NOAA the AVHRR in ten-bit minor frames. `MetOp`
//! sends AHRPT, coded QPSK with the AVHRR in CCSDS packets.

use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{Control, KindSpec, SettingField};
mod model;
#[cfg(test)]
pub(crate) mod synth;
pub use model::*;
pub(crate) mod image;
pub(crate) mod meteor;
pub(crate) mod metop;
pub(crate) mod noaa;

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use image::{Imagery, Placement, Scan};
use meteor::Pushed;
use num_complex::Complex;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, DecodedImage, NativeChannel, check_rate,
};
use sdrmm_modem::{cpm::Mapping, split_phase::SplitPhaseDemod};
use sdrmm_wire::{
    ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind, HzRange,
    Modulation, SignalSignature,
};

pub const BIT_RATE: f64 = 665_400.0;
/// Four samples a bit, two a chip.
pub(crate) const INPUT_RATE_HZ: f64 = 4.0 * BIT_RATE;
const CARRIER_BW_HZ: f64 = 1_500.0;
const FILTER_TAPS: usize = 63;
const CHUNK: usize = 16_384;
const SIGNAL_LOSS_S: f64 = 2.0;
const MIN_COMPLETE_LINES: u16 = 32;
/// A picture redraws each time this many new lines have come in.
const REDRAW_LINES: u16 = 16;
const SOURCE: &str = "hrpt";
/// One split-phase bit in this many reaches the scope, some fourteen thousand a second.
const BIT_TAP_EVERY: usize = 48;
/// One `MetOp` QPSK symbol in this many reaches the scope, some eighteen thousand a second.
const SYMBOL_TAP_EVERY: usize = 128;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "hrpt".to_owned(),
    name: "HRPT".to_owned(),
    summary: "Meteor-M, NOAA and MetOp weather pictures in L band".to_owned(),
    family: DecoderFamily::WEATHER,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: 2_400_000.0,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    has_video: true,
    emits: vec![EventKind::named("hrpt")],
    ..ChannelDescriptor::default()
});

fn hrpt_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "HRPT",
            &[Modulation::Psk2],
            HzRange::new(1_800_000.0, 3_000_000.0),
            "665.4 kbit/s split-phase PM: a carrier between two wide lobes",
        )
        .on(&[HzRange::mhz(1_695.0, 1_710.0)]),
        SignalSignature::new(
            "MetOp AHRPT",
            &[Modulation::Psk4],
            HzRange::new(3_000_000.0, 4_000_000.0),
            "2.33 Msym/s QPSK, flat 3.5 MHz wide",
        )
        .on(&[HzRange::mhz(1_700.0, 1_708.0)]),
    ]
}

pub(crate) fn hrpt_decoder() -> Native<HrptParams> {
    Native::new::<HrptChannel>(|p: &HrptParams| occupied_band(p.satellite))
        .filter(|p| Ok(channel_filter(p.satellite)))
        .input_rate(|p| input_rate(p.satellite))
        .home(1_700_000_000.0)
        .signatures(hrpt_signatures)
        .form(hrpt_form)
        .kinds(hrpt_kinds)
}

pub(crate) fn hrpt_form() -> Vec<SettingField> {
    vec![
        SettingField::new("satellite", "Satellite", Control::choice::<HrptSatellite>())
            .title("Meteor-M 1700 MHz, NOAA 1698 or 1707, MetOp 1701.3 or 1707"),
        SettingField::new("picture", "Picture", Control::choice::<HrptPicture>())
            .title("Auto shows visible by day and infrared at night"),
    ]
}

fn hrpt_kinds() -> Vec<KindSpec> {
    vec![model::hrpt_kind()]
}

pub(crate) fn input_rate(satellite: HrptSatellite) -> f64 {
    match satellite {
        HrptSatellite::Meteor | HrptSatellite::Noaa => INPUT_RATE_HZ,
        HrptSatellite::Metop => metop::INPUT_RATE_HZ,
    }
}

pub(crate) fn occupied_band(satellite: HrptSatellite) -> (f64, f64) {
    let width = match satellite {
        HrptSatellite::Meteor | HrptSatellite::Noaa => DESCRIPTOR.bandwidth_hz,
        HrptSatellite::Metop => metop::BANDWIDTH_HZ,
    };
    (-width / 2.0, width / 2.0)
}

pub(crate) fn channel_filter(satellite: HrptSatellite) -> FilterSpec {
    let (_, half) = occupied_band(satellite);
    FilterSpec::lowpass(FILTER_TAPS, half)
}

fn params(settings: &ChannelSettings) -> Result<HrptParams, ChannelError> {
    sdrmm_decoder::settings_of::<HrptParams>(settings)
}

enum Downlink {
    Meteor(meteor::Link),
    Noaa(noaa::Link),
    Metop(Box<metop::Link>),
}

impl Downlink {
    fn new(satellite: HrptSatellite) -> Result<Self, ChannelError> {
        Ok(match satellite {
            HrptSatellite::Meteor => Self::Meteor(meteor::Link::new()),
            HrptSatellite::Noaa => Self::Noaa(noaa::Link::new()),
            HrptSatellite::Metop => Self::Metop(Box::new(metop::Link::new()?)),
        })
    }

    fn geometry(satellite: HrptSatellite) -> (usize, f64) {
        match satellite {
            HrptSatellite::Meteor => (meteor::WIDTH, meteor::LINE_MS),
            HrptSatellite::Noaa => (noaa::WIDTH, noaa::LINE_MS),
            HrptSatellite::Metop => (metop::WIDTH, metop::LINE_MS),
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Meteor(link) => link.reset(),
            Self::Noaa(link) => link.reset(),
            Self::Metop(link) => link.reset(),
        }
    }

    /// Takes in `chunk`: the split-phase downlinks through `demod` into `soft`, `MetOp` whole.
    fn feed(&mut self, chunk: &[Complex<f32>], demod: &mut SplitPhaseDemod, soft: &mut Vec<f32>) {
        soft.clear();
        match self {
            Self::Metop(link) => link.feed(chunk),
            Self::Meteor(_) | Self::Noaa(_) => demod.process(chunk, soft),
        }
    }

    /// The next completed frame, reading the split-phase downlinks' bits from `soft` at `at`.
    fn next(&mut self, soft: &[f32], at: &mut usize) -> Option<Pushed> {
        if let Self::Metop(link) = self {
            return link.next();
        }
        while let Some(&value) = soft.get(*at) {
            *at += 1;
            let pushed = match self {
                Self::Meteor(link) => link.push(value > 0.0),
                Self::Noaa(link) => link.push(value > 0.0),
                Self::Metop(_) => Pushed::default(),
            };
            if pushed.cadu.is_some() {
                return Some(pushed);
            }
        }
        None
    }

    fn dropped(&self) -> u32 {
        match self {
            Self::Meteor(link) => link.dropped(),
            Self::Noaa(link) => link.frames_dropped(),
            Self::Metop(link) => link.dropped(),
        }
    }

    fn scan(&self, out: &mut Scan) {
        match self {
            Self::Meteor(link) => meteor::scan(link.line(), out),
            Self::Noaa(link) => noaa::scan(link.frame(), out),
            Self::Metop(link) => metop::scan(link.line(), out),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct FrameStats {
    frames: u32,
    damaged: u32,
}

struct Picture {
    imagery: Imagery,
    satellite: HrptSatellite,
    stats: FrameStats,
    seq: u32,
    started: u64,
    last_frame: u64,
    drawn: u16,
}

impl Picture {
    fn new(satellite: HrptSatellite) -> Self {
        let (width, line_ms) = Downlink::geometry(satellite);
        Self {
            imagery: Imagery::new(width, line_ms),
            satellite,
            stats: FrameStats::default(),
            seq: 0,
            started: 0,
            last_frame: 0,
            drawn: 0,
        }
    }

    fn clear(&mut self) {
        let (width, line_ms) = Downlink::geometry(self.satellite);
        self.imagery.reset(width, line_ms);
        self.stats = FrameStats::default();
        self.drawn = 0;
    }

    fn add(
        &mut self,
        scan: &Scan,
        now: u64,
        picture: HrptPicture,
        rate: f64,
        out: &mut ChannelOutputs,
    ) {
        if self.imagery.is_empty() {
            self.started = now;
        }
        if self.imagery.add(scan) == Placement::Full {
            self.finish(picture, true, rate, out);
            self.started = now;
            self.imagery.add(scan);
        }
    }

    fn progress(&mut self, picture: HrptPicture, out: &mut ChannelOutputs) {
        let lines = self.imagery.lines();
        if lines >= self.drawn.saturating_add(REDRAW_LINES) {
            self.drawn = lines;
            out.video.push(self.imagery.snapshot(picture));
        }
    }

    fn finish(
        &mut self,
        picture: HrptPicture,
        complete: bool,
        rate: f64,
        out: &mut ChannelOutputs,
    ) {
        if self.imagery.is_empty() {
            self.clear();
            return;
        }
        self.seq = self.seq.wrapping_add(1);
        let lines = self.imagery.lines();
        let elapsed = self.last_frame.saturating_sub(self.started);
        let shown = self.imagery.resolve(picture);
        let snapshot = self.imagery.snapshot(picture);
        out.events.push(DecoderEvent::of(&HrptImage {
            seq: self.seq,
            satellite: self.satellite,
            picture: shown,
            width: self.imagery.width(),
            lines,
            complete,
            duration_ms: (elapsed as f64 * 1_000.0 / rate) as u32,
            first_line: self.imagery.first_line(),
            frames: self.stats.frames,
            frames_damaged: self.stats.damaged,
        }));
        out.video.push(snapshot.clone());
        out.images.push(DecodedImage {
            source: SOURCE.into(),
            mode: format!("{} HRPT {}", self.satellite.label(), shown.label()),
            complete,
            lines,
            picture: snapshot,
        });
        self.clear();
    }
}

pub struct HrptChannel {
    params: HrptParams,
    rate: f64,
    demod: SplitPhaseDemod,
    downlink: Downlink,
    picture: Picture,
    scan: Box<Scan>,
    soft: Vec<f32>,
    bits: Mapping,
    levels: Vec<f32>,
    points: Vec<Complex<f32>>,
    samples: u64,
    closing: bool,
    /// Frames lost before this link: kept on position alone, or dropped by a link since replaced.
    lost: u32,
}

impl HrptChannel {
    fn restart(&mut self) {
        self.demod.reset();
        self.downlink.reset();
        self.closing = true;
    }

    /// Closes the picture cut short, then starts the next for the satellite now chosen.
    fn close(&mut self, out: &mut ChannelOutputs) {
        self.picture
            .finish(self.params.picture, false, self.rate, out);
        self.picture.satellite = self.params.satellite;
        self.picture.clear();
    }

    fn take_frames(&mut self, out: &mut ChannelOutputs) {
        let mut at = 0;
        while let Some(pushed) = self.downlink.next(&self.soft, &mut at) {
            let Some(framed) = pushed.cadu else {
                continue;
            };
            let stats = &mut self.picture.stats;
            stats.frames = stats.frames.saturating_add(1);
            if framed.flywheel {
                stats.damaged = stats.damaged.saturating_add(1);
                self.lost = self.lost.saturating_add(1);
            }
            self.picture.last_frame = self.samples;
            if pushed.line {
                self.downlink.scan(&mut self.scan);
                self.picture.add(
                    &self.scan,
                    self.samples,
                    self.params.picture,
                    self.rate,
                    out,
                );
            }
        }
    }

    fn check_signal(&mut self, out: &mut ChannelOutputs) {
        let silent = self.samples.saturating_sub(self.picture.last_frame);
        if (silent as f64) < SIGNAL_LOSS_S * self.rate {
            return;
        }
        let complete = self.picture.imagery.lines() >= MIN_COMPLETE_LINES;
        self.picture
            .finish(self.params.picture, complete, self.rate, out);
        self.picture.last_frame = self.samples;
    }

    fn tap_symbols(&mut self, out: &mut ChannelOutputs) {
        if !out.symbols.wanted() {
            return;
        }
        if let Downlink::Metop(link) = &self.downlink {
            self.points.clear();
            self.points
                .extend(link.symbols().iter().step_by(SYMBOL_TAP_EVERY).copied());
            out.symbols.linear(
                &self.points,
                link.constellation(),
                metop::SYMBOL_RATE,
                link.frequency_offset_hz(),
            );
            return;
        }
        self.levels.clear();
        self.levels
            .extend(self.soft.iter().step_by(BIT_TAP_EVERY).copied());
        out.symbols.levels(
            &self.levels,
            &[],
            &self.bits,
            BIT_RATE,
            self.demod.carrier().offset_hz(),
        );
    }

    fn run_chunk(&mut self, chunk: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.downlink.feed(chunk, &mut self.demod, &mut self.soft);
        self.tap_symbols(out);
        self.samples += chunk.len() as u64;
        self.take_frames(out);
        self.picture.progress(self.params.picture, out);
        self.check_signal(out);
    }
}

impl NativeChannel for HrptChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?;
        let rate = input_rate(params.satellite);
        check_rate(ctx, &DESCRIPTOR, rate)?;
        Ok(Self {
            params,
            rate,
            demod: SplitPhaseDemod::new(INPUT_RATE_HZ, BIT_RATE, CARRIER_BW_HZ),
            downlink: Downlink::new(params.satellite)?,
            picture: Picture::new(params.satellite),
            scan: Box::new(Scan::new()),
            soft: Vec::with_capacity(CHUNK),
            bits: Mapping::natural(2),
            levels: Vec::with_capacity(CHUNK),
            points: Vec::with_capacity(CHUNK),
            samples: 0,
            closing: false,
            lost: 0,
        })
    }
}

impl Channel for HrptChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        check_rate(
            ChannelCtx {
                input_rate: self.rate,
            },
            &DESCRIPTOR,
            input_rate(params.satellite),
        )?;
        if params.satellite != self.params.satellite {
            self.lost = self.lost.saturating_add(self.downlink.dropped());
            self.downlink = Downlink::new(params.satellite)?;
            self.restart();
        } else if params.picture != self.params.picture {
            self.picture.drawn = 0;
        }
        self.params = params;
        Ok(())
    }

    fn retuned(&mut self) {
        self.restart();
    }

    fn rejected(&self) -> u32 {
        self.lost.saturating_add(self.downlink.dropped())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        if self.closing {
            self.closing = false;
            self.close(out);
        }
        for chunk in iq.chunks(CHUNK) {
            self.run_chunk(chunk, out);
        }
    }
}
