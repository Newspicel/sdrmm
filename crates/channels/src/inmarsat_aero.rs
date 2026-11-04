use crate::registry::Native;
use sdrmm_wire::{Control, KindSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
pub use model::*;
mod acquisition;
mod burst;
mod cchannel;
mod coherent;
mod decoder;
mod demod;
mod frame;
mod framer;
mod msk;
mod oqpsk;
mod receiver;
mod satellite;
mod state;
mod su;
mod taps;

const INMARSAT_BAND: &[HzRange] = &[HzRange::mhz(1_525.0, 1_559.0)];

fn inmarsat_aero_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "Inmarsat Aero",
            &[Modulation::Psk2, Modulation::Psk4, Modulation::Unknown],
            HzRange::new(2_000.0, 20_000.0),
            "phase keying at an aeronautical satellite rate",
        )
        .symbol_rate(HzRange::new(500.0, 11_000.0))
        .on(INMARSAT_BAND),
    ]
}

fn inmarsat_aero_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(EventKind::named("inmarsat_aero"))
}

pub(crate) fn inmarsat_aero_decoder() -> Native<InmarsatAeroParams> {
    Native::new::<InmarsatAeroChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(1_541_450_000.0)
        .signatures(inmarsat_aero_signatures)
        .trial(inmarsat_aero_trial)
        .form(inmarsat_aero_form)
        .kinds(inmarsat_aero_kinds)
}

pub(crate) fn inmarsat_aero_form() -> Vec<SettingField> {
    vec![
        SettingField::new("channel", "Channel", Control::choice::<AeroChannel>())
            .title("Aero channel"),
    ]
}

fn inmarsat_aero_kinds() -> Vec<KindSpec> {
    vec![crate::datalink::inmarsat_aero_kind()]
}

#[cfg(test)]
mod modulate;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use self::{
    burst::BurstEvent,
    decoder::{AeroChannelDecoder, AeroEvent, INPUT_RATE},
    receiver::{BurstReceiver, CircuitEvent, CircuitReceiver, burst_tag},
};
use crate::datalink::DataLinkMessage;
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    acars::block as acars_block,
    check_input_rate,
    datalink::{self, Quality},
};
use num_complex::Complex;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};
use serde::Serialize;
use serde_json::{Value, json};

const HALF_BANDWIDTH: f64 = 6_500.0;
const P_CHANNEL: &str = "p-channel";
const C_CHANNEL_VOICE: &str = "c-channel-voice";

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "inmarsat_aero".to_owned(),
    name: "Inmarsat Classic Aero".to_owned(),
    summary: "Aircraft datalink over Inmarsat".to_owned(),
    family: DecoderFamily::AVIATION,
    bandwidth_hz: HALF_BANDWIDTH * 2.0,
    input_rate_hz: INPUT_RATE,
    has_audio: false,
    emits: vec![EventKind::named("inmarsat_aero")],
    ..ChannelDescriptor::default()
});

enum Receiver {
    Forward(Box<AeroChannelDecoder>, Vec<AeroEvent>),
    Burst(Box<BurstReceiver>),
    Circuit(Box<CircuitReceiver>, Vec<CircuitEvent>),
}

