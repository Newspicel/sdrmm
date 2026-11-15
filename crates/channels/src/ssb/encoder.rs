//! The SSB encoder: one sideband of the voice, phased by a Hilbert transformer.

use num_complex::Complex;
use sdrmm_dsp::{Limiter, RealDecimator, design_bandpass};
use sdrmm_modem::analog::{Delay, design_hilbert};
use sdrmm_modem::keying::{Burst, TxQueue};
use sdrmm_wire::Sideband;
use serde::{Deserialize, Serialize};

use super::{AUDIO_TAPS, DESCRIPTOR, PASSBAND_LOW_HZ, SsbParams, ssb_form};
use crate::{
    AUDIO_RATE,
    encoder::{
        EncoderBase, EncoderError, Modulator, NativeEncoder, NativeModulator, TxPayload, tx_limiter,
    },
};

const TYPE_ID: &str = "ssb";

const HILBERT_TAPS: usize = 257;

fn audio_bandpass(bandwidth_hz: f64) -> Result<RealDecimator, EncoderError> {
    let rate = DESCRIPTOR.input_rate_hz;
    if !(bandwidth_hz.is_finite() && bandwidth_hz > PASSBAND_LOW_HZ && bandwidth_hz < rate / 2.0) {
        return Err(EncoderError::settings(
            TYPE_ID,
            format!(
                "width must be in ({PASSBAND_LOW_HZ}, {}) Hz, got {bandwidth_hz}",
                rate / 2.0
            ),
        ));
    }
    Ok(RealDecimator::new(
        &design_bandpass(AUDIO_TAPS, PASSBAND_LOW_HZ / rate, bandwidth_hz / rate),
        1,
    ))
}

/// What an SSB encoder sends: the sideband and how wide its audio runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SsbEncoderSettings {
    pub sideband: Sideband,
    pub bandwidth_hz: f64,
}

impl Default for SsbEncoderSettings {
    fn default() -> Self {
        let heard = SsbParams::default();
        Self {
            sideband: heard.sideband,
            bandwidth_hz: heard.bandwidth_hz,
        }
    }
}

pub struct SsbModulator {
    queue: TxQueue<Complex<f32>>,
    sideband: Sideband,
    audio_bp: RealDecimator,
    hilbert: RealDecimator,
    delay: Delay,
    limiter: Limiter<Complex<f32>>,
    shaped: Vec<f32>,
    in_phase: Vec<f32>,
    quadrature: Vec<f32>,
    baseband: Vec<Complex<f32>>,
    burst: Burst,
}

impl NativeModulator for SsbModulator {
    type Settings = SsbEncoderSettings;

    fn rate_hz() -> f64 {
        DESCRIPTOR.input_rate_hz
    }

    fn new(settings: &SsbEncoderSettings) -> Result<Self, EncoderError> {
        Ok(Self {
            queue: TxQueue::new(TYPE_ID, f64::from(AUDIO_RATE)),
            sideband: settings.sideband,
            audio_bp: audio_bandpass(settings.bandwidth_hz)?,
            hilbert: RealDecimator::new(&design_hilbert(HILBERT_TAPS), 1),
            delay: Delay::new(HILBERT_TAPS / 2),
            limiter: tx_limiter(f64::from(AUDIO_RATE)),
            shaped: Vec::new(),
            in_phase: Vec::new(),
            quadrature: Vec::new(),
            baseband: Vec::new(),
            burst: Burst::new(DESCRIPTOR.input_rate_hz),
        })
    }
}

impl Modulator for SsbModulator {
    fn submit(&mut self, payload: TxPayload) -> Result<(), EncoderError> {
        let pcm = payload.audio(TYPE_ID)?;
        self.queue.accept(pcm.len())?;
        self.audio_bp.process(&pcm, &mut self.shaped);
        self.hilbert.process(&self.shaped, &mut self.quadrature);
        self.delay.process(&self.shaped, &mut self.in_phase);
        let sign = match self.sideband {
            Sideband::Usb => 1.0,
            Sideband::Lsb => -1.0,
        };
        self.baseband.clear();
        self.baseband.extend(
            self.in_phase
                .iter()
                .zip(&self.quadrature)
                .map(|(&i, &q)| Complex::new(i, sign * q)),
        );
        self.limiter.process(&mut self.baseband);
        self.queue.extend(self.baseband.iter().copied());
        Ok(())
    }

    fn generate(&mut self, out: &mut [Complex<f32>]) -> usize {
        let mut written = 0;
        for slot in out {
            let Some(envelope) = self.burst.next(!self.queue.is_empty()) else {
                break;
            };
            *slot = self.queue.pop().unwrap_or_default() * envelope;
            written += 1;
        }
        written
    }
}

pub(crate) fn ssb_encoder() -> NativeEncoder<SsbEncoderSettings> {
    NativeEncoder::new::<SsbModulator>(
        EncoderBase {
            type_id: TYPE_ID,
            name: "SSB",
            summary: "Single sideband voice",
        },
        |s: &SsbEncoderSettings| s.bandwidth_hz,
    )
    .form(ssb_form)
}
