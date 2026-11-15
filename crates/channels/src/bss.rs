use crate::registry::Native;
use sdrmm_wire::{HzRange, KindSpec, Modulation, SettingField, SignalSignature};
mod frame;
pub(crate) use frame::Compose;
mod model;
pub use model::*;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::fixture;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_lowpass, hdlc_fcs_ok, hdlc_repair};
use sdrmm_modem::packet::{PacketMode, PacketReceiver, RecentFrames};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate,
};

const CHANNEL_TAPS: usize = 65;
const FCS_BYTES: usize = 2;
/// The marker and one field with a one-byte value.
const MIN_FRAME_BYTES: usize = 1 + 3 + FCS_BYTES;
const MAX_FRAME_BYTES: usize = 512;
const DEDUP_SAMPLES: u64 = 48_000;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "bss".to_owned(),
    name: "BSS".to_owned(),
    summary: "Benshi handheld chat, positions and calls".to_owned(),
    family: DecoderFamily::AMATEUR,
    bandwidth_hz: 12_500.0,
    input_rate_hz: 48_000.0,
    has_audio: false,
    emits: vec![EventKind::named("bss")],
    ..ChannelDescriptor::default()
});

pub struct BssChannel {
    receiver: PacketReceiver,
    listener: Listener,
}

struct Listener {
    recent: RecentFrames,
    scratch: Vec<u8>,
    rejected: u32,
}

fn params(settings: &ChannelSettings) -> Result<BssParams, ChannelError> {
    let p: BssParams = crate::settings_of(settings)?;
    let rate = DESCRIPTOR.input_rate_hz;
    if p.bandwidth_hz.is_finite() && p.bandwidth_hz > 0.0 && p.bandwidth_hz < rate {
        Ok(p)
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "bss bandwidth must be in (0, {rate}) Hz, got {}",
            p.bandwidth_hz
        )))
    }
}

fn occupied_band(p: &BssParams) -> (f64, f64) {
    let half = p.bandwidth_hz / 2.0;
    (-half, half)
}

fn channel_filter(p: &BssParams) -> Result<ChannelFilter, ChannelError> {
    let (_, half) = occupied_band(p);
    Ok(ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(CHANNEL_TAPS, half / DESCRIPTOR.input_rate_hz),
        1,
    )))
}

fn body(frame: &[u8]) -> &[u8] {
    &frame[..frame.len().saturating_sub(FCS_BYTES)]
}

impl Listener {
    /// Copies a frame that opens with the BSS marker into `scratch` with its FCS checked or
    /// repaired. Repair only runs on frames whose fields already line up, so noise does not buy
    /// a burst of CRC trials.
    fn checked(&mut self, frame: &[u8]) -> bool {
        if frame.first() != Some(&frame::MARKER) {
            return false;
        }
        let intact = hdlc_fcs_ok(frame);
        if !intact && !frame::lines_up(body(frame)) {
            return false;
        }
        self.scratch.clear();
        self.scratch.extend_from_slice(frame);
        intact || hdlc_repair(&mut self.scratch)
    }

    fn hear(&mut self, frame: &[u8], out: &mut ChannelOutputs) {
        if !self.checked(frame) || !self.recent.first(&self.scratch) {
            return;
        }
        match frame::parse(body(&self.scratch)) {
            Some(packet) => out.events.push(DecoderEvent::of(&packet)),
            None => self.rejected = self.rejected.saturating_add(1),
        }
    }
}

impl NativeChannel for BssChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        params(&settings)?;
        Ok(Self {
            receiver: PacketReceiver::new(
                PacketMode::Afsk1200,
                ctx.input_rate,
                &(MIN_FRAME_BYTES..=MAX_FRAME_BYTES),
            ),
            listener: Listener {
                recent: RecentFrames::new(DEDUP_SAMPLES),
                scratch: Vec::with_capacity(MAX_FRAME_BYTES),
                rejected: 0,
            },
        })
    }
}

impl ChannelRx for BssChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        params(&settings).map(|_| ())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let Self { receiver, listener } = self;
        listener.recent.advance(iq.len());
        receiver.process(iq, |frame| listener.hear(frame, out));
    }

    fn rejected(&self) -> u32 {
        self.listener.rejected
    }
}

/// Where Benshi handhelds key up: the 2 m and 70 cm amateur bands and GMRS.
const BSS_BANDS: &[HzRange] = &[
    HzRange::mhz(144.0, 148.0),
    HzRange::mhz(420.0, 450.0),
    HzRange::mhz(462.5, 467.8),
];

fn bss_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "BSS (AFSK over FM)",
            &[Modulation::Fm, Modulation::Fsk2],
            HzRange::new(6_000.0, 20_000.0),
            "audio tones inside an FM channel, at packet speed",
        )
        .symbol_rate(HzRange::new(1_000.0, 2_400.0))
        .deviation(HzRange::new(1_500.0, 5_000.0))
        .on(BSS_BANDS)
        .prior(0.8),
    ]
}

fn bss_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(EventKind::named("bss"))
}

pub(crate) fn bss_decoder() -> Native<BssParams> {
    Native::new::<BssChannel>(occupied_band)
        .filter(channel_filter)
        .signatures(bss_signatures)
        .trial(bss_trial)
        .form(bss_form)
        .kinds(bss_kinds)
}

fn bss_form() -> Vec<SettingField> {
    vec![SettingField::bandwidth(&[12_500.0, 25_000.0])]
}

fn bss_kinds() -> Vec<KindSpec> {
    vec![bss_kind()]
}
