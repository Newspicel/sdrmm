use crate::registry::Native;
use sdrmm_wire::{Control, KindSpec, NumberSpec, SettingField};
mod model;
pub use model::*;
mod demod;
pub(crate) mod dfm;
pub(crate) mod fields;
mod forecast;
pub(crate) mod imet;
pub(crate) mod m10;
pub(crate) mod m20;
pub(crate) mod meteomodem;
pub(crate) mod rs41;
pub(crate) fn radiosonde_decoder() -> Native<RadiosondeParams> {
    Native::new::<RadiosondeChannel>(occupied_band)
        .filter(channel_filter)
        .form(radiosonde_form)
        .kinds(radiosonde_kinds)
}

pub(crate) fn radiosonde_form() -> Vec<SettingField> {
    vec![
        SettingField::new(
            "sonde",
            "Sonde",
            Control::optional_choice::<SondeType>("Auto"),
        )
        .title("Sonde type")
        .quiet(None::<SondeType>),
        SettingField::new(
            "burst_m",
            "Burst",
            NumberSpec::new(5_000.0, 45_000.0, 500.0).unit("m"),
        )
        .title("Burst altitude assumed while a sonde climbs")
        .fallback(DEFAULT_SONDE_BURST_M),
        SettingField::new(
            "ground_m",
            "Ground",
            NumberSpec::new(-500.0, 6_000.0, 10.0).unit("m"),
        )
        .title("Ground altitude where sondes land")
        .fallback(0),
    ]
}

fn radiosonde_kinds() -> Vec<KindSpec> {
    vec![crate::radiosonde::radiosonde_kind()]
}

#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_lowpass};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate,
};
use demod::{CHUNK, FrontEnd};
use dfm::Dfm;
use forecast::Flights;
use imet::Imet;
use meteomodem::{Accept, Meteomodem};
use rs41::Rs41;

pub(crate) const INPUT_RATE_HZ: f64 = demod::RATE;
const FILTER_TAPS: usize = 127;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "radiosonde".to_owned(),
    name: "Radiosonde".to_owned(),
    summary: "Weather balloon telemetry".to_owned(),
    family: DecoderFamily::WEATHER,
    bandwidth_hz: 20_000.0,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    has_video: false,
    emits: vec![EventKind::named("radiosonde")],
    ..ChannelDescriptor::default()
});

pub(crate) fn occupied_band(_p: &RadiosondeParams) -> (f64, f64) {
    let half = DESCRIPTOR.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(p: &RadiosondeParams) -> Result<ChannelFilter, ChannelError> {
    let (_, half) = occupied_band(p);
    Ok(ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(FILTER_TAPS, half / INPUT_RATE_HZ),
        1,
    )))
}

fn params(settings: &ChannelSettings) -> Result<RadiosondeParams, ChannelError> {
    crate::settings_of::<RadiosondeParams>(settings)
}

fn runs(params: &RadiosondeParams, sonde: SondeType) -> bool {
    params.sonde.is_none_or(|fixed| fixed == sonde)
}

fn accept(params: &RadiosondeParams) -> Accept {
    Accept {
        m10: runs(params, SondeType::M10),
        m20: runs(params, SondeType::M20),
    }
}

pub struct RadiosondeChannel {
    params: RadiosondeParams,
    front: FrontEnd,
    rs41: Rs41,
    dfm: Dfm,
    meteomodem: Meteomodem,
    imet: Imet,
    flights: Flights,
    frames: Vec<RadiosondeFrame>,
}

impl NativeChannel for RadiosondeChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        Ok(Self {
            params,
            front: FrontEnd::new(),
            rs41: Rs41::new(),
            dfm: Dfm::new(),
            meteomodem: Meteomodem::new(accept(&params)),
            imet: Imet::new(),
            flights: Flights::default(),
            frames: Vec::new(),
        })
    }
}

impl ChannelRx for RadiosondeChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.params = params(&settings)?;
        self.meteomodem.set_accept(accept(&self.params));
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let meteomodem = runs(&self.params, SondeType::M10) || runs(&self.params, SondeType::M20);
        for chunk in iq.chunks(CHUNK) {
            let audio = self.front.demodulate(chunk);
            if runs(&self.params, SondeType::Rs41) {
                self.rs41.push(audio, &mut self.frames);
            }
            if runs(&self.params, SondeType::Dfm) {
                self.dfm.push(audio, &mut self.frames);
            }
            if meteomodem {
                self.meteomodem.push(audio, &mut self.frames);
            }
            if runs(&self.params, SondeType::Imet4) {
                self.imet.push(audio, &mut self.frames);
            }
        }
        for mut frame in self.frames.drain(..) {
            self.flights.observe(&mut frame, &self.params);
            out.events.push(DecoderEvent::of(&frame));
        }
    }

    fn rejected(&self) -> u32 {
        [
            self.rs41.rejected(),
            self.dfm.rejected(),
            self.meteomodem.rejected(),
            self.imet.rejected(),
        ]
        .into_iter()
        .fold(0, u32::saturating_add)
    }
}
