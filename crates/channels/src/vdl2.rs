use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{Capability, Hue, KindSpec, SettingField, Symbol};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
pub use model::*;
mod atn;
mod avlc;
mod decoder;
mod demod;
mod header;
mod interleave;
const VDL2_CHANNELS: &[HzRange] = &[HzRange::mhz(136.7, 137.0)];

fn vdl2_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "VDL Mode 2",
            &[Modulation::Psk4, Modulation::NoiseLike, Modulation::Unknown],
            HzRange::new(15_000.0, 30_000.0),
            "10.5 kbaud D8PSK in an aeronautical data channel",
        )
        .symbol_rate(HzRange::about(10_500.0, 0.05))
        .on(VDL2_CHANNELS),
    ]
}

fn vdl2_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(EventKind::named("vdl2"))
}

pub(crate) fn vdl2_decoder() -> Native<Vdl2Params> {
    Native::new::<Vdl2Channel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(136_975_000.0)
        .signatures(vdl2_signatures)
        .trial(vdl2_trial)
        .form(vdl2_form)
        .kinds(vdl2_kinds)
}

pub(crate) fn vdl2_form() -> Vec<SettingField> {
    Vec::new()
}

fn vdl2_kinds() -> Vec<KindSpec> {
    vec![
        KindSpec::tagged::<DataLinkMessage>(
            EventKind::named("vdl2"),
            "VDL Mode 2",
            "Aircraft",
            Hue::Teal,
        )
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::PLANE)
        .sighting()
        .build(),
    ]
}

#[cfg(test)]
mod modulate;
mod scramble;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};
use serde::Serialize;
use serde_json::{Value, json};

use self::avlc::{AvlcFrame, Control, Payload};
use self::decoder::{Vdl2Decoder, Vdl2Frame};
use sdrmm_decoder::link::{self, DataLinkMessage, Fields, Quality};
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};

const RATE: f64 = 100_000.0;
const HALF_BANDWIDTH: f64 = 8_500.0;
const INFO_HEX_LIMIT: usize = 64;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "vdl2".to_owned(),
    name: "VDL Mode 2".to_owned(),
    summary: "Aircraft datalink on VHF".to_owned(),
    family: DecoderFamily::AVIATION,
    verification: sdrmm_wire::Verification::OwnModulator,
    bandwidth_hz: HALF_BANDWIDTH * 2.0,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::named("vdl2")],
    ..ChannelDescriptor::default()
});

pub struct Vdl2Channel {
    decoder: Vdl2Decoder,
    frames: Vec<Vdl2Frame>,
}

fn params(settings: &ChannelSettings) -> Result<Vdl2Params, ChannelError> {
    sdrmm_decoder::settings_of::<Vdl2Params>(settings)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-HALF_BANDWIDTH, HALF_BANDWIDTH)
}

pub(crate) fn channel_filter() -> FilterSpec {
    FilterSpec::channel(HALF_BANDWIDTH)
}

impl NativeChannel for Vdl2Channel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        params(&settings)?;
        Ok(Self {
            decoder: Vdl2Decoder::new(ctx.input_rate),
            frames: Vec::new(),
        })
    }
}

impl Channel for Vdl2Channel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        params(&settings).map(|_| ())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.frames.clear();
        self.decoder.process(iq, &mut self.frames);
        out.events.extend(
            self.frames
                .iter()
                .map(|frame| DecoderEvent::tagged(EventKind::named("vdl2"), &message(frame))),
        );
    }
}

/// Where a VDL2 frame keeps its envelope: the aircraft, or the AVLC address that sent it.
const LINK: Fields = Fields::station(&["tail", "src", "icao"]);

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Body<'a, C: Serialize> {
    Acars(&'a C),
    Vdl2 { kind: String, details: Value },
}

fn message(frame: &Vdl2Frame) -> DataLinkMessage {
    let quality = Quality {
        crc_ok: frame.acars.as_ref().is_none_or(|block| block.crc_ok),
        fec_corrected: u32::try_from(frame.rs_corrected).ok(),
        snr_db: Some(frame.snr_db),
        frequency_error_hz: Some(frame.freq_skew_hz),
    };
    let raw = Some(frame.avlc.raw.as_slice());
    if let Some(block) = &frame.acars {
        link::message(&Body::Acars(&block.core), &LINK, quality, raw)
    } else {
        let (kind, details) = avlc_body(&frame.avlc, frame.atn.as_ref());
        link::message(&Body::<()>::Vdl2 { kind, details }, &LINK, quality, raw)
    }
}

fn frame_kind(frame: &AvlcFrame) -> String {
    match (&frame.control, &frame.payload) {
        (Control::Unnumbered { kind: "XID", .. }, _) => "xid".to_owned(),
        (Control::Unnumbered { kind, .. } | Control::Supervisory { kind, .. }, _) => {
            format!("avlc-{}", kind.to_lowercase())
        }
        (Control::Info { .. }, Payload::Atn { .. }) => "atn".to_owned(),
        (Control::Info { .. }, _) => "avlc-i".to_owned(),
    }
}

fn avlc_body(frame: &AvlcFrame, atn: Option<&Value>) -> (String, Value) {
    let mut details = json!({
        "dst": frame.dst,
        "src": frame.src,
        "control": frame.control,
    });
    if let Payload::Atn { ipi } = frame.payload {
        details["protocol"] = json!(match ipi {
            0x81 => "CLNP",
            0x82 => "ES-IS",
            _ => "IDRP",
        });
    }
    if let Some(atn) = atn {
        details["atn"] = atn.clone();
    }
    if let Control::Unnumbered { kind, .. } = frame.control {
        if kind == "XID"
            && let Some(params) = avlc::parse_xid(&frame.info)
        {
            details["params"] = json!(params);
        }
        if kind == "FRMR"
            && let Some(frmr) = avlc::parse_frmr(&frame.info)
        {
            details["frmr"] = json!(frmr);
        }
    }
    if !frame.info.is_empty() {
        let shown = &frame.info[..frame.info.len().min(INFO_HEX_LIMIT)];
        details["info_hex"] = json!(sdrmm_crypto::hex_string(shown));
        details["info_len"] = json!(frame.info.len());
    }
    (frame_kind(frame), details)
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
