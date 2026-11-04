use crate::registry::Native;
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
use sdrmm_wire::{KindSpec, SettingField};
mod model;
pub use model::*;
use std::sync::LazyLock;

use crate::datalink::DataLinkMessage;
use num_complex::Complex;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};
use serde::Serialize;

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate,
    datalink::{self, Quality},
};

mod demod;
mod frame;
mod message;
mod symbol;

const DSC_HF: &[HzRange] = &[
    HzRange::spot_khz(2_187.5, 1.0),
    HzRange::spot_khz(4_207.5, 1.0),
    HzRange::spot_khz(6_312.0, 1.0),
    HzRange::spot_khz(8_414.5, 1.0),
    HzRange::spot_khz(12_577.0, 1.0),
    HzRange::spot_khz(16_804.5, 1.0),
];

const DSC_VHF: &[HzRange] = &[HzRange::spot_khz(156_525.0, 12.5)];

fn dsc_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "DSC (HF)",
            &[Modulation::Fsk2],
            HzRange::new(100.0, 1_000.0),
            "100 baud at a 170 Hz shift on a distress and calling frequency",
        )
        .symbol_rate(HzRange::about(100.0, 0.08))
        .deviation(HzRange::new(50.0, 200.0))
        .on(DSC_HF),
        SignalSignature::new(
            "DSC (VHF channel 70)",
            &[Modulation::Fm, Modulation::Fsk2],
            HzRange::new(6_000.0, 20_000.0),
            "1200 baud audio tones inside an FM channel on marine channel 70",
        )
        .symbol_rate(HzRange::about(1_200.0, 0.06))
        .deviation(HzRange::new(1_500.0, 5_000.0))
        .on(DSC_VHF),
    ]
}

fn dsc_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(EventKind::named("dsc"))
}

pub(crate) fn dsc_decoder() -> Native<DscParams> {
    Native::new::<DscChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(2_187_500.0)
        .signatures(dsc_signatures)
        .trial(dsc_trial)
        .form(dsc_form)
        .kinds(dsc_kinds)
}

pub(crate) fn dsc_form() -> Vec<SettingField> {
    Vec::new()
}

fn dsc_kinds() -> Vec<KindSpec> {
    vec![crate::datalink::dsc_kind()]
}

#[cfg(test)]
mod equivalence;
#[cfg(test)]
mod modulate;
#[cfg(test)]
mod sensitivity;

use demod::{FskDemod, RATE, find_phasing};
use frame::{Received, decode_at, frame_bits};
use message::{DscMessage, Format};
use symbol::{LEADING_DX_PHASING, RX_DELAY, SYMBOL_BITS};

const HALF_BANDWIDTH: f64 = 250.0;
const CHAR_PAIR_BITS: usize = 2 * SYMBOL_BITS;
const MAX_BITS_WINDOW: usize = 4_096;
const HISTORY_BITS: usize = CHAR_PAIR_BITS * LEADING_DX_PHASING;
const MIN_FRAME_BITS: usize = 460;
const MAX_FRAME_BITS: usize = CHAR_PAIR_BITS * (LEADING_DX_PHASING + 25 + RX_DELAY + 1);

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "dsc".to_owned(),
    name: "Digital Selective Calling".to_owned(),
    summary: "Maritime distress and calling alerts".to_owned(),
    family: DecoderFamily::Marine,
    bandwidth_hz: HALF_BANDWIDTH * 2.0,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::named("dsc")],
    ..ChannelDescriptor::default()
});

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Body<'a> {
    Dsc {
        kind: &'static str,
        details: &'a DscMessage,
    },
}

enum Outcome {
    Emit(Box<DscMessage>, usize),
    Skip,
    Wait,
}

pub struct DscDecoder {
    demod: FskDemod,
    soft: Vec<f32>,
    hard: Vec<u8>,
    scanned: usize,
    waiting: Option<(usize, usize)>,
}

impl DscDecoder {
    pub fn new() -> Self {
        Self {
            demod: FskDemod::new(),
            soft: Vec::with_capacity(2 * MAX_BITS_WINDOW),
            hard: Vec::with_capacity(2 * MAX_BITS_WINDOW),
            scanned: 0,
            waiting: None,
        }
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<DscMessage>) {
        let before = self.soft.len();
        self.demod.process(iq, &mut self.soft);
        self.hard.extend(
            self.soft[before..]
                .iter()
                .map(|&soft| u8::from(soft >= 0.0)),
        );
        loop {
            let Some(offset) = self.hard.get(self.scanned..).and_then(find_phasing) else {
                self.scanned = self.scanned.max(self.phasing_search_end());
                break;
            };
            let found = self.scanned + offset;
            match self.outcome(found) {
                Outcome::Wait => break,
                Outcome::Skip => self.scanned = found + SYMBOL_BITS,
                Outcome::Emit(message, end) => {
                    out.push(*message);
                    self.scanned = end.max(found + SYMBOL_BITS);
                }
            }
            if self.scanned >= self.hard.len() {
                break;
            }
        }
        self.trim();
    }

