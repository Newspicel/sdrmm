use crate::registry::Native;
use sdrmm_wire::{HzRange, KindSpec, Modulation, SettingField, SignalSignature, SignalTrial};
mod model;
pub use model::*;
mod bits;
pub(crate) mod burst;
mod cell;
mod cmce;
pub(crate) mod coding;
pub(crate) mod demod;
mod llc;
mod mac;
mod sds;
const TETRA_BANDS: &[HzRange] = &[
    HzRange::mhz(380.0, 400.0),
    HzRange::mhz(410.0, 430.0),
    HzRange::mhz(450.0, 470.0),
    HzRange::mhz(870.0, 876.0),
    HzRange::mhz(915.0, 921.0),
];

fn tetra_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "TETRA",
            &[Modulation::Psk4],
            HzRange::new(18_000.0, 30_000.0),
            "18 ksym/s pi/4-DQPSK in a 25 kHz channel on a TETRA band",
        )
        .symbol_rate(HzRange::about(SYMBOL_RATE, 0.03))
        .on(TETRA_BANDS),
    ]
}

fn tetra_trial() -> SignalTrial {
    SignalTrial::proven_by(EventKind::DV)
}

pub(crate) fn tetra_decoder() -> Native<TetraParams> {
    Native::new::<TetraChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .signatures(tetra_signatures)
        .trial(tetra_trial)
        .form(tetra_form)
        .kinds(tetra_kinds)
}

pub(crate) fn tetra_form() -> Vec<SettingField> {
    Vec::new()
}

fn tetra_kinds() -> Vec<KindSpec> {
    vec![]
}

#[cfg(test)]
mod tests;
mod upper;

use std::{f64::consts::FRAC_PI_4, sync::LazyLock};

use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_lowpass};
use sdrmm_modem::constellation::{Constellation, tables};
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, DvFrame, EventKind,
};

use self::{
    cell::Receiver,
    demod::{Demod, Dibit, INPUT_RATE_HZ, SYMBOL_RATE},
};
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate,
};

pub(crate) const BANDWIDTH_HZ: f64 = 25_000.0;
const OCCUPIED_HZ: f64 = 24_300.0;
const FILTER_TAPS: usize = 129;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "tetra".to_owned(),
    name: "TETRA".to_owned(),
    summary: "TETRA downlink control and calls".to_owned(),
    family: DecoderFamily::DIGITAL_VOICE,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    emits: vec![EventKind::DV],
    calls: Some(CallGate::Frames),
    ..ChannelDescriptor::default()
});

pub struct TetraChannel {
    demod: Demod,
    receiver: Receiver,
    dibits: Vec<Dibit>,
    symbols: Vec<Complex<f32>>,
    frames: Vec<DvFrame>,
    constellation: Option<Constellation>,
}

fn params(settings: &ChannelSettings) -> Result<TetraParams, ChannelError> {
    settings
        .params
        .parse()
        .map_err(ChannelError::InvalidSettings)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-OCCUPIED_HZ / 2.0, OCCUPIED_HZ / 2.0)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(FILTER_TAPS, BANDWIDTH_HZ / 2.0 / INPUT_RATE_HZ),
        1,
    ))
}

impl NativeChannel for TetraChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        params(&settings)?;
        Ok(Self {
            demod: Demod::new(),
            receiver: Receiver::new(),
            dibits: Vec::new(),
            symbols: Vec::new(),
            frames: Vec::new(),
            constellation: tables::psk_rotated(4, FRAC_PI_4).ok(),
        })
    }
}

impl ChannelRx for TetraChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        params(&settings)?;
        Ok(())
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.receiver.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.dibits.clear();
        self.demod.process(iq, &mut self.dibits);
        self.tap(out);
        for dibit in &self.dibits {
            self.receiver
                .push(dibit.first, dibit.second, &mut self.frames);
        }
        out.events
            .extend(self.frames.drain(..).map(|frame| DecoderEvent::of(&frame)));
    }

    fn rejected(&self) -> u32 {
        self.receiver.rejected()
    }
}

impl TetraChannel {
    fn tap(&mut self, out: &mut ChannelOutputs) {
        let Some(table) = &self.constellation else {
            return;
        };
        self.symbols.clear();
        self.symbols
            .extend(self.dibits.iter().map(|dibit| dibit.product));
        out.symbols.linear(
            &self.symbols,
            table,
            SYMBOL_RATE,
            self.demod.frequency_offset_hz(),
        );
    }
}
