mod assembly;
pub(crate) mod coding;
mod frame;
mod link;
mod message;
mod scrambling;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_modem::{
    cpm::{CpmDemod, CpmParams, Mapping, TIMING_BW_CONTINUOUS},
    pulse::{self, Norm},
};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderFamily, EventKind,
    MAX_TETRAPOL_SCRAMBLING, TETRAPOL_BANDWIDTH_HZ, TetrapolBand, TetrapolParams,
};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate,
    dv::{INPUT_RATE_HZ, tap_symbols},
};
use coding::Decoder;
use frame::Framer;
use link::Link;
use scrambling::Scrambling;

pub(crate) const BAUD: f64 = 8_000.0;
pub(crate) const DEVIATION_HZ: f64 = 2_000.0;
pub(crate) const BT: f64 = 0.25;
pub(crate) const PULSE_SPAN: usize = 4;
const MATCHED_BT: f64 = 1.0;
const MATCHED_SPAN: usize = 3;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "tetrapol".to_owned(),
    name: "Tetrapol".to_owned(),
    summary: "Tetrapol PMR control and traffic channels".to_owned(),
    family: DecoderFamily::DigitalVoice,
    bandwidth_hz: TETRAPOL_BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    emits: vec![EventKind::Tetrapol],
    ..ChannelDescriptor::default()
});

pub(crate) fn cpm_params(sps: f64) -> CpmParams {
    CpmParams::from_deviation(
        Mapping::natural(2),
        DEVIATION_HZ,
        BAUD,
        pulse::gaussian_freq(sps, BT, PULSE_SPAN, Norm::Area),
        sps,
    )
}

fn params(settings: &ChannelSettings) -> Result<&TetrapolParams, ChannelError> {
    match &settings.params {
        ChannelParams::Tetrapol(params) => Ok(params),
        other => Err(ChannelError::InvalidSettings(format!(
            "tetrapol channel got {} params",
            other.type_id()
        ))),
    }
}

fn check_params(params: &TetrapolParams) -> Result<(), ChannelError> {
    match params.scrambling {
        Some(code) if code > MAX_TETRAPOL_SCRAMBLING => Err(ChannelError::InvalidSettings(
            format!("tetrapol scrambling must be in [0, {MAX_TETRAPOL_SCRAMBLING}], got {code}"),
        )),
        _ => Ok(()),
    }
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-TETRAPOL_BANDWIDTH_HZ / 2.0, TETRAPOL_BANDWIDTH_HZ / 2.0)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    crate::dv::channel_filter(TETRAPOL_BANDWIDTH_HZ)
}

pub struct TetrapolChannel {
    demod: CpmDemod,
    slicer: Mapping,
    band: TetrapolBand,
    framer: Framer,
    scrambling: Scrambling,
    decoder: Decoder,
    link: Link,
    soft: Vec<f32>,
}

impl TetrapolChannel {
    fn symbol(&mut self, soft: f32, out: &mut ChannelOutputs) {
        let was_locked = self.framer.locked();
        let framed = self.framer.push(soft);
        if was_locked && !self.framer.locked() {
            self.link.flush_voice(out);
            self.link.reset();
        }
        let Some(framed) = framed else {
            return;
        };
        let decoded = self
            .scrambling
            .decode(&mut self.decoder, &framed.frame, self.band);
        if decoded.is_some() {
            self.framer.confirm();
        } else if framed.sync_errors == 0 && self.scrambling.known() {
            self.link.reject();
        }
        self.link.frame(decoded, out);
    }
}

impl ChannelRx for TetrapolChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        check_params(params)?;
        let sps = ctx.input_rate / BAUD;
        let cpm = cpm_params(sps);
        Ok(Self {
            demod: CpmDemod::new(
                &cpm,
                &pulse::gaussian(sps, MATCHED_BT, MATCHED_SPAN, Norm::Area),
                TIMING_BW_CONTINUOUS,
            )
            .continuous(),
            slicer: cpm.mapping().clone(),
            band: params.band,
            framer: Framer::new(),
            scrambling: Scrambling::new(params.scrambling),
            decoder: Decoder::new(),
            link: Link::new(),
            soft: Vec::new(),
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        check_params(params)?;
        if params.band != self.band {
            self.band = params.band;
            self.scrambling.reset();
        }
        self.scrambling.set_fixed(params.scrambling);
        Ok(())
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.framer.reset();
        self.scrambling.reset();
        self.link.forget();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.soft.clear();
        self.demod.process(iq, &mut self.soft);
        let soft = std::mem::take(&mut self.soft);
        for &symbol in &soft {
            self.symbol(symbol, out);
        }
        self.soft = soft;
        tap_symbols(
            out,
            &self.demod,
            &self.soft,
            &self.slicer,
            BAUD,
            INPUT_RATE_HZ,
        );
    }

    fn rejected(&self) -> u32 {
        self.link.rejected()
    }
}

#[cfg(test)]
mod tests;
