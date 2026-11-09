use num_complex::Complex;
use sdrmm_dsp::{
    Ddc, Decimator, Deemphasis, RealDecimator, RealResampler, design_lowpass, phase_diff_into,
};
use sdrmm_modem::analog::{AngleDemod, AngleDetector, AngleKind, AngleParams, AngleRx};

use crate::{AUDIO_RATE, ChannelError, ChannelOutputs, clamp_full_scale};

const AM_TAPS: usize = 63;
const RESAMPLER_DB: f64 = 50.0;
const SOUND_IF_RATE_HZ: f64 = 240_000.0;
pub(super) const SOUND_DEVIATION_HZ: f64 = 50_000.0;
pub(super) const SOUND_AUDIO_HZ: f64 = 15_000.0;
const SOUND_DECIM: usize = 5;
const SOUND_TAPS: usize = 199;

/// Phase step per sample, in radians.
#[derive(Clone, Debug)]
pub(super) struct Discriminator {
    prev: Complex<f32>,
}

impl Discriminator {
    pub fn new() -> Self {
        Self {
            prev: Complex::new(1.0, 0.0),
        }
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<f32>) {
        phase_diff_into(iq, &mut self.prev, 1.0, out);
    }
}

#[derive(Clone, Debug)]
pub(super) enum Detector {
    Envelope(Box<Decimator>, Vec<Complex<f32>>),
    Discriminator(Discriminator),
}

impl Detector {
    pub fn am(cutoff: f64) -> Self {
        Self::Envelope(
            Box::new(Decimator::new(
                &design_lowpass(AM_TAPS, cutoff.min(0.49)),
                1,
            )),
            Vec::new(),
        )
    }

    pub fn process(&mut self, iq: &[Complex<f32>], video: &mut Vec<f32>) {
        match self {
            Self::Envelope(filter, filtered) => {
                filter.process(iq, filtered);
                video.clear();
                video.extend(filtered.iter().map(|sample| sample.norm()));
            }
            Self::Discriminator(fm) => fm.process(iq, video),
        }
    }
}

/// The detected video taken from the channel rate to the line rate.
#[derive(Clone, Debug)]
pub(super) enum Rate {
    Same,
    Resample(RealResampler),
}

impl Rate {
    /// Keeps `keep_hz` of video and rejects only what would fold back into it.
    pub fn new(input_rate: f64, video_rate: f64, keep_hz: f64) -> Result<Self, ChannelError> {
        if (video_rate - input_rate).abs() < 1.0 {
            return Ok(Self::Same);
        }
        let stop = input_rate.min(video_rate) - keep_hz;
        if stop <= keep_hz {
            return Err(ChannelError::InvalidSettings(format!(
                "atv needs {keep_hz:.0} Hz of video, more than {input_rate:.0} Hz sampling carries"
            )));
        }
        Ok(Self::Resample(RealResampler::new(
            video_rate / input_rate,
            keep_hz / input_rate,
            stop / input_rate,
            RESAMPLER_DB,
        )))
    }

    pub fn process(&mut self, video: &[f32], out: &mut Vec<f32>) {
        match self {
            Self::Same => {
                out.clear();
                out.extend_from_slice(video);
            }
            Self::Resample(resampler) => resampler.process(video, out),
        }
    }
}

pub(super) struct SoundDecoder {
    ddc: Ddc,
    discriminator: AngleDemod,
    decimator: RealDecimator,
    deemphasis: Deemphasis,
    baseband: Vec<Complex<f32>>,
    demodulated: Vec<f32>,
    real_iq: Vec<Complex<f32>>,
}

impl SoundDecoder {
    pub fn new(rate: f64, carrier_hz: f64, deemphasis_us: f32) -> Result<Self, ChannelError> {
        let ddc = Ddc::new(rate, SOUND_IF_RATE_HZ, carrier_hz)
            .map_err(|error| ChannelError::InvalidSettings(error.to_string()))?;
        Ok(Self {
            ddc,
            discriminator: AngleDemod::new(
                &AngleParams::new(
                    AngleKind::Fm {
                        deviation: SOUND_DEVIATION_HZ / SOUND_IF_RATE_HZ,
                    },
                    SOUND_AUDIO_HZ / SOUND_IF_RATE_HZ,
                ),
                &AngleRx::detector_only(AngleDetector::Discriminator),
            ),
            decimator: RealDecimator::new(
                &design_lowpass(SOUND_TAPS, SOUND_AUDIO_HZ / SOUND_IF_RATE_HZ),
                SOUND_DECIM,
            ),
            deemphasis: Deemphasis::new(f64::from(AUDIO_RATE), deemphasis_us),
            baseband: Vec::new(),
            demodulated: Vec::new(),
            real_iq: Vec::new(),
        })
    }

    pub fn process_iq(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.ddc.process(iq, &mut self.baseband);
        self.finish(out);
    }

    pub fn process_composite(&mut self, video: &[f32], out: &mut ChannelOutputs) {
        self.real_iq.clear();
        self.real_iq
            .extend(video.iter().map(|&sample| Complex::new(sample, 0.0)));
        self.ddc.process(&self.real_iq, &mut self.baseband);
        self.finish(out);
    }

    fn finish(&mut self, out: &mut ChannelOutputs) {
        self.discriminator
            .process(&self.baseband, &mut self.demodulated);
        self.decimator
            .process(&self.demodulated, &mut out.audio_pcm);
        self.deemphasis.process(&mut out.audio_pcm);
        clamp_full_scale(&mut out.audio_pcm);
        if !out.audio_pcm.is_empty() {
            out.audio_rate = AUDIO_RATE;
        }
    }
}
