mod bits;
pub(crate) mod burst;
mod cell;
mod cmce;
pub(crate) mod coding;
pub(crate) mod demod;
mod llc;
mod mac;
mod sds;
#[cfg(test)]
mod tests;
mod upper;

use std::{f64::consts::FRAC_PI_4, sync::LazyLock};

use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_lowpass};
use sdrmm_modem::constellation::{Constellation, tables};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, DvFrame,
    TetraParams,
};

use self::{
    cell::Receiver,
    demod::{Demod, Dibit, INPUT_RATE_HZ, SYMBOL_RATE},
};
use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};

pub(crate) const BANDWIDTH_HZ: f64 = 25_000.0;
const OCCUPIED_HZ: f64 = 24_300.0;
const FILTER_TAPS: usize = 129;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "tetra".to_owned(),
    name: "TETRA".to_owned(),
    summary: "TETRA downlink control and calls".to_owned(),
    family: DecoderFamily::DigitalVoice,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    decoder_kind: Some("dv".to_owned()),
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

fn params(settings: &ChannelSettings) -> Result<&TetraParams, ChannelError> {
    match &settings.params {
        ChannelParams::Tetra(p) => Ok(p),
        other => Err(ChannelError::InvalidSettings(format!(
            "tetra channel got {} params",
            other.type_id()
        ))),
    }
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

impl ChannelRx for TetraChannel {
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
            .extend(self.frames.drain(..).map(DecoderEvent::Dv));
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