impl Receiver {
    fn new(channel: AeroChannel) -> Self {
        match channel {
            AeroChannel::P => Self::Forward(Box::new(AeroChannelDecoder::new()), Vec::new()),
            AeroChannel::Burst => Self::Burst(Box::new(BurstReceiver::new())),
            AeroChannel::C => Self::Circuit(Box::new(CircuitReceiver::new()), Vec::new()),
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let messages: Vec<DataLinkMessage> = match self {
            Self::Forward(decoder, events) => {
                decoder.process(iq, events);
                events.drain(..).map(|event| to_message(&event)).collect()
            }
            Self::Burst(receiver) => receiver.process(iq).iter().map(burst_message).collect(),
            Self::Circuit(receiver, events) => {
                receiver.process(iq, events);
                events.drain(..).map(circuit_message).collect()
            }
        };
        out.events.extend(
            messages
                .into_iter()
                .map(|message| DecoderEvent::tagged(EventKind::named("inmarsat_aero"), &message)),
        );
    }
}

pub struct InmarsatAeroChannel {
    channel: AeroChannel,
    receiver: Receiver,
}

fn params(settings: &ChannelSettings) -> Result<InmarsatAeroParams, ChannelError> {
    crate::settings_of::<InmarsatAeroParams>(settings)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-HALF_BANDWIDTH, HALF_BANDWIDTH)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    datalink::channel_filter(INPUT_RATE, HALF_BANDWIDTH)
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AcarsBody<'a, C> {
    Acars(&'a C),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AeroBody {
    Aero { kind: String, details: Value },
    Undecoded,
}

fn enrich(details: &mut Value, event: &AeroEvent) {
    let Value::Object(map) = details else {
        return;
    };
    if let Some(Value::Object(satellite)) = &event.satellite {
        for (key, value) in satellite {
            map.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    if let Some(header) = event.frame_header {
        map.entry("frame_header")
            .or_insert_with(|| header.to_json());
    }
    if let Some(lock) = &event.lock {
        map.entry("superframe_lock").or_insert_with(|| lock.clone());
    }
}

fn aero_body(kind: &str, event: &AeroEvent) -> AeroBody {
    let mut details = json!({});
    enrich(&mut details, event);
    AeroBody::Aero {
        kind: kind.to_owned(),
        details,
    }
}

fn su_body(su_event: &Value, event: &AeroEvent) -> AeroBody {
    let mut details = su_event.clone();
    if let Value::Object(map) = &mut details {
        map.insert("channel".to_owned(), json!(P_CHANNEL));
        map.insert("line_bit_rate".to_owned(), json!(event.bit_rate));
    }
    enrich(&mut details, event);
    AeroBody::Aero {
        kind: su::p_su_kind(su_event),
        details,
    }
}

fn to_message(event: &AeroEvent) -> DataLinkMessage {
    let quality = |crc_ok| Quality {
        crc_ok,
        fec_corrected: event.fec_corrected,
        snr_db: None,
        frequency_error_hz: None,
    };
    let raw = Some(event.user.data.as_slice());
    if let Some(block) = &event.acars {
        return datalink::message(&AcarsBody::Acars(&block.core), quality(block.crc_ok), raw);
    }
    let body = match &event.su_event {
        Some(su_event) => su_body(su_event, event),
        None if event.lock.is_some() => aero_body("p-channel-status", event),
        None if event.satellite.is_some() || event.frame_header.is_some() => {
            aero_body("aero-frame", event)
        }
        None => AeroBody::Undecoded,
    };
    datalink::message(&body, quality(true), raw)
}

fn burst_message(event: &BurstEvent) -> DataLinkMessage {
    let quality = |crc_ok| Quality {
        crc_ok,
        fec_corrected: Some(event.fec_corrected),
        snr_db: None,
        frequency_error_hz: None,
    };
    if let Some(user) = &event.user {
        let raw = Some(user.data.as_slice());
        return match su::parse_acars(&user.data) {
            Some(block) => {
                datalink::message(&AcarsBody::Acars(&block.core), quality(block.crc_ok), raw)
            }
            None => datalink::message(&AeroBody::Undecoded, quality(true), raw),
        };
    }
    let mut details = event.su_event.clone().unwrap_or_else(|| json!({}));
    let kind = su::p_su_kind(&details);
    if let Value::Object(map) = &mut details {
        map.insert("channel".to_owned(), json!(burst_tag(event.channel)));
        map.insert("line_bit_rate".to_owned(), json!(event.bit_rate));
    }
    datalink::message(&AeroBody::Aero { kind, details }, quality(true), None)
}

fn circuit_message(event: CircuitEvent) -> DataLinkMessage {
    let body = match event {
        CircuitEvent::SignalUnit { kind, details } => AeroBody::Aero {
            kind: kind.to_owned(),
            details,
        },
        CircuitEvent::VoiceStarted => AeroBody::Aero {
            kind: C_CHANNEL_VOICE.to_owned(),
            details: json!({ "channel": "c-channel" }),
        },
    };
    datalink::message(
        &body,
        Quality {
            crc_ok: true,
            ..Quality::default()
        },
        None,
    )
}

impl NativeChannel for InmarsatAeroChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let channel = params(&settings)?.channel;
        Ok(Self {
            channel,
            receiver: Receiver::new(channel),
        })
    }
}

impl ChannelRx for InmarsatAeroChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let channel = params(&settings)?.channel;
        if channel != self.channel {
            self.channel = channel;
            self.receiver = Receiver::new(channel);
        }
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.receiver.process(iq, out);
    }
}
