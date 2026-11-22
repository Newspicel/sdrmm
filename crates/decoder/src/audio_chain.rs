//! The listening chain after a decoder: AGC, filters, notches, click removal and denoising.

use sdrmm_dsp::{
    Agc, AutoNotch, ClickRemover, SpectralDenoiser, Svf, SvfShape,
    audio::noise::{NeuralDenoiseError, NeuralDenoiser},
    math::neural::Net,
};
use std::sync::Arc;

use sdrmm_wire::{
    AudioProcessing, ChannelParams, DenoiseMode, DenoiseModel, DenoiseSettings, NotchSettings,
    PeakSettings,
};

use crate::AUDIO_RATE;

#[cfg(test)]
mod tests;

/// Where the neural denoiser's models come from; the host loads them, the chain only runs them.
pub trait DenoiseNets: Send + Sync {
    fn net(&self, model: DenoiseModel) -> Result<Arc<Net>, NeuralDenoiseError>;
}

const AGC_TARGET_RMS: f32 = 0.25;
const AGC_MAX_GAIN: f32 = 100.0;
const BUTTERWORTH_Q: [f64; 2] = [0.541_196_1, 1.306_562_9];

pub use sdrmm_wire::ClickProfile;

/// The click shape of the detector that made `params`' audio, as its decoder declares it.
#[must_use]
pub fn click_profile(params: &ChannelParams) -> ClickProfile {
    crate::descriptor(params.type_id()).map_or_else(ClickProfile::default, |d| d.click)
}

fn click_width_s(profile: ClickProfile) -> f64 {
    match profile {
        ClickProfile::Discriminator => 100e-6,
        ClickProfile::Detector => 400e-6,
        ClickProfile::Vocoder => 60e-6,
    }
}

pub struct AudioChain {
    nets: Arc<dyn DenoiseNets>,
    settings: AudioProcessing,
    profile: ClickProfile,
    planes: Vec<Plane>,
    deinterleaved: Vec<Vec<f32>>,
}

impl AudioChain {
    pub fn new(
        audio_channels: u8,
        settings: &AudioProcessing,
        profile: ClickProfile,
        nets: Arc<dyn DenoiseNets>,
    ) -> Result<Self, NeuralDenoiseError> {
        let mut chain = Self {
            nets,
            settings: AudioProcessing::default(),
            profile,
            planes: Vec::new(),
            deinterleaved: Vec::new(),
        };
        chain.configure(audio_channels, settings, profile)?;
        Ok(chain)
    }

    pub fn configure(
        &mut self,
        audio_channels: u8,
        settings: &AudioProcessing,
        profile: ClickProfile,
    ) -> Result<(), NeuralDenoiseError> {
        let planes = usize::from(audio_channels).max(1);
        let rebuild = planes != self.planes.len();
        if rebuild {
            self.planes.clear();
            self.planes.resize_with(planes, Plane::default);
        }
        let agc_changed = rebuild || settings.agc != self.settings.agc;
        let profile_changed = rebuild || profile != self.profile;
        let mut result = Ok(());
        for plane in &mut self.planes {
            let changes = Changes {
                agc: agc_changed,
                profile,
                profile_changed,
            };
            if let Err(error) = plane.configure(settings, changes, self.nets.as_ref()) {
                result = Err(error);
            }
        }
        self.deinterleaved.resize_with(planes, Vec::new);
        self.settings = settings.clone();
        self.profile = profile;
        result
    }

    #[must_use]
    pub fn planes(&self) -> usize {
        self.planes.len()
    }

    /// Frames until the chain falls silent once its input stops.
    #[must_use]
    pub fn tail(&self) -> usize {
        self.planes.first().map_or(0, Plane::tail)
    }

    pub fn reset(&mut self) {
        for plane in &mut self.planes {
            plane.reset();
        }
    }

    pub fn process_audio(&mut self, pcm: &mut [f32]) -> Result<(), NeuralDenoiseError> {
        if self.planes.is_empty() || pcm.is_empty() {
            return Ok(());
        }
        if self.planes.len() == 1 {
            return self.planes[0].process(pcm);
        }
        let planes = self.planes.len();
        for (index, buffer) in self.deinterleaved.iter_mut().enumerate() {
            buffer.clear();
            buffer.extend(pcm.iter().skip(index).step_by(planes));
        }
        for (plane, buffer) in self.planes.iter_mut().zip(&mut self.deinterleaved) {
            plane.process(buffer)?;
        }
        for (index, buffer) in self.deinterleaved.iter().enumerate() {
            for (slot, &value) in pcm.iter_mut().skip(index).step_by(planes).zip(buffer) {
                *slot = value;
            }
        }
        Ok(())
    }
}

