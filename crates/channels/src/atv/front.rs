use std::f64::consts::{PI, TAU};

use num_complex::Complex;
use sdrmm_dsp::{
    Ddc, Deemphasis, RealDecimator, RealResampler, blackman_harris, design_lowpass, fast_arg,
    phase_diff_into,
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

pub(super) enum Detector {
    Synchronous(Box<Synchronous>),
    Discriminator(Discriminator),
}

impl Detector {
    /// Synchronous AM with a Nyquist slope: `video` and `ramp` are shares of the rate.
    pub fn am(video: f64, ramp: f64) -> Self {
        Self::Synchronous(Box::new(Synchronous::new(video, ramp)))
    }

    pub fn process(&mut self, iq: &[Complex<f32>], video: &mut Vec<f32>) {
        match self {
            Self::Synchronous(detector) => detector.process(iq, video),
            Self::Discriminator(fm) => fm.process(iq, video),
        }
    }
}

/// Samples per carrier loop update; the loop pulls in a carrier up to half a turn per block.
const CARRIER_BLOCK: usize = 32;
const CARRIER_PULL: f64 = 0.9 * PI / CARRIER_BLOCK as f64;
const CARRIER_KP: f32 = 0.02;
const CARRIER_KI: f64 = 2.5e-4;
const CARRIER_KF: f64 = 0.02;

/// Holds the vision carrier on the real axis: a frequency loop pulls it in, a phase loop
/// settles it, both updated once a block so the sample loop is a single rotation.
#[derive(Clone, Debug)]
struct CarrierLock {
    turn: Complex<f32>,
    step: Complex<f32>,
    freq: f64,
    sum: Complex<f32>,
    previous: Complex<f32>,
    count: usize,
}

impl CarrierLock {
    fn new() -> Self {
        Self {
            turn: Complex::new(1.0, 0.0),
            step: Complex::new(1.0, 0.0),
            freq: 0.0,
            sum: Complex::new(0.0, 0.0),
            previous: Complex::new(0.0, 0.0),
            count: 0,
        }
    }

    fn derotate(&mut self, iq: &[Complex<f32>], re: &mut Vec<f32>, im: &mut Vec<f32>) {
        re.clear();
        im.clear();
        re.resize(iq.len(), 0.0);
        im.resize(iq.len(), 0.0);
        let mut at = 0;
        while at < iq.len() {
            let take = (CARRIER_BLOCK - self.count).min(iq.len() - at);
            let end = at + take;
            let sum = self.turn_block(&iq[at..end], &mut re[at..end], &mut im[at..end]);
            self.sum += sum;
            self.count += take;
            at = end;
            if self.count == CARRIER_BLOCK {
                self.update();
            }
        }
    }

    /// Turns a stretch inside one block by four interleaved oscillators, so no sample waits
    /// on the one before; returns the stretch's sum.
    fn turn_block(&mut self, iq: &[Complex<f32>], re: &mut [f32], im: &mut [f32]) -> Complex<f32> {
        let step2 = self.step * self.step;
        let step4 = step2 * step2;
        let mut lanes = [
            self.turn,
            self.turn * self.step,
            self.turn * step2,
            self.turn * step2 * self.step,
        ];
        let mut sums = [Complex::new(0.0, 0.0); 4];
        let (chunks, tail) = iq.as_chunks::<4>();
        for (z, (r, i)) in chunks.iter().zip(
            re.as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(im.as_chunks_mut::<4>().0.iter_mut()),
        ) {
            for lane in 0..4 {
                let y = z[lane] * lanes[lane];
                sums[lane] += y;
                r[lane] = y.re;
                i[lane] = y.im;
                lanes[lane] *= step4;
            }
        }
        let done = chunks.len() * 4;
        let mut turn = lanes[0];
        for (k, z) in tail.iter().enumerate() {
            let y = z * turn;
            sums[0] += y;
            re[done + k] = y.re;
            im[done + k] = y.im;
            turn *= self.step;
        }
        self.turn = turn;
        sums.iter().sum()
    }

    fn update(&mut self) {
        let sum = std::mem::take(&mut self.sum);
        self.count = 0;
        let (error, drift) = if sum.norm_sqr() > 0.0 && sum.is_finite() {
            (fast_arg(sum), fast_arg(sum * self.previous.conj()))
        } else {
            (0.0, 0.0)
        };
        let correction = Complex::from_polar(1.0, -CARRIER_KP * error);
        self.previous = sum * correction;
        self.freq = (self.freq
            + (CARRIER_KI * f64::from(error) + CARRIER_KF * f64::from(drift))
                / CARRIER_BLOCK as f64)
            .clamp(-CARRIER_PULL, CARRIER_PULL);
        let turn = self.turn * correction;
        self.turn = turn / turn.norm();
        self.step = Complex::from_polar(1.0, -self.freq as f32);
    }
}

/// AM video detected against the recovered carrier through a Nyquist slope, so a vestigial
/// sideband comes out flat and its chroma unbent; a double sideband passes the same way.
#[derive(Clone, Debug)]
pub(super) struct Synchronous {
    carrier: CarrierLock,
    even: RealDecimator,
    odd: RealDecimator,
    re: Vec<f32>,
    im: Vec<f32>,
    even_out: Vec<f32>,
    odd_out: Vec<f32>,
}

impl Synchronous {
    fn new(video: f64, ramp: f64) -> Self {
        let (even, odd) = nyquist_taps(AM_TAPS, video, ramp);
        Self {
            carrier: CarrierLock::new(),
            even: RealDecimator::new(&even, 1),
            odd: RealDecimator::new(&odd, 1),
            re: Vec::new(),
            im: Vec::new(),
            even_out: Vec::new(),
            odd_out: Vec::new(),
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], video: &mut Vec<f32>) {
        self.carrier.derotate(iq, &mut self.re, &mut self.im);
        self.even.process(&self.re, &mut self.even_out);
        self.odd.process(&self.im, &mut self.odd_out);
        video.clear();
        video.extend(
            self.even_out
                .iter()
                .zip(&self.odd_out)
                .map(|(&re, &im)| re - im),
        );
    }
}

/// The real and imaginary taps of a filter that rises linearly across `-ramp..ramp` and
/// passes up to `video`: its slope and its mirror sum to one, so synchronous detection of
/// either sideband arrangement is flat.
pub(super) fn nyquist_taps(taps: usize, video: f64, ramp: f64) -> (Vec<f32>, Vec<f32>) {
    const GRID: usize = 2048;
    let centre = (taps - 1) as f64 / 2.0;
    let gain = |f: f64| {
        if f > video {
            0.0
        } else {
            ((f + ramp) / (2.0 * ramp)).clamp(0.0, 1.0)
        }
    };
    let window = blackman_harris(taps);
    let mut even = Vec::with_capacity(taps);
    let mut odd = Vec::with_capacity(taps);
    for (n, &w) in window.iter().enumerate() {
        let t = n as f64 - centre;
        let h: Complex<f64> = (0..GRID)
            .map(|k| {
                let f = k as f64 / GRID as f64 - 0.5;
                Complex::from_polar(gain(f), TAU * f * t)
            })
            .sum::<Complex<f64>>()
            / GRID as f64;
        even.push((h.re * f64::from(w)) as f32);
        odd.push((h.im * f64::from(w)) as f32);
    }
    let dc: f32 = even.iter().sum();
    let scale = if dc.abs() > f32::EPSILON {
        1.0 / dc
    } else {
        1.0
    };
    (
        even.iter().map(|t| t * scale).collect(),
        odd.iter().map(|t| t * scale).collect(),
    )
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
