use crate::registry::Native;
use sdrmm_wire::{Control, KindSpec, NumberSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
pub use model::*;
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
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderFamily, EventKind};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate,
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
    emits: vec![EventKind::named("tetrapol")],
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

fn params(settings: &ChannelSettings) -> Result<TetrapolParams, ChannelError> {
    settings
        .params
        .parse()
        .map_err(ChannelError::InvalidSettings)
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

impl NativeChannel for TetrapolChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        check_params(&params)?;
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
}

impl ChannelRx for TetrapolChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        check_params(&params)?;
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

const TETRAPOL_BANDS: &[HzRange] = &[
    HzRange::mhz(70.0, 87.5),
    HzRange::mhz(380.0, 400.0),
    HzRange::mhz(410.0, 430.0),
];

fn tetrapol_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "Tetrapol",
            &[Modulation::Fsk2],
            HzRange::new(8_000.0, 14_000.0),
            "8 kbaud GMSK in a 12.5 kHz channel on a Tetrapol band",
        )
        .symbol_rate(HzRange::new(7_600.0, 8_400.0))
        .deviation(HzRange::new(1_400.0, 2_600.0))
        .on(TETRAPOL_BANDS),
    ]
}

fn tetrapol_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(EventKind::named("tetrapol"))
}

pub(crate) fn tetrapol_decoder() -> Native<TetrapolParams> {
    Native::new::<TetrapolChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .signatures(tetrapol_signatures)
        .trial(tetrapol_trial)
        .form(tetrapol_form)
        .kinds(tetrapol_kinds)
}

pub(crate) fn tetrapol_form() -> Vec<SettingField> {
    vec![
        SettingField::new("band", "Band", Control::choice::<TetrapolBand>()).title("Tetrapol band"),
        SettingField::new(
            "scrambling",
            "Scrambling",
            NumberSpec::new(0.0, f64::from(MAX_TETRAPOL_SCRAMBLING), 1.0).optional("auto"),
        )
        .title("Scrambling code, empty to find it"),
    ]
}

fn tetrapol_kinds() -> Vec<KindSpec> {
    vec![crate::tetrapol::tetrapol_kind()]
}

#[cfg(test)]
mod tests;