enum Denoiser {
    Spectral(Box<SpectralDenoiser>),
    Neural(Box<NeuralDenoiser>, sdrmm_wire::DenoiseModel),
}

impl Denoiser {
    fn build(
        settings: DenoiseSettings,
        nets: &dyn DenoiseNets,
    ) -> Result<Self, NeuralDenoiseError> {
        Ok(match settings.mode {
            DenoiseMode::Spectral => {
                Self::Spectral(Box::new(SpectralDenoiser::new(settings.strength)))
            }
            DenoiseMode::Neural => Self::Neural(
                Box::new(NeuralDenoiser::new(
                    nets.net(settings.model)?,
                    AUDIO_RATE,
                    settings.strength,
                )?),
                settings.model,
            ),
        })
    }

    fn matches(&self, settings: DenoiseSettings) -> bool {
        match self {
            Self::Spectral(_) => settings.mode == DenoiseMode::Spectral,
            Self::Neural(_, model) => {
                settings.mode == DenoiseMode::Neural && settings.model == *model
            }
        }
    }

    fn tail(&self) -> usize {
        match self {
            Self::Spectral(denoiser) => denoiser.latency(),
            Self::Neural(denoiser, _) => denoiser.tail(),
        }
    }

    fn set_strength(&mut self, strength: f32) {
        match self {
            Self::Spectral(denoiser) => denoiser.set_strength(strength),
            Self::Neural(denoiser, _) => denoiser.set_strength(strength),
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Spectral(denoiser) => denoiser.reset(),
            Self::Neural(denoiser, _) => denoiser.reset(),
        }
    }

    fn process(&mut self, pcm: &mut [f32]) -> Result<(), NeuralDenoiseError> {
        match self {
            Self::Spectral(denoiser) => Ok(denoiser.process(pcm)?),
            Self::Neural(denoiser, _) => denoiser.process(pcm),
        }
    }
}

#[derive(Clone, Copy)]
struct Changes {
    agc: bool,
    profile: ClickProfile,
    profile_changed: bool,
}

#[derive(Default)]
struct Plane {
    clicks: Option<ClickRemover>,
    highpass: Vec<Svf>,
    lowpass: Vec<Svf>,
    notches: Vec<Svf>,
    peak: Option<Svf>,
    auto_notch: Option<AutoNotch>,
    denoise: Option<Denoiser>,
    agc: Option<Agc>,
}

impl Plane {
    fn configure(
        &mut self,
        settings: &AudioProcessing,
        changes: Changes,
        nets: &dyn DenoiseNets,
    ) -> Result<(), NeuralDenoiseError> {
        let Changes {
            agc: agc_changed,
            profile,
            profile_changed,
        } = changes;
        let rate = f64::from(AUDIO_RATE);
        match (&mut self.clicks, settings.click_removal.enabled) {
            (Some(clicks), true) if !profile_changed => {
                clicks.set_threshold(settings.click_removal.threshold);
            }
            (slot, true) => {
                *slot = Some(ClickRemover::new(
                    rate,
                    click_width_s(profile),
                    settings.click_removal.threshold,
                ));
            }
            (slot, false) => *slot = None,
        }
        let filter = &settings.filter;
        let on = |specs: Vec<Tuning>| if filter.enabled { specs } else { Vec::new() };
        retune(
            &mut self.highpass,
            &on(butterworth(SvfShape::Highpass, filter.low_hz)),
        );
        retune(
            &mut self.lowpass,
            &on(butterworth(SvfShape::Lowpass, filter.high_hz)),
        );
        let notches: Vec<Tuning> = settings.notches.iter().map(notch).collect();
        retune(&mut self.notches, &notches);
        self.configure_peak(&settings.peak);

        if settings.auto_notch {
            self.auto_notch
                .get_or_insert_with(|| AutoNotch::new(f64::from(AUDIO_RATE)));
        } else {
            self.auto_notch = None;
        }

        let denoised = self.configure_denoise(settings.denoise, nets);

        match settings.agc.time_constants_s() {
            Some((attack_s, release_s)) if agc_changed || self.agc.is_none() => {
                self.agc = Some(Agc::new(
                    f64::from(AUDIO_RATE),
                    AGC_TARGET_RMS,
                    attack_s,
                    release_s,
                    AGC_MAX_GAIN,
                ));
            }
            Some(_) => {}
            None => self.agc = None,
        }
        denoised
    }

