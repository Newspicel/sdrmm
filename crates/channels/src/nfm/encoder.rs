//! The NFM encoder: voice pre-emphasised, optionally compressed or inverted, on an FM carrier.

use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{Compander, Limiter, RealDecimator};
use sdrmm_modem::keying::{Burst, TxQueue};
use sdrmm_wire::{Control, NumberSpec, SettingField};
use serde::{Deserialize, Serialize};

use super::{
    AudioPreemphasis, BANDWIDTHS_HZ, COMPANDER_HEADROOM_DB, COMPANDER_RANGE_DB,
    COMPANDER_REFERENCE_RMS, DESCRIPTOR, DEVIATION_RATIO, NfmParams, audio_lowpass,
};
use crate::{
    AUDIO_RATE,
    encoder::{
        EncoderBase, EncoderError, Modulator, NativeEncoder, NativeModulator, TxPayload, tx_limiter,
    },
    voice_inversion::{
        DEFAULT_INVERSION_HZ, MAX_INVERSION_HZ, MIN_INVERSION_HZ, VoiceInverter,
        is_supported_inversion,
    },
};

const TYPE_ID: &str = "nfm";

/// What an NFM encoder sends: its channel width, and how the voice is shaped before it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NfmEncoderSettings {
    pub bandwidth_hz: f64,
    pub inversion: bool,
    pub inversion_hz: f64,
    pub compander: bool,
}

impl Default for NfmEncoderSettings {
    fn default() -> Self {
        Self {
            bandwidth_hz: NfmParams::default().bandwidth_hz,
            inversion: false,
            inversion_hz: DEFAULT_INVERSION_HZ,
            compander: false,
        }
    }
}

pub struct NfmModulator {
    rate: f64,
    deviation_hz: f64,
    queue: TxQueue<f32>,
    audio_lp: RealDecimator,
    compressor: Option<Compander>,
    inverter: Option<VoiceInverter>,
    preemphasis: AudioPreemphasis,
    limiter: Limiter<f32>,
    filtered: Vec<f32>,
    burst: Burst,
    phase: f64,
}

fn tx_compressor(s: &NfmEncoderSettings, rate: f64) -> Option<Compander> {
    s.compander.then(|| {
        Compander::compressor(
            rate,
            COMPANDER_REFERENCE_RMS,
            COMPANDER_RANGE_DB,
            COMPANDER_HEADROOM_DB,
        )
    })
}

fn tx_inverter(s: &NfmEncoderSettings, rate: f64) -> Option<VoiceInverter> {
    s.inversion
        .then(|| VoiceInverter::new(rate, s.inversion_hz))
}

fn check_encoder(s: &NfmEncoderSettings) -> Result<(), EncoderError> {
    let rate = DESCRIPTOR.input_rate_hz;
    if !(s.bandwidth_hz.is_finite() && s.bandwidth_hz > 0.0 && s.bandwidth_hz < rate) {
        return Err(EncoderError::settings(
            TYPE_ID,
            format!("width must be in (0, {rate}) Hz, got {}", s.bandwidth_hz),
        ));
    }
    if s.inversion && !is_supported_inversion(s.inversion_hz) {
        return Err(EncoderError::settings(
            TYPE_ID,
            format!(
                "inversion needs a carrier in {MIN_INVERSION_HZ}..={MAX_INVERSION_HZ} Hz, got {}",
                s.inversion_hz
            ),
        ));
    }
    Ok(())
}

impl NativeModulator for NfmModulator {
    type Settings = NfmEncoderSettings;

    fn rate_hz() -> f64 {
        DESCRIPTOR.input_rate_hz
    }

    fn new(settings: &NfmEncoderSettings) -> Result<Self, EncoderError> {
        check_encoder(settings)?;
        let rate = DESCRIPTOR.input_rate_hz;
        Ok(Self {
            rate,
            deviation_hz: settings.bandwidth_hz / DEVIATION_RATIO,
            queue: TxQueue::new(TYPE_ID, f64::from(AUDIO_RATE)),
            audio_lp: audio_lowpass(),
            compressor: tx_compressor(settings, f64::from(AUDIO_RATE)),
            inverter: tx_inverter(settings, f64::from(AUDIO_RATE)),
            preemphasis: AudioPreemphasis::new(f64::from(AUDIO_RATE)),
            limiter: tx_limiter(f64::from(AUDIO_RATE)),
            filtered: Vec::new(),
            burst: Burst::new(rate),
            phase: 0.0,
        })
    }
}

impl Modulator for NfmModulator {
    fn submit(&mut self, payload: TxPayload) -> Result<(), EncoderError> {
        let pcm = payload.audio(TYPE_ID)?;
        self.queue.accept(pcm.len())?;
        self.audio_lp.process(&pcm, &mut self.filtered);
        if let Some(compressor) = &mut self.compressor {
            compressor.process(&mut self.filtered);
        }
        if let Some(inverter) = &mut self.inverter {
            inverter.process(&mut self.filtered);
        }
        self.preemphasis.process(&mut self.filtered);
        self.limiter.process(&mut self.filtered);
        self.queue.extend(self.filtered.iter().copied());
        Ok(())
    }

