use sdrmm_decoder::registry::Native;
use sdrmm_wire::{
    ANY_MODULATION, ChannelDescriptor, ChannelSettings, Control, DecoderEvent, DecoderFamily,
    EventKind, FilterSpec, HzRange, KindSpec, SettingField, SignalSignature,
};
mod downlink;
mod fec;
mod model;
mod receiver;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
mod uplink;
pub use model::*;

use std::{f64::consts::TAU, sync::LazyLock};

use num_complex::Complex;
use sdrmm_decoder::aircraft::adsb_kind;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use sdrmm_modem::cpm::Mapping;
use sdrmm_modem::packet::RecentFrames;

use self::receiver::{Heard, Receiver, SAMPLES_PER_BIT, Sink};

const BIT_RATE_HZ: f64 = 1_041_667.0;
/// Two samples a bit, the rate dump978 and most UAT receivers run at.
pub(crate) const INPUT_RATE_HZ: f64 = 2_083_334.0;
const BANDWIDTH_HZ: f64 = 1_300_000.0;
const HOME_HZ: f64 = 978_000_000.0;
/// A ground station repeats a report every few minutes; it is passed on once in this window.
const REPEAT_HOLD_S: f64 = 600.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "uat".to_owned(),
    name: "UAT (978 MHz)".to_owned(),
    summary: "US aircraft positions and FIS-B weather on 978 MHz".to_owned(),
    family: DecoderFamily::AVIATION,
    verification: sdrmm_wire::Verification::ReferenceFixture,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    emits: vec![EventKind::named("adsb"), EventKind::named("fisb")],
    ..ChannelDescriptor::default()
});

pub struct UatChannel {
    fisb: bool,
    receiver: Receiver,
    recent: RecentFrames,
    reports: Vec<FisbReport>,
    mapping: Mapping,
}

fn params(settings: &ChannelSettings) -> Result<UatParams, ChannelError> {
    sdrmm_decoder::settings_of::<UatParams>(settings)
}

impl NativeChannel for UatChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let p = params(&settings)?;
        Ok(Self {
            fisb: p.fisb,
            receiver: Receiver::new(),
            recent: RecentFrames::new((REPEAT_HOLD_S * INPUT_RATE_HZ) as u64),
            reports: Vec::new(),
            mapping: Mapping::natural(2),
        })
    }
}

impl Channel for UatChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.fisb = params(&settings)?.fisb;
        Ok(())
    }

    fn retuned(&mut self) {
        self.receiver.reset();
        self.recent.clear();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let mut sink = EventSink {
            fisb: self.fisb,
            recent: &mut self.recent,
            reports: &mut self.reports,
            events: &mut out.events,
        };
        self.receiver.set_tapping(out.symbols.wanted());
        self.receiver.process(iq, &mut sink);
        self.recent.advance(iq.len());
        let offset_hz = f64::from(self.receiver.centre()) * INPUT_RATE_HZ / TAU;
        if !self.receiver.soft().is_empty() {
            out.symbols.levels(
                self.receiver.soft(),
                &[],
                &self.mapping,
                BIT_RATE_HZ,
                offset_hz,
            );
        }
    }

    fn rejected(&self) -> u32 {
        self.receiver.rejected()
    }
}

struct EventSink<'a> {
    fisb: bool,
    recent: &'a mut RecentFrames,
    reports: &'a mut Vec<FisbReport>,
    events: &'a mut Vec<DecoderEvent>,
}

impl Sink for EventSink<'_> {
    fn heard(&mut self, heard: Heard<'_>) {
        match heard {
            Heard::Downlink { frame, level, .. } => {
                let mut message = downlink::report(frame.bytes());
                message.signal_level = Some(signal_level(level));
                self.events.push(DecoderEvent::of(&message));
            }
            Heard::Uplink { data, .. } if self.fisb => {
                for apdu in uplink::apdus(data) {
                    uplink::reports(&apdu, self.reports);
                }
                for report in self.reports.drain(..) {
                    if self.recent.first(report.text.as_bytes()) {
                        self.events.push(DecoderEvent::of(&report));
                    }
                }
            }
            Heard::Uplink { .. } => {}
        }
    }
}

/// The frame's RMS amplitude on the 0 to 255 scale the aircraft report shares with Beast.
fn signal_level(power: f32) -> u8 {
    (power.sqrt().clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(crate) fn occupied_band() -> (f64, f64) {
    let half = BANDWIDTH_HZ / 2.0;
    (-half, half)
}

const UAT: &[HzRange] = &[HzRange::mhz(977.0, 979.0)];

fn uat_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "UAT",
            ANY_MODULATION,
            HzRange::new(800_000.0, 1_600_000.0),
            "1 Mbit/s FSK bursts on 978 MHz: aircraft and FIS-B ground stations",
        )
        .bursts(HzRange::new(0.2, 5.0))
        .on(UAT),
    ]
}

pub(crate) fn uat_decoder() -> Native<UatParams> {
    let decoder = Native::new::<UatChannel>(|_| occupied_band())
        .filter(|_| Ok(FilterSpec::Passthrough))
        .home(HOME_HZ)
        .signatures(uat_signatures)
        .form(uat_form)
        .kinds(uat_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn uat_form() -> Vec<SettingField> {
    vec![
        SettingField::new("fisb", "FIS-B", Control::Toggle)
            .title("Read weather and NOTAM text from ground stations"),
    ]
}

fn uat_kinds() -> Vec<KindSpec> {
    vec![adsb_kind(), fisb_kind()]
}

const _: () = assert!(INPUT_RATE_HZ as usize == SAMPLES_PER_BIT * BIT_RATE_HZ as usize);

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
