use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{ANY_MODULATION, HzRange, SignalSignature};
use sdrmm_wire::{Capability, Control, Hue, KindSpec, SettingField, Symbol};
mod model;
pub use model::*;
mod burst;
mod decode;
mod demod;
const IRIDIUM_BAND: &[HzRange] = &[HzRange::mhz(1_616.0, 1_626.5)];

fn iridium_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "Iridium",
            ANY_MODULATION,
            HzRange::new(25_000.0, 50_000.0),
            "8.3 ms bursts in a 41.67 kHz satellite channel",
        )
        .bursts(HzRange::new(6.0, 10.0))
        .on(IRIDIUM_BAND),
    ]
}

pub(crate) fn iridium_decoder() -> Native<IridiumParams> {
    Native::new::<IridiumChannel>(|p| occupied_band(*p))
        .filter(|p| Ok(channel_filter(*p)))
        .input_rate(|p| input_rate(*p))
        .home(1_621_500_000.0)
        .signatures(iridium_signatures)
        .form(iridium_form)
        .kinds(iridium_kinds)
}

pub(crate) fn iridium_form() -> Vec<SettingField> {
    vec![SettingField::new("span", "Span", Control::choice::<IridiumSpan>()).title("Iridium span")]
}

fn iridium_kinds() -> Vec<KindSpec> {
    vec![
        KindSpec::tagged::<DataLinkMessage>(
            EventKind::named("iridium"),
            "Iridium",
            "Bursts",
            Hue::Blue,
        )
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::DOT)
        .sighting()
        .build(),
    ]
}

#[cfg(test)]
mod encode;
#[cfg(test)]
pub(crate) mod fixture;
mod frame;
mod gsm;
mod iip;
mod ira;
mod itl;
mod itl_tables;
mod lcw;
#[cfg(test)]
mod modulate;
mod ms;
mod mtpos;
mod receiver;
mod rs;
mod sbd;
#[cfg(test)]
mod sensitivity;
#[cfg(test)]
mod tests;
mod u3;
mod voice;
mod wideband;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_aviation::acars::block as acars;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};
use serde::Serialize;
use serde_json::Value;

use self::ira::IridiumFrame;
use self::receiver::{ChannelDecoder, Shown};
use self::wideband::{USABLE_FRACTION, WidebandDecoder};
use sdrmm_decoder::link::{self, DataLinkMessage, Fields, Quality};
use sdrmm_decoder::{Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_rate};
use sdrmm_modem::constellation::{Constellation, tables};

const CHANNEL_RATE: f64 = 250_000.0;
const HALF_BANDWIDTH: f64 = 25_000.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "iridium".to_owned(),
    name: "Iridium bursts".to_owned(),
    summary: "Iridium satellite bursts".to_owned(),
    family: DecoderFamily::UTILITY,
    verification: sdrmm_wire::Verification::ReferenceFixture,
    bandwidth_hz: HALF_BANDWIDTH * 2.0,
    input_rate_hz: CHANNEL_RATE,
    has_audio: false,
    emits: vec![EventKind::named("iridium")],
    ..ChannelDescriptor::default()
});

/// Where an Iridium burst keeps its envelope: the aircraft, or the satellite that sent it.
const LINK: Fields = Fields::station(&["tail", "icao", "sat"]);

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Body<'a, A: Serialize> {
    Acars(&'a A),
    Iridium { kind: &'a str, details: &'a Value },
}

enum Decoder {
    Channel(Box<ChannelDecoder>),
    Wide(Box<WidebandDecoder>),
}

impl Decoder {
    fn new(span: IridiumSpan) -> Result<Self, ChannelError> {
        match span.sample_rate_hz() {
            None => Ok(Self::Channel(Box::new(ChannelDecoder::new()))),
            Some(rate) => Ok(Self::Wide(Box::new(WidebandDecoder::new(rate)?))),
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], frames: &mut Vec<IridiumFrame>) {
        match self {
            Self::Channel(decoder) => decoder.process(iq, frames),
            Self::Wide(decoder) => decoder.process(iq, frames),
        }
    }

    fn rejected(&self) -> u32 {
        match self {
            Self::Channel(decoder) => decoder.rejected(),
            Self::Wide(decoder) => decoder.rejected(),
        }
    }

    fn shown(&self) -> &Shown {
        match self {
            Self::Channel(decoder) => &decoder.shown,
            Self::Wide(decoder) => &decoder.shown,
        }
    }
}

pub struct IridiumChannel {
    span: IridiumSpan,
    decoder: Decoder,
    frames: Vec<IridiumFrame>,
    qpsk: Constellation,
}

fn params(settings: &ChannelSettings) -> Result<IridiumParams, ChannelError> {
    sdrmm_decoder::settings_of::<IridiumParams>(settings)
}

pub(crate) fn input_rate(params: IridiumParams) -> f64 {
    params.span.sample_rate_hz().unwrap_or(CHANNEL_RATE)
}

pub(crate) fn occupied_band(params: IridiumParams) -> (f64, f64) {
    let half = params
        .span
        .sample_rate_hz()
        .map_or(HALF_BANDWIDTH, |rate| rate * USABLE_FRACTION);
    (-half, half)
}

pub(crate) fn channel_filter(params: IridiumParams) -> FilterSpec {
    match params.span {
        IridiumSpan::Channel => FilterSpec::channel(HALF_BANDWIDTH),
        _ => FilterSpec::Passthrough,
    }
}

fn message(frame: &IridiumFrame) -> DataLinkMessage {
    let fec_corrected = frame
        .details
        .get("bch_corrected")
        .and_then(Value::as_u64)
        .and_then(|fixed| u32::try_from(fixed).ok());
    match &frame.acars {
        Some(block) => link::message(
            &Body::Acars(&block.core),
            &LINK,
            Quality {
                crc_ok: block.crc_ok,
                fec_corrected,
                frequency_error_hz: frame.offset_hz,
                ..Quality::default()
            },
            None,
        ),
        None => link::message(
            &Body::<()>::Iridium {
                kind: frame.kind,
                details: &frame.details,
            },
            &LINK,
            Quality {
                crc_ok: frame
                    .details
                    .get("crc_ok")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                fec_corrected,
                frequency_error_hz: frame.offset_hz,
                ..Quality::default()
            },
            None,
        ),
    }
}

impl NativeChannel for IridiumChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?;
        check_rate(ctx, &DESCRIPTOR, input_rate(params))?;
        Ok(Self {
            span: params.span,
            decoder: Decoder::new(params.span)?,
            frames: Vec::new(),
            qpsk: tables::psk(4)
                .map_err(|error| ChannelError::InvalidSettings(error.to_string()))?,
        })
    }
}

impl Channel for IridiumChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        if params(&settings)?.span == self.span {
            Ok(())
        } else {
            Err(ChannelError::InvalidSettings(
                "Iridium span changes need a rebuilt channel".to_owned(),
            ))
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.frames.clear();
        self.decoder.process(iq, &mut self.frames);
        let shown = self.decoder.shown();
        if out.symbols.wanted() && !shown.points.is_empty() {
            out.symbols.linear(
                &shown.points,
                &self.qpsk,
                demod::SYMBOL_RATE,
                shown.offset_hz,
            );
        }
        out.events.extend(
            self.frames
                .iter()
                .map(|frame| DecoderEvent::tagged(EventKind::named("iridium"), &message(frame))),
        );
    }

    fn rejected(&self) -> u32 {
        self.decoder.rejected()
    }
}