    fn configure_peak(&mut self, settings: &PeakSettings) {
        if !settings.enabled {
            self.peak = None;
            return;
        }
        let (shape, freq_hz, q) = peak(settings);
        let rate = f64::from(AUDIO_RATE);
        match &mut self.peak {
            Some(section) => section.retune(shape, rate, freq_hz, q),
            slot => *slot = Some(Svf::new(shape, rate, freq_hz, q)),
        }
    }

    fn configure_denoise(
        &mut self,
        settings: DenoiseSettings,
        nets: &dyn DenoiseNets,
    ) -> Result<(), NeuralDenoiseError> {
        match &mut self.denoise {
            Some(denoise) if settings.enabled && denoise.matches(settings) => {
                denoise.set_strength(settings.strength);
                Ok(())
            }
            slot if settings.enabled => {
                *slot = None;
                *slot = Some(Denoiser::build(settings, nets)?);
                Ok(())
            }
            slot => {
                *slot = None;
                Ok(())
            }
        }
    }

    fn tail(&self) -> usize {
        self.clicks.as_ref().map_or(0, ClickRemover::latency)
            + self.denoise.as_ref().map_or(0, Denoiser::tail)
    }

    fn reset(&mut self) {
        if let Some(clicks) = &mut self.clicks {
            clicks.reset();
        }
        for section in self.highpass.iter_mut().chain(&mut self.lowpass) {
            section.reset();
        }
        for section in self.notches.iter_mut().chain(&mut self.peak) {
            section.reset();
        }
        if let Some(auto_notch) = &mut self.auto_notch {
            auto_notch.reset();
        }
        if let Some(denoise) = &mut self.denoise {
            denoise.reset();
        }
    }

    fn process(&mut self, pcm: &mut [f32]) -> Result<(), NeuralDenoiseError> {
        if let Some(clicks) = &mut self.clicks {
            clicks.process(pcm);
        }
        for section in self.highpass.iter_mut().chain(&mut self.lowpass) {
            section.process(pcm);
        }
        for section in self.notches.iter_mut().chain(&mut self.peak) {
            section.process(pcm);
        }
        if let Some(auto_notch) = &mut self.auto_notch {
            auto_notch.process(pcm);
        }
        if let Some(denoise) = &mut self.denoise {
            denoise.process(pcm)?;
        }
        if let Some(agc) = &mut self.agc {
            agc.process(pcm);
        }
        Ok(())
    }
}

/// A section's shape, centre and Q.
type Tuning = (SvfShape, f64, f64);

/// Moves `sections` to `specs`, keeping the state of every section that stays so a moved
/// setting does not click.
fn retune(sections: &mut Vec<Svf>, specs: &[Tuning]) {
    let rate = f64::from(AUDIO_RATE);
    sections.truncate(specs.len());
    for (index, &(shape, freq_hz, q)) in specs.iter().enumerate() {
        match sections.get_mut(index) {
            Some(section) => section.retune(shape, rate, freq_hz, q),
            None => sections.push(Svf::new(shape, rate, freq_hz, q)),
        }
    }
}

fn butterworth(shape: SvfShape, freq_hz: f64) -> Vec<Tuning> {
    BUTTERWORTH_Q.iter().map(|&q| (shape, freq_hz, q)).collect()
}

fn q_of(freq_hz: f64, width_hz: f64) -> f64 {
    freq_hz / width_hz.max(f64::MIN_POSITIVE)
}

fn notch(settings: &NotchSettings) -> Tuning {
    (
        SvfShape::Notch,
        settings.freq_hz,
        q_of(settings.freq_hz, settings.width_hz),
    )
}

fn peak(settings: &PeakSettings) -> Tuning {
    (
        SvfShape::Peak {
            gain_db: settings.gain_db,
        },
        settings.freq_hz,
        q_of(settings.freq_hz, settings.width_hz),
    )
}
