use crate::registry::Native;
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
use sdrmm_wire::{KindSpec, SettingField};
mod model;
pub use model::*;
pub(crate) mod burst;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{BitSync, Decimator, FmDemod, RealDecimator, ToneCorrelator, design_lowpass};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate,
};
use burst::{Bursts, Framer};

pub(crate) const RATE: f64 = 50_000.0;
pub(crate) const DEVIATION_HZ: f64 = 5_000.0;
const AUDIO_DECIMATION: usize = 4;
pub(crate) const AUDIO_RATE: f64 = RATE / AUDIO_DECIMATION as f64;
pub(crate) const SAMPLES_PER_BIT: usize = (AUDIO_RATE / SAME_BAUD) as usize;
const CHANNEL_TAPS: usize = 65;
const AUDIO_TAPS: usize = 31;
const AUDIO_CUTOFF_HZ: f64 = 2_800.0;
const CLIP: f32 = 1.2;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "same".to_owned(),
    name: "SAME alerts".to_owned(),
    summary: "NOAA Weather Radio and EAS alert headers".to_owned(),
    family: DecoderFamily::WEATHER,
    bandwidth_hz: SAME_BANDWIDTH_HZ,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::named("same")],
    ..ChannelDescriptor::default()
});

struct Demod {
    fm: FmDemod,
    audio: RealDecimator,
    mark: ToneCorrelator,
    space: ToneCorrelator,
    sync: BitSync,
    discriminated: Vec<f32>,
    decimated: Vec<f32>,
}

impl Demod {
    fn new() -> Self {
        Self {
            fm: FmDemod::new(RATE, DEVIATION_HZ),
            audio: RealDecimator::new(
                &design_lowpass(AUDIO_TAPS, AUDIO_CUTOFF_HZ / RATE),
                AUDIO_DECIMATION,
            ),
            mark: ToneCorrelator::new(AUDIO_RATE, SAME_MARK_HZ, SAMPLES_PER_BIT),
            space: ToneCorrelator::new(AUDIO_RATE, SAME_SPACE_HZ, SAMPLES_PER_BIT),
            sync: BitSync::new(AUDIO_RATE, SAME_BAUD),
            discriminated: Vec::new(),
            decimated: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.audio.reset();
        self.mark.reset();
        self.space.reset();
        self.sync.reset();
    }

    fn bits(&mut self, iq: &[Complex<f32>], out: &mut Vec<bool>) {
        self.fm.process(iq, &mut self.discriminated);
        self.discriminated
            .iter_mut()
            .for_each(|sample| *sample = sample.clamp(-CLIP, CLIP));
        self.audio.process(&self.discriminated, &mut self.decimated);
        out.clear();
        for &sample in &self.decimated {
            let soft = self.mark.push(sample) - self.space.push(sample);
            if let Some(symbol) = self.sync.push_soft(soft) {
                out.push(symbol >= 0.0);
            }
        }
    }
}

pub struct SameChannel {
    demod: Demod,
    framer: Framer,
    bursts: Bursts,
    bits: Vec<bool>,
    clock: u64,
}

fn params(settings: &ChannelSettings) -> Result<SameParams, ChannelError> {
    crate::settings_of::<SameParams>(settings)
}

fn check_params(params: &SameParams) -> Result<(), ChannelError> {
    if params.bandwidth_hz.is_finite()
        && (MIN_SAME_BANDWIDTH_HZ..=MAX_SAME_BANDWIDTH_HZ).contains(&params.bandwidth_hz)
    {
        Ok(())
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "same bandwidth must be in [{MIN_SAME_BANDWIDTH_HZ}, {MAX_SAME_BANDWIDTH_HZ}] Hz, got {}",
            params.bandwidth_hz
        )))
    }
}

pub(crate) fn occupied_band(params: &SameParams) -> (f64, f64) {
    let half = params.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(params: &SameParams) -> Result<ChannelFilter, ChannelError> {
    check_params(params)?;
    let half = params.bandwidth_hz / 2.0;
    Ok(ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(CHANNEL_TAPS, half / RATE),
        1,
    )))
}

impl NativeChannel for SameChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        check_params(&params(&settings)?)?;
        Ok(Self {
            demod: Demod::new(),
            framer: Framer::default(),
            bursts: Bursts::default(),
            bits: Vec::new(),
            clock: 0,
        })
    }
}

impl ChannelRx for SameChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        check_params(&params(&settings)?)
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.framer = Framer::default();
        self.bursts = Bursts::default();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let mut bits = std::mem::take(&mut self.bits);
        self.demod.bits(iq, &mut bits);
        self.clock += iq.len() as u64;
        for &bit in &bits {
            if let Some(text) = self.framer.push(bit) {
                self.bursts.received(self.clock, text);
            }
        }
        self.bursts.expire(self.clock);
        for message in self.bursts.take() {
            out.events.push(DecoderEvent::of(&message));
        }
        self.bits = bits;
    }

    fn rejected(&self) -> u32 {
        self.bursts.rejected()
    }
}

const NWR_CHANNELS: &[HzRange] = &[HzRange::mhz(162.4, 162.56)];

fn same_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "SAME alert header (AFSK over FM)",
            &[Modulation::Fm, Modulation::Fsk2],
            HzRange::new(6_000.0, 20_000.0),
            "521 baud tones inside an FM channel on a weather radio frequency",
        )
        .symbol_rate(HzRange::about(520.83, 0.06))
        .deviation(HzRange::new(2_000.0, 5_500.0))
        .on(NWR_CHANNELS),
    ]
}

fn same_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(EventKind::named("same"))
}

pub(crate) fn same_decoder() -> Native<SameParams> {
    Native::new::<SameChannel>(occupied_band)
        .filter(channel_filter)
        .signatures(same_signatures)
        .trial(same_trial)
        .form(same_form)
        .kinds(same_kinds)
}

pub(crate) fn same_form() -> Vec<SettingField> {
    vec![SettingField::bandwidth(&[12_500.0, 25_000.0])]
}

fn same_kinds() -> Vec<KindSpec> {
    vec![crate::same::same_kind()]
}

#[cfg(test)]
mod tests;
