use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{HzRange, KindSpec, Modulation, SettingField, SignalSignature, SignalTrial};
mod framer;
mod layer2;
mod model;
mod packet;
mod v3;
pub use model::*;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_modem::orthogonal::{TrackingMfsk, TrackingMfskConfig};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};

use framer::Framer;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};

const INPUT_RATE_HZ: u32 = 8_000;
const BAUD: u32 = 100;
const TONES: usize = 4;
const BANDWIDTH_HZ: f64 = 6_000.0;
const FILTER_TAPS: usize = 127;

/// The 70 cm ISM band most Horus payloads fly in.
const HORUS_BANDS: &[HzRange] = &[HzRange::mhz(433.05, 434.79)];

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "horus".to_owned(),
    name: "Horus Binary".to_owned(),
    summary: "High-altitude balloon telemetry".to_owned(),
    family: DecoderFamily::WEATHER,
    verification: sdrmm_wire::Verification::ReferenceFixture,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: f64::from(INPUT_RATE_HZ),
    has_audio: false,
    has_video: false,
    emits: vec![EventKind::named("horus")],
    ..ChannelDescriptor::default()
});

fn horus_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "Horus Binary",
            &[Modulation::Fsk4],
            HzRange::new(500.0, 1_400.0),
            "four tones at 100 baud from a balloon on 70 cm",
        )
        .symbol_rate(HzRange::about(f64::from(BAUD), 0.08))
        .on(HORUS_BANDS),
    ]
}

fn horus_trial() -> SignalTrial {
    SignalTrial::proven_by(EventKind::named("horus"))
}

pub(crate) fn horus_decoder() -> Native<HorusParams> {
    Native::new::<HorusChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .signatures(horus_signatures)
        .trial(horus_trial)
        .form(horus_form)
        .kinds(horus_kinds)
}

pub(crate) fn horus_form() -> Vec<SettingField> {
    Vec::new()
}

fn horus_kinds() -> Vec<KindSpec> {
    vec![horus_kind()]
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-BANDWIDTH_HZ / 2.0, BANDWIDTH_HZ / 2.0)
}

pub(crate) fn channel_filter() -> FilterSpec {
    FilterSpec::lowpass(FILTER_TAPS, BANDWIDTH_HZ / 2.0)
}

fn params(settings: &ChannelSettings) -> Result<HorusParams, ChannelError> {
    sdrmm_decoder::settings_of::<HorusParams>(settings)
}

/// The demodulator and framer behind a channel, with the band its tones are searched in.
pub(crate) struct HorusReceiver {
    demod: TrackingMfsk,
    framer: Framer,
}

impl HorusReceiver {
    pub(crate) fn new(search_hz: (f32, f32)) -> Result<Self, ChannelError> {
        let config = TrackingMfskConfig::new(INPUT_RATE_HZ, BAUD, TONES, search_hz);
        let demod = TrackingMfsk::new(config)
            .map_err(|err| ChannelError::InvalidSettings(err.to_string()))?;
        Ok(Self {
            demod,
            framer: Framer::new(),
        })
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], frames: &mut Vec<HorusFrame>) {
        let first = frames.len();
        let framer = &mut self.framer;
        self.demod.push(iq, |symbol| {
            framer.push(symbol >> 1, frames);
            framer.push(symbol, frames);
        });
        let snr_db = self.demod.snr_db();
        for frame in &mut frames[first..] {
            frame.snr_db = snr_db;
        }
    }

    pub(crate) fn rejected(&self) -> u32 {
        self.framer.rejected()
    }
}

pub struct HorusChannel {
    receiver: HorusReceiver,
    frames: Vec<HorusFrame>,
}

impl NativeChannel for HorusChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        params(&settings)?;
        let (low, high) = occupied_band();
        Ok(Self {
            receiver: HorusReceiver::new((low as f32, high as f32))?,
            frames: Vec::new(),
        })
    }
}

impl Channel for HorusChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        params(&settings).map(|_| ())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.receiver.process(iq, &mut self.frames);
        out.events
            .extend(self.frames.drain(..).map(|frame| DecoderEvent::of(&frame)));
    }

    fn rejected(&self) -> u32 {
        self.receiver.rejected()
    }
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