    fn generate(&mut self, out: &mut [Complex<f32>]) -> usize {
        let mut written = 0;
        for slot in out {
            let Some(envelope) = self.burst.next(!self.queue.is_empty()) else {
                break;
            };
            let audio = self.queue.pop().unwrap_or(0.0);
            self.phase += TAU * self.deviation_hz * f64::from(audio) / self.rate;
            if self.phase > TAU {
                self.phase -= TAU;
            } else if self.phase < -TAU {
                self.phase += TAU;
            }
            *slot = Complex::from_polar(envelope, self.phase as f32);
            written += 1;
        }
        written
    }
}

pub(crate) fn nfm_encoder() -> NativeEncoder<NfmEncoderSettings> {
    NativeEncoder::new::<NfmModulator>(
        EncoderBase {
            type_id: TYPE_ID,
            name: "NFM",
            summary: "Narrowband FM voice",
        },
        |s: &NfmEncoderSettings| s.bandwidth_hz,
    )
    .form(nfm_encoder_form)
}

fn nfm_encoder_form() -> Vec<SettingField> {
    vec![
        SettingField::bandwidth(&BANDWIDTHS_HZ),
        SettingField::new("inversion", "Inversion", Control::Toggle)
            .title("Scramble by inverting the voice"),
        SettingField::new(
            "inversion_hz",
            "Carrier",
            NumberSpec::hz(MIN_INVERSION_HZ, MAX_INVERSION_HZ, 50.0),
        )
        .title("Inversion carrier")
        .when("inversion", [true]),
        SettingField::new("compander", "Compander", Control::Toggle)
            .title("Compress audio 2:1 for a receiver that expands it"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{encoder::Carries, synth::burst};

    const RATE: f64 = 48_000.0;

    fn ramp_len() -> usize {
        Burst::new(RATE).ramp_len()
    }

    fn transmitter() -> NfmModulator {
        NfmModulator::new(&NfmEncoderSettings::default()).unwrap()
    }

    #[test]
    fn tx_ramps_the_burst_edges_and_holds_a_constant_envelope_between() {
        let ramp = ramp_len();
        let mut tx = transmitter();
        tx.submit(TxPayload::Audio(vec![0.0; 4_800])).unwrap();
        let iq = burst(&mut tx);

        assert!(iq[0].norm() < 0.01, "first sample {}", iq[0].norm());
        for (k, pair) in iq[..ramp].windows(2).enumerate() {
            assert!(pair[1].norm() > pair[0].norm(), "rise not monotonic at {k}");
        }
        let tail = &iq[iq.len() - ramp..];
        for (k, pair) in tail.windows(2).enumerate() {
            assert!(pair[1].norm() < pair[0].norm(), "fall not monotonic at {k}");
        }
        assert!(
            tail[ramp - 1].norm() < 0.01,
            "last sample {}",
            tail[ramp - 1].norm()
        );
        for (k, s) in iq[ramp..iq.len() - ramp].iter().enumerate() {
            assert!(
                (s.norm() - 1.0).abs() < 1e-5,
                "envelope {} at {k}",
                s.norm()
            );
        }
    }

    #[test]
    fn tx_radiates_nothing_until_audio_is_submitted() {
        let mut tx = transmitter();
        let mut block = [Complex::new(9.0, 9.0); 64];
        assert_eq!(tx.generate(&mut block), 0);
        assert_eq!(block[0], Complex::new(9.0, 9.0));
    }

    #[test]
    fn tx_rejects_a_frame_payload() {
        let mut tx = transmitter();
        let err = tx.submit(TxPayload::Frame(vec![0x7E]));
        assert!(matches!(
            err,
            Err(EncoderError::WrongPayload {
                takes: Carries::Audio,
                ..
            })
        ));
    }

    #[test]
    fn tx_refuses_a_backlog_past_the_queue_bound() {
        let mut tx = transmitter();
        let over = (sdrmm_modem::keying::MAX_QUEUE_S * f64::from(AUDIO_RATE)) as usize + 1;
        assert!(matches!(
            tx.submit(TxPayload::Audio(vec![0.0; over])),
            Err(EncoderError::Backlog(_))
        ));
        let mut block = [Complex::new(0.0, 0.0); 16];
        assert_eq!(tx.generate(&mut block), 0);
    }

    #[test]
    fn tx_refuses_a_width_past_its_rate() {
        let built = NfmModulator::new(&NfmEncoderSettings {
            bandwidth_hz: 96_000.0,
            ..NfmEncoderSettings::default()
        });
        assert!(matches!(built, Err(EncoderError::Settings { .. })));
    }
}
