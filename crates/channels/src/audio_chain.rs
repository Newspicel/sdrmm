use sdrmm_dsp::{Agc, AutoNotch, ClickRemover, SpectralDenoiser, Svf, SvfShape};
use std::sync::Arc;

use sdrmm_wire::{
    AudioProcessing, ChannelParams, DenoiseMode, DenoiseSettings, NotchSettings, PeakSettings,
};

use crate::{
    AUDIO_RATE,
    neural_denoise::{DenoiseNets, NeuralDenoiseError, NeuralDenoiser},
};

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

#[cfg(test)]
mod tests {
    use crate::ssb::SsbParams;
    use sdrmm_wire::{
        AudioAgcMode, AudioFilterSettings, ClickRemovalSettings, DenoiseSettings,
        NotchSettings as Notch,
    };

    use super::*;
    use crate::{
        neural_denoise::FixtureNets,
        testutil::{rms, tone_amplitude},
    };

    const RATE: f64 = AUDIO_RATE as f64;

    fn chain(settings: &AudioProcessing) -> AudioChain {
        AudioChain::new(
            1,
            settings,
            ClickProfile::Discriminator,
            Arc::new(FixtureNets),
        )
        .expect("chain builds")
    }

    fn tone(freq_hz: f64, amplitude: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|n| amplitude * (std::f64::consts::TAU * freq_hz * n as f64 / RATE).sin() as f32)
            .collect()
    }

    fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b).map(|(x, y)| x + y).collect()
    }

    fn run(chain: &mut AudioChain, pcm: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(pcm.len());
        let mut pos = 0;
        for len in [997usize, 4_096, 65, 2_048].iter().cycle() {
            if pos >= pcm.len() {
                break;
            }
            let end = (pos + len).min(pcm.len());
            let mut block = pcm[pos..end].to_vec();
            chain.process_audio(&mut block).expect("chain runs");
            out.extend_from_slice(&block);
            pos = end;
        }
        out
    }

    #[test]
    fn a_default_chain_passes_audio_through_untouched() {
        let input = tone(1_000.0, 0.3, 24_000);
        let output = run(&mut chain(&AudioProcessing::default()), &input);
        assert_eq!(output, input);
    }

    #[test]
    fn every_stage_returns_exactly_what_it_was_given() {
        let settings = AudioProcessing {
            click_removal: ClickRemovalSettings {
                enabled: true,
                threshold: 6.0,
            },
            filter: AudioFilterSettings {
                enabled: true,
                low_hz: 300.0,
                high_hz: 3_000.0,
            },
            notches: vec![Notch {
                freq_hz: 1_500.0,
                width_hz: 80.0,
            }],
            peak: PeakSettings {
                enabled: true,
                ..PeakSettings::default()
            },
            auto_notch: true,
            denoise: DenoiseSettings {
                enabled: true,
                mode: DenoiseMode::Neural,
                strength: 0.8,
                ..DenoiseSettings::default()
            },
            agc: AudioAgcMode::Medium,
        };
        let input = tone(1_000.0, 0.3, 48_000);
        assert_eq!(run(&mut chain(&settings), &input).len(), input.len());
    }

    #[test]
    fn the_passband_keeps_the_voice_band_and_drops_what_is_outside_it() {
        let settings = AudioProcessing {
            filter: AudioFilterSettings {
                enabled: true,
                low_hz: 300.0,
                high_hz: 3_000.0,
            },
            ..AudioProcessing::default()
        };
        for (freq_hz, kept) in [(100.0, false), (1_000.0, true), (6_000.0, false)] {
            let output = run(&mut chain(&settings), &tone(freq_hz, 0.5, 48_000));
            let level = rms(&output[24_000..]);
            if kept {
                assert!(level > 0.3, "{freq_hz} Hz was cut: {level}");
            } else {
                assert!(level < 0.02, "{freq_hz} Hz survived: {level}");
            }
        }
    }

    #[test]
    fn a_notch_removes_its_own_tone_and_leaves_the_voice_beside_it() {
        let settings = AudioProcessing {
            notches: vec![Notch {
                freq_hz: 1_500.0,
                width_hz: 60.0,
            }],
            ..AudioProcessing::default()
        };
        let input = mix(&tone(1_500.0, 0.4, 48_000), &tone(800.0, 0.4, 48_000));
        let output = run(&mut chain(&settings), &input);
        let settled = &output[24_000..];
        assert!(tone_amplitude(settled, 1_500.0, RATE) < 0.02);
        assert!(tone_amplitude(settled, 800.0, RATE) > 0.35);
    }

    fn peaked(freq_hz: f64) -> AudioProcessing {
        AudioProcessing {
            peak: PeakSettings {
                enabled: true,
                freq_hz,
                width_hz: 100.0,
                gain_db: 12.0,
            },
            ..AudioProcessing::default()
        }
    }

    #[test]
    fn the_peak_lifts_its_tone_over_the_rest() {
        let input = mix(&tone(700.0, 0.1, 48_000), &tone(1_500.0, 0.1, 48_000));
        let output = run(&mut chain(&peaked(700.0)), &input);
        let settled = &output[24_000..];
        let lifted = tone_amplitude(settled, 700.0, RATE);
        let left = tone_amplitude(settled, 1_500.0, RATE);
        assert!((0.37..0.43).contains(&lifted), "centre {lifted}");
        assert!((0.09..0.12).contains(&left), "neighbour {left}");
    }

    #[test]
    fn moving_the_peak_keeps_the_audio_running() {
        let mut chain = chain(&peaked(700.0));
        let input = tone(720.0, 0.1, 9_600);
        let mut head = run(&mut chain, &input[..4_800]);
        chain
            .configure(1, &peaked(740.0), ClickProfile::Discriminator)
            .expect("chain reconfigures");
        head.extend(run(&mut chain, &input[4_800..]));
        let jump = (head[4_800] - head[4_799]).abs();
        let swing = head[4_700..4_799]
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(jump < swing * 1.5, "jump {jump} against {swing}");
    }

    #[test]
    fn the_auto_notch_finds_a_carrier_nobody_pointed_at_it() {
        let settings = AudioProcessing {
            auto_notch: true,
            ..AudioProcessing::default()
        };
        let input = tone(1_200.0, 0.5, 96_000);
        let output = run(&mut chain(&settings), &input);
        assert!(tone_amplitude(&output[48_000..], 1_200.0, RATE) < 0.1);
    }

    #[test]
    fn the_agc_lifts_a_quiet_signal_to_its_target() {
        let settings = AudioProcessing {
            agc: AudioAgcMode::Fast,
            ..AudioProcessing::default()
        };
        let output = run(&mut chain(&settings), &tone(1_000.0, 0.01, 96_000));
        let level = rms(&output[72_000..]);
        assert!((0.2..0.3).contains(&level), "levelled to {level}");
    }

    #[test]
    fn switching_the_denoiser_mode_swaps_the_denoiser_and_keeps_the_length() {
        let mut chain = chain(&AudioProcessing {
            denoise: DenoiseSettings {
                enabled: true,
                ..DenoiseSettings::default()
            },
            ..AudioProcessing::default()
        });
        let input = tone(700.0, 0.2, 9_600);
        assert_eq!(run(&mut chain, &input).len(), input.len());
        chain
            .configure(
                1,
                &AudioProcessing {
                    denoise: DenoiseSettings {
                        enabled: true,
                        mode: DenoiseMode::Neural,
                        strength: 1.0,
                        ..DenoiseSettings::default()
                    },
                    ..AudioProcessing::default()
                },
                ClickProfile::Discriminator,
            )
            .expect("the neural denoiser loads");
        assert_eq!(run(&mut chain, &input).len(), input.len());
    }

    #[test]
    fn click_removal_takes_the_impulses_out_of_the_audio() {
        let settings = AudioProcessing {
            click_removal: ClickRemovalSettings {
                enabled: true,
                threshold: 6.0,
            },
            ..AudioProcessing::default()
        };
        let mut input = tone(800.0, 0.2, 48_000);
        for n in (1_000..input.len()).step_by(700) {
            input[n] = 3.0;
        }
        let output = run(&mut chain(&settings), &input);
        let settled = &output[8_000..];
        let peak = settled.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(peak < 0.3, "a click survived at {peak}");
        assert!(
            tone_amplitude(settled, 800.0, RATE) > 0.18,
            "the audio under them went with them"
        );
    }

    #[test]
    fn a_detector_mode_removes_a_wider_click_than_a_discriminator_does() {
        let settings = AudioProcessing {
            click_removal: ClickRemovalSettings {
                enabled: true,
                threshold: 6.0,
            },
            ..AudioProcessing::default()
        };
        let mut input = tone(800.0, 0.2, 48_000);
        for n in (1_000..input.len() - 8).step_by(700) {
            input[n..n + 6].fill(3.0);
        }
        let peak_with = |profile| {
            let mut chain = AudioChain::new(1, &settings, profile, Arc::new(FixtureNets))
                .expect("chain builds");
            run(&mut chain, &input)[8_000..]
                .iter()
                .fold(0.0f32, |a, s| a.max(s.abs()))
        };
        assert!(peak_with(ClickProfile::Discriminator) > 1.0);
        assert!(peak_with(ClickProfile::Detector) < 0.3);
    }

    #[test]
    fn every_mode_lands_on_the_detector_that_made_its_audio() {
        use crate::dv::DmrParams;
        use crate::nfm::NfmParams;
        use crate::wfm::WfmParams;
        for (params, profile) in [
            (
                ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings"),
                ClickProfile::Discriminator,
            ),
            (
                ChannelParams::new("wfm", &WfmParams::default()).expect("wfm settings"),
                ClickProfile::Discriminator,
            ),
            (
                ChannelParams::new("ssb", &SsbParams::default()).expect("ssb settings"),
                ClickProfile::Detector,
            ),
            (
                ChannelParams::new("dmr", &DmrParams::default()).expect("dmr settings"),
                ClickProfile::Vocoder,
            ),
        ] {
            assert_eq!(click_profile(&params), profile, "{params:?}");
        }
    }

    #[test]
    fn stereo_planes_are_filtered_independently() {
        let settings = AudioProcessing {
            notches: vec![Notch {
                freq_hz: 1_500.0,
                width_hz: 60.0,
            }],
            ..AudioProcessing::default()
        };
        let left = tone(1_500.0, 0.5, 48_000);
        let right = tone(800.0, 0.5, 48_000);
        let mut interleaved: Vec<f32> = left
            .iter()
            .zip(&right)
            .flat_map(|(&l, &r)| [l, r])
            .collect();
        AudioChain::new(
            2,
            &settings,
            ClickProfile::Discriminator,
            Arc::new(FixtureNets),
        )
        .expect("chain builds")
        .process_audio(&mut interleaved)
        .expect("chain runs");

        let taken: Vec<f32> = interleaved
            .iter()
            .skip(2 * 24_000)
            .step_by(2)
            .copied()
            .collect();
        let kept: Vec<f32> = interleaved
            .iter()
            .skip(2 * 24_000 + 1)
            .step_by(2)
            .copied()
            .collect();
        assert!(
            tone_amplitude(&taken, 1_500.0, RATE) < 0.03,
            "notch missed its side"
        );
        assert!(
            tone_amplitude(&kept, 800.0, RATE) > 0.4,
            "the other side was cut"
        );
    }

    #[test]
    fn an_unrelated_edit_keeps_the_agc_where_it_was() {
        let mut chain = chain(&AudioProcessing {
            agc: AudioAgcMode::Fast,
            ..AudioProcessing::default()
        });
        let quiet = tone(1_000.0, 0.01, 96_000);
        run(&mut chain, &quiet);
        chain
            .configure(
                1,
                &AudioProcessing {
                    agc: AudioAgcMode::Fast,
                    auto_notch: false,
                    notches: vec![Notch {
                        freq_hz: 2_000.0,
                        width_hz: 100.0,
                    }],
                    ..AudioProcessing::default()
                },
                ClickProfile::Discriminator,
            )
            .expect("chain reconfigures");
        let mut block = tone(1_000.0, 0.01, 1_200);
        chain.process_audio(&mut block).expect("chain runs");
        let level = rms(&block);
        assert!((0.15..0.35).contains(&level), "agc restarted: {level}");
    }

    #[test]
    fn turning_a_stage_off_takes_it_out_of_the_path() {
        let mut chain = chain(&AudioProcessing {
            filter: AudioFilterSettings {
                enabled: true,
                low_hz: 300.0,
                high_hz: 3_000.0,
            },
            ..AudioProcessing::default()
        });
        run(&mut chain, &tone(100.0, 0.5, 24_000));
        chain
            .configure(1, &AudioProcessing::default(), ClickProfile::Discriminator)
            .expect("chain reconfigures");
        let output = run(&mut chain, &tone(100.0, 0.5, 24_000));
        assert!(rms(&output[12_000..]) > 0.3, "the filter kept filtering");
    }
}