    fn phasing_search_end(&self) -> usize {
        self.hard.len().saturating_sub(SYMBOL_BITS - 1)
    }

    fn outcome(&mut self, found: usize) -> Outcome {
        let available = self.hard.len() - found;
        let unchanged = self
            .waiting
            .is_some_and(|(at, length)| at == found && self.hard.len() < length + SYMBOL_BITS);
        if available < MIN_FRAME_BITS || unchanged {
            return Outcome::Wait;
        }
        let received = Received {
            hard: &self.hard,
            soft: &self.soft,
        };
        if let Some((start, message)) = best_valid(&received, found) {
            self.waiting = None;
            let end = start + frame_bits(&message);
            return Outcome::Emit(Box::new(message), end);
        }
        let primary = decode_at(&received, found);
        if primary.format == Format::Unknown {
            return Outcome::Skip;
        }
        let settled =
            primary.is_complete() && available >= frame_bits(&primary) + CHAR_PAIR_BITS * RX_DELAY;
        if !settled && available < MAX_FRAME_BITS {
            self.waiting = Some((found, self.hard.len()));
            return Outcome::Wait;
        }
        self.waiting = None;
        let end = found + frame_bits(&primary);
        Outcome::Emit(Box::new(primary), end)
    }

    fn trim(&mut self) {
        if self.hard.len() <= MAX_BITS_WINDOW {
            return;
        }
        let overflow = self.hard.len() - MAX_BITS_WINDOW;
        let drop = self
            .scanned
            .saturating_sub(HISTORY_BITS)
            .max(overflow)
            .min(self.hard.len());
        self.hard.drain(..drop);
        self.soft.drain(..drop);
        self.scanned = self.scanned.saturating_sub(drop);
        self.waiting = None;
    }
}

fn best_valid(received: &Received, found: usize) -> Option<(usize, DscMessage)> {
    let mut best: Option<(usize, usize, DscMessage)> = None;
    for start in (0..LEADING_DX_PHASING).filter_map(|back| found.checked_sub(back * CHAR_PAIR_BITS))
    {
        let message = decode_at(received, start);
        if message.format == Format::Unknown || !message.is_complete() || !message.ecc_ok() {
            continue;
        }
        let score = received.phasing_score(start);
        if best
            .as_ref()
            .is_none_or(|(best_score, _, _)| score > *best_score)
        {
            best = Some((score, start, message));
        }
    }
    best.map(|(_, start, message)| (start, message))
}

pub fn to_datalink(message: &DscMessage) -> DataLinkMessage {
    let raw: Vec<u8> = message
        .symbols
        .iter()
        .map(|&symbol| symbol.clamp(0, 255) as u8)
        .collect();
    datalink::message(
        &Body::Dsc {
            kind: message.format.kind(),
            details: message,
        },
        Quality {
            crc_ok: message.ecc_ok(),
            ..Quality::default()
        },
        Some(&raw),
    )
}

pub struct DscChannel {
    decoder: DscDecoder,
    messages: Vec<DscMessage>,
}

fn params(settings: &ChannelSettings) -> Result<DscParams, ChannelError> {
    crate::settings_of::<DscParams>(settings)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-HALF_BANDWIDTH, HALF_BANDWIDTH)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    datalink::channel_filter(RATE, HALF_BANDWIDTH)
}

impl NativeChannel for DscChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        params(&settings)?;
        Ok(Self {
            decoder: DscDecoder::new(),
            messages: Vec::new(),
        })
    }
}

