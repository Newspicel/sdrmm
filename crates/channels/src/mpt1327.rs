//! MPT 1327 trunked radio signalling: 1200 baud FFSK codewords on the control and traffic
//! channels, read as digital voice frames so a trunk node can follow the calls they grant.

mod codeword;
mod frame;
mod framer;
mod message;
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
mod trunk;
pub use model::*;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_decoder::registry::Native;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use sdrmm_modem::cpm::Mapping;
use sdrmm_modem::ffsk::{FfskDemod, FfskTones};
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, DvFrame, EventKind,
    FilterSpec, HzRange, LAND_MOBILE, Modulation, SettingField, SignalSignature, SignalTrial,
};

use framer::Framer;

const RATE: f64 = 48_000.0;
pub(crate) const DEVIATION_HZ: f64 = 2_500.0;
const CHANNEL_TAPS: usize = 65;
const MIN_BANDWIDTH_HZ: f64 = 6_000.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "mpt1327".to_owned(),
    name: "MPT 1327".to_owned(),
    summary: "MPT 1327 trunked radio signalling".to_owned(),
    family: DecoderFamily::ANALOG_VOICE,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: 12_500.0,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::DV],
    calls: Some(CallGate::Frames),
    ..ChannelDescriptor::default()
});

pub struct Mpt1327Channel {
    ffsk: FfskDemod,
    framer: Framer,
    bits: Vec<f32>,
    rail: Mapping,
    frames: Vec<DvFrame>,
}

fn params(settings: &ChannelSettings) -> Result<Mpt1327Params, ChannelError> {
    sdrmm_decoder::settings_of::<Mpt1327Params>(settings)
}

fn check_params(params: &Mpt1327Params) -> Result<(), ChannelError> {
    if params.bandwidth_hz.is_finite()
        && params.bandwidth_hz >= MIN_BANDWIDTH_HZ
        && params.bandwidth_hz < RATE
    {
        Ok(())
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "mpt1327 bandwidth must be in [{MIN_BANDWIDTH_HZ}, {RATE}) Hz, got {}",
            params.bandwidth_hz
        )))
    }
}

fn occupied_band(params: &Mpt1327Params) -> (f64, f64) {
    let half = params.bandwidth_hz / 2.0;
    (-half, half)
}

fn channel_filter(params: &Mpt1327Params) -> Result<FilterSpec, ChannelError> {
    check_params(params)?;
    Ok(FilterSpec::lowpass(CHANNEL_TAPS, params.bandwidth_hz / 2.0))
}

impl NativeChannel for Mpt1327Channel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        check_params(&params(&settings)?)?;
        Ok(Self {
            ffsk: FfskDemod::new(RATE, DEVIATION_HZ, FfskTones::CCIR_1200),
            framer: Framer::new(),
            bits: Vec::new(),
            rail: Mapping::natural(2),
            frames: Vec::new(),
        })
    }
}

impl Channel for Mpt1327Channel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        check_params(&params(&settings)?)
    }

    fn retuned(&mut self) {
        self.ffsk.reset();
        self.framer.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let mut bits = std::mem::take(&mut self.bits);
        self.ffsk.symbols(iq, &mut bits);
        out.symbols
            .levels(&bits, &[], &self.rail, FfskTones::CCIR_1200.baud, 0.0);
        for &bit in &bits {
            self.framer.push(bit, &mut self.frames);
        }
        self.bits = bits;
        for frame in self.frames.drain(..) {
            out.events.push(DecoderEvent::of(&frame));
        }
    }

    fn rejected(&self) -> u32 {
        self.framer.rejected()
    }
}

fn mpt1327_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "MPT 1327 (FFSK over FM)",
            &[Modulation::Fm],
            HzRange::new(6_000.0, 20_000.0),
            "A trunked radio control channel: continuous 1200 baud tones inside an FM channel",
        )
        .symbol_rate(HzRange::about(1_200.0, 0.06))
        .deviation(HzRange::new(1_500.0, 3_800.0))
        .on(LAND_MOBILE),
    ]
}

fn mpt1327_trial() -> SignalTrial {
    SignalTrial::proven_by(EventKind::DV)
}

pub(crate) fn mpt1327_decoder() -> Native<Mpt1327Params> {
    let decoder = Native::new::<Mpt1327Channel>(occupied_band)
        .filter(channel_filter)
        .signatures(mpt1327_signatures)
        .trial(mpt1327_trial)
        .form(mpt1327_form)
        .trunking(trunk::trunking());
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

fn mpt1327_form() -> Vec<SettingField> {
    vec![SettingField::bandwidth(&[12_500.0, 25_000.0])]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
