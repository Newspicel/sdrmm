use crate::registry::Native;
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
use sdrmm_wire::{KindSpec, SettingField};
mod model;
pub use model::*;
mod ac_cache;
mod demod;
mod fec;
mod lms;
const HFDL_BANDS: &[HzRange] = &[HzRange::khz(2_500.0, 22_000.0)];

fn hfdl_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "HFDL",
            &[Modulation::Psk2, Modulation::Psk4, Modulation::Unknown],
            HzRange::new(1_500.0, 3_500.0),
            "1800 baud phase keying in an aeronautical HF channel",
        )
        .symbol_rate(HzRange::about(1_800.0, 0.06))
        .on(HFDL_BANDS),
    ]
}

fn hfdl_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(EventKind::named("hfdl"))
}

pub(crate) fn hfdl_decoder() -> Native<HfdlParams> {
    Native::new::<HfdlChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(10_081_000.0)
        .signatures(hfdl_signatures)
        .trial(hfdl_trial)
        .form(hfdl_form)
        .kinds(hfdl_kinds)
}

pub(crate) fn hfdl_form() -> Vec<SettingField> {
    Vec::new()
}

fn hfdl_kinds() -> Vec<KindSpec> {
    vec![crate::datalink::hfdl_kind()]
}

#[cfg(test)]
mod modulate;
mod pdu;
mod systable;

use std::{f64::consts::PI, sync::LazyLock};

use crate::datalink::DataLinkMessage;
use num_complex::Complex;
use sdrmm_dsp::{Decimator, Nco};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};
use serde::Serialize;
use serde_json::Value;

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate,
    datalink::{self, Quality},
};
use demod::{Burst, HfdlDemod};
use pdu::{HfdlEvent, PduParser};

const RATE: f64 = 12_000.0;
const HALF_BANDWIDTH: f64 = 3_000.0;
const SUBCARRIER_OFFSET_HZ: f64 = 1_440.0;
const SELECTIVITY_HZ: f64 = 1_500.0;
const SELECTIVITY_TAPS: usize = 9;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "hfdl".to_owned(),
    name: "High Frequency Data Link".to_owned(),
    summary: "Aircraft datalink on HF".to_owned(),
    family: DecoderFamily::Aviation,
    bandwidth_hz: HALF_BANDWIDTH * 2.0,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::named("hfdl")],
    ..ChannelDescriptor::default()
});

fn blackman_harris_lowpass(cutoff: f64, taps: usize) -> Vec<f32> {
    const WINDOW: [f64; 4] = [0.35875, 0.48829, 0.14128, 0.01168];
    let center = (taps as f64 - 1.0) / 2.0;
    let raw: Vec<f64> = (0..taps)
        .map(|i| {
            let x = 2.0 * PI * i as f64 / (taps as f64 - 1.0);
            let window = WINDOW[0] - WINDOW[1] * x.cos() + WINDOW[2] * (2.0 * x).cos()
                - WINDOW[3] * (3.0 * x).cos();
            let t = i as f64 - center;
            let sinc = if t.abs() < 1e-12 {
                2.0 * cutoff
            } else {
                (2.0 * PI * cutoff * t).sin() / (PI * t)
            };
            sinc * window
        })
        .collect();
    let sum: f64 = raw.iter().sum();
    raw.into_iter().map(|t| (t / sum) as f32).collect()
}

struct Receiver {
    mixer: Nco,
    mixed: Vec<Complex<f32>>,
    selectivity: Decimator,
    selected: Vec<Complex<f32>>,
    demod: HfdlDemod,
    parser: PduParser,
    bursts: Vec<Burst>,
}

impl Receiver {
    fn new(rate: f64) -> Self {
        Self {
            mixer: Nco::new(-SUBCARRIER_OFFSET_HZ as f32, rate as f32),
            mixed: Vec::new(),
            selectivity: Decimator::new(
                &blackman_harris_lowpass(SELECTIVITY_HZ / rate, SELECTIVITY_TAPS),
                1,
            ),
            selected: Vec::new(),
            demod: HfdlDemod::new(rate),
            parser: PduParser::new(),
            bursts: Vec::new(),
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], events: &mut Vec<HfdlEvent>) {
        self.mixed.resize(iq.len(), Complex::default());
        self.mixer.mix_into(iq, &mut self.mixed);
        self.selectivity.process(&self.mixed, &mut self.selected);
        self.demod.process(&self.selected, &mut self.bursts);
        for burst in self.bursts.drain(..) {
            let first = events.len();
            self.parser.parse(&burst.payload, burst.bps, events);
            for event in &mut events[first..] {
                event.fec_corrected = Some(burst.fec_corrected);
                event.freq_skew_hz = Some(burst.freq_skew_hz);
                event.snr_db = burst.snr_db;
            }
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Body<'a, C> {
    Acars(&'a C),
    Hfdl { kind: &'a str, details: &'a Value },
}

fn message(event: &HfdlEvent) -> DataLinkMessage {
    let quality = Quality {
        crc_ok: event.acars.as_ref().is_none_or(|block| block.crc_ok),
        fec_corrected: event.fec_corrected,
        snr_db: event.snr_db,
        frequency_error_hz: event.freq_skew_hz,
    };
    let raw = Some(event.raw.as_slice());
    match &event.acars {
        Some(block) => datalink::message(&Body::Acars(&block.core), quality, raw),
        None => datalink::message(
            &Body::<()>::Hfdl {
                kind: &event.kind,
                details: &event.details,
            },
            quality,
            raw,
        ),
    }
}

pub struct HfdlChannel {
    receiver: Receiver,
    events: Vec<HfdlEvent>,
}

fn params(settings: &ChannelSettings) -> Result<HfdlParams, ChannelError> {
    crate::settings_of::<HfdlParams>(settings)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-HALF_BANDWIDTH, HALF_BANDWIDTH)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    datalink::channel_filter(RATE, HALF_BANDWIDTH)
}

impl NativeChannel for HfdlChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        params(&settings)?;
        Ok(Self {
            receiver: Receiver::new(ctx.input_rate),
            events: Vec::new(),
        })
    }
}

impl ChannelRx for HfdlChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        params(&settings).map(|_| ())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.receiver.process(iq, &mut self.events);
        out.events.extend(
            self.events
                .drain(..)
                .map(|event| DecoderEvent::tagged(EventKind::named("hfdl"), &message(&event))),
        );
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::fixture;