impl ChannelRx for DscChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        params(&settings).map(|_| ())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.decoder.process(iq, &mut self.messages);
        out.events.extend(
            self.messages.drain(..).map(|message| {
                DecoderEvent::tagged(EventKind::named("dsc"), &to_datalink(&message))
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{cf32_le, run_events, settings};
    use sdrmm_wire::ChannelParams;

    const DISTRESS: &[i32] = &[
        112, 112, 25, 58, 5, 99, 70, 107, 4, 52, 60, 13, 7, 12, 52, 109, 127, 52, 127, 127,
    ];
    const INDIVIDUAL: &[i32] = &[
        120, 120, 32, 51, 42, 0, 0, 108, 0, 23, 71, 0, 0, 118, 126, 4, 10, 10, 4, 39, 30, 122, 54,
        122, 122,
    ];

    fn padded(symbols: &[i32]) -> Vec<Complex<f32>> {
        let mut iq = vec![Complex::new(0.0, 0.0); 400];
        iq.extend(modulate::call_iq(symbols, RATE, 0.0, 0.6));
        iq.extend(vec![Complex::new(0.0, 0.0); 400]);
        iq
    }

    fn decode_all(iq: &[Complex<f32>]) -> Vec<DscMessage> {
        let mut decoder = DscDecoder::new();
        let mut out = Vec::new();
        for chunk in iq.chunks(512) {
            decoder.process(chunk, &mut out);
        }
        out
    }

    #[test]
    fn decodes_a_distress_alert_fixture() {
        let symbols = [
            112, 112, 12, 34, 56, 78, 90, 100, 12, 34, 45, 67, 89, 12, 12, 34, 117, 88,
        ];
        let iq = modulate::call_iq(&symbols, RATE, 0.0, 0.8);
        let mut channel = DscChannel::new(
            ChannelCtx { input_rate: RATE },
            settings(ChannelParams::new("dsc", &DscParams::default()).expect("dsc settings")),
        )
        .expect("channel");
        let events = run_events(&mut channel, &iq);
        let message = events
            .iter()
            .find_map(|event| crate::datalink::read(event, "dsc"))
            .expect("DSC message");
        assert!(message.crc_ok);
        assert_eq!(message.message_type, "distress_alert");
        assert_eq!(message.station.as_deref(), Some("123456789"));
    }

    #[test]
    fn distress_alert_from_iq() {
        let out = decode_all(&padded(DISTRESS));
        let m = out
            .iter()
            .find(|m| m.format == Format::DistressAlert)
            .expect("distress alert");
        assert_eq!(m.from.as_deref(), Some("255805997"));
        assert_eq!(m.position.as_deref(), Some("45 26N 013 07E"));
        assert_eq!(m.time.as_deref(), Some("12:52"));
        assert_eq!(m.ecc, 52);
        assert!(m.ecc_ok());
    }

    #[test]
    fn individual_station_call_from_iq() {
        let out = decode_all(&padded(INDIVIDUAL));
        let m = out
            .iter()
            .find(|m| m.format == Format::IndividualStationCall)
            .expect("individual call");
        assert_eq!(m.to.as_deref(), Some("325142000"));
        assert_eq!(m.from.as_deref(), Some("002371000"));
        assert_eq!(m.frequency.as_deref(), Some("04101.0/04393.0"));
        assert!(m.ecc_ok());
    }

    #[test]
    fn datalink_shape() {
        let out = decode_all(&padded(DISTRESS));
        let m = out
            .iter()
            .find(|m| m.format == Format::DistressAlert)
            .expect("distress alert");
        let message = to_datalink(m);
        assert_eq!(message.message_type, "distress_alert");
        assert!(message.crc_ok);
        assert_eq!(message.station.as_deref(), Some("255805997"));
        assert_eq!(message.details["type"], "dsc");
        assert_eq!(message.details["details"]["position"], "45 26N 013 07E");
        assert!(message.raw.is_some());
    }

    #[test]
    fn a_frame_that_never_completes_does_not_stall_the_decoder() {
        let mut symbols = DISTRESS.to_vec();
        symbols.truncate(16);
        symbols.extend([5; 10]);
        let mut iq = padded(&symbols);
        iq.extend(padded(DISTRESS));
        let out = decode_all(&iq);
        assert!(
            out.iter()
                .any(|m| m.format == Format::DistressAlert && !m.ecc_ok())
        );
        assert!(
            out.iter()
                .any(|m| m.format == Format::DistressAlert && m.ecc_ok())
        );
    }

    #[test]
    fn a_closed_squelch_never_grows_the_bit_buffers() {
        let mut decoder = DscDecoder::new();
        let mut out = Vec::new();
        let silence = vec![Complex::new(0.0, 0.0); 4_096];
        for _ in 0..120 {
            decoder.process(&silence, &mut out);
        }
        assert!(out.is_empty());
        assert!(
            decoder.hard.len() <= MAX_BITS_WINDOW,
            "{}",
            decoder.hard.len()
        );
        assert_eq!(decoder.soft.len(), decoder.hard.len());
    }

    #[test]
    fn a_closed_squelch_still_decodes_a_following_call() {
        let mut decoder = DscDecoder::new();
        let mut out = Vec::new();
        let silence = vec![Complex::new(0.0, 0.0); 4_096];
        for _ in 0..120 {
            decoder.process(&silence, &mut out);
        }
        for chunk in padded(DISTRESS).chunks(512) {
            decoder.process(chunk, &mut out);
        }
        assert!(
            out.iter()
                .any(|m| m.format == Format::DistressAlert && m.ecc_ok()),
            "{out:?}"
        );
    }

    #[test]
    fn decodes_a_coast_station_off_air() {
        let iq = cf32_le(include_bytes!("../../../fixtures/dsc_offair_8k.sigmf-data"));
        let mut filtered = Vec::new();
        channel_filter().process(&iq, &mut filtered);
        let out = decode_all(&filtered);
        assert_eq!(out.len(), 1, "{out:?}");
        let m = &out[0];
        assert_eq!(m.format, Format::IndividualStationCall);
        assert_eq!(m.from.as_deref(), Some("002191000"));
        assert_eq!(m.to.as_deref(), Some("231700000"));
        assert_eq!(m.eos, message::EndOfSequence::AcknowledgeBq);
        assert!(m.ecc_ok());
    }
}
