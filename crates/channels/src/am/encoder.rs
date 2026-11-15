//! The AM encoder: audio cut to the channel width on a full carrier.

use num_complex::Complex;
use sdrmm_dsp::{Limiter, RealDecimator, design_lowpass};
use sdrmm_modem::keying::{Burst, TxQueue};
use sdrmm_wire::SettingField;
use serde::{Deserialize, Serialize};

use super::{AUDIO_TAPS, AmParams, BANDWIDTHS_HZ, DESCRIPTOR, MODULATION_DEPTH};
use crate::{
    AUDIO_RATE,
    encoder::{
        EncoderBase, EncoderError, Modulator, NativeEncoder, NativeModulator, TxPayload, tx_limiter,
    },
};

const TYPE_ID: &str = "am";

/// What an AM encoder sends: the width its audio is cut to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AmEncoderSettings {
    pub bandwidth_hz: f64,
}

impl Default for AmEncoderSettings {
    fn default() -> Self {
        Self {
            bandwidth_hz: AmParams::default().bandwidth_hz,
        }
    }
}

pub struct AmModulator {
    queue: TxQueue<f32>,
    audio_lp: RealDecimator,
    limiter: Limiter<f32>,
    filtered: Vec<f32>,
    burst: Burst,
}

impl NativeModulator for AmModulator {
    type Settings = AmEncoderSettings;

    fn rate_hz() -> f64 {
        DESCRIPTOR.input_rate_hz
    }

    fn new(settings: &AmEncoderSettings) -> Result<Self, EncoderError> {
        Ok(Self {
            queue: TxQueue::new(TYPE_ID, f64::from(AUDIO_RATE)),
            audio_lp: audio_lowpass(settings.bandwidth_hz)?,
            limiter: tx_limiter(f64::from(AUDIO_RATE)),
            filtered: Vec::new(),
            burst: Burst::new(DESCRIPTOR.input_rate_hz),
        })
    }
}

impl Modulator for AmModulator {
    fn submit(&mut self, payload: TxPayload) -> Result<(), EncoderError> {
        let pcm = payload.audio(TYPE_ID)?;
        self.queue.accept(pcm.len())?;
        self.audio_lp.process(&pcm, &mut self.filtered);
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
            let modulated = (1.0 + MODULATION_DEPTH * audio) / (1.0 + MODULATION_DEPTH);
            *slot = Complex::new(envelope * modulated, 0.0);
            written += 1;
        }
        written
    }
}

fn audio_lowpass(bandwidth_hz: f64) -> Result<RealDecimator, EncoderError> {
    let rate = DESCRIPTOR.input_rate_hz;
    if !(bandwidth_hz.is_finite() && bandwidth_hz > 0.0 && bandwidth_hz < rate) {
        return Err(EncoderError::settings(
            TYPE_ID,
            format!("width must be in (0, {rate}) Hz, got {bandwidth_hz}"),
        ));
    }
    let cutoff = bandwidth_hz / 2.0 / DESCRIPTOR.input_rate_hz;
    Ok(RealDecimator::new(&design_lowpass(AUDIO_TAPS, cutoff), 1))
}

pub(crate) fn am_encoder() -> NativeEncoder<AmEncoderSettings> {
    NativeEncoder::new::<AmModulator>(
        EncoderBase {
            type_id: TYPE_ID,
            name: "AM",
            summary: "AM voice",
        },
        |s: &AmEncoderSettings| s.bandwidth_hz,
    )
    .form(am_encoder_form)
}

fn am_encoder_form() -> Vec<SettingField> {
    vec![SettingField::bandwidth(&BANDWIDTHS_HZ)]
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{ChannelParams, ChannelSettings};

    use super::*;
    use crate::{
        ChannelCtx, NativeChannel,
        am::AmChannel,
        encoder::Carries,
        synth::{burst, tone_audio},
        testutil::{dominant_tone, rms, run_ragged, settings},
    };

    const RATE: f64 = 48_000.0;

    const fn ctx() -> ChannelCtx {
        ChannelCtx { input_rate: RATE }
    }

    fn tx_params() -> ChannelSettings {
        settings(
            ChannelParams::of(&AmParams {
                bandwidth_hz: 10_000.0,
                ..AmParams::default()
            })
            .expect("am settings"),
        )
    }

    fn transmitter() -> AmModulator {
        AmModulator::new(&AmEncoderSettings::default()).unwrap()
    }

    #[test]
    fn tx_round_trips_a_tone_through_the_detector() {
        let mut tx = transmitter();
        tx.submit(TxPayload::Audio(tone_audio(1_000.0, 1.0, RATE, 24_000)))
            .unwrap();
        let iq = burst(&mut tx);
        assert_eq!(iq.len(), 24_000 + Burst::new(RATE).ramp_len());

        let mut rx = AmChannel::new(ctx(), tx_params()).unwrap();
        let audio = run_ragged(&mut rx, &iq);
        let window = &audio[4_000..20_000];
        let (freq, ratio) = dominant_tone(window, RATE);
        assert!((995.0..1_005.0).contains(&freq), "dominant {freq} Hz");
        assert!(ratio > 10.0, "tone-to-rest ratio {ratio}");
        let amplitude = rms(window);
        assert!((0.2..0.3).contains(&amplitude), "rms {amplitude}");
    }

    #[test]
    fn tx_keeps_the_envelope_between_the_trough_and_full_scale() {
        let mut tx = transmitter();
        tx.submit(TxPayload::Audio(tone_audio(1_000.0, 4.0, RATE, 24_000)))
            .unwrap();
        let iq = burst(&mut tx);
        let ramp = Burst::new(RATE).ramp_len();
        let keyed = &iq[ramp..iq.len() - ramp];
        let peak = keyed.iter().map(|s| s.norm()).fold(f32::MIN, f32::max);
        let trough = keyed.iter().map(|s| s.norm()).fold(f32::MAX, f32::min);
        assert!((peak - 1.0).abs() < 0.02, "peak envelope {peak}");
        let floor = (1.0 - MODULATION_DEPTH) / (1.0 + MODULATION_DEPTH);
        assert!(trough > floor - 0.02, "trough envelope {trough}");
    }

    fn bin_power(x: &[f32], freq_hz: f64) -> f64 {
        let w = std::f64::consts::TAU * freq_hz / RATE;
        let (re, im) = x.iter().enumerate().fold((0.0, 0.0), |(re, im), (n, &v)| {
            let phase = w * n as f64;
            (
                re + f64::from(v) * phase.cos(),
                im - f64::from(v) * phase.sin(),
            )
        });
        re * re + im * im
    }

    #[test]
    fn tx_holds_a_loud_tone_to_full_scale_without_distorting_it() {
        let mut tx = transmitter();
        tx.submit(TxPayload::Audio(tone_audio(1_000.0, 4.0, RATE, 24_000)))
            .unwrap();
        let iq = burst(&mut tx);
        let envelope: Vec<f32> = iq[12_000..12_000 + 4_800].iter().map(|s| s.re).collect();
        let harmonic = bin_power(&envelope, 3_000.0) / bin_power(&envelope, 1_000.0);
        assert!(
            10.0 * harmonic.log10() < -40.0,
            "third harmonic {} dB",
            10.0 * harmonic.log10()
        );
    }

    #[test]
    fn tx_ramps_the_burst_edges() {
        let mut tx = transmitter();
        tx.submit(TxPayload::Audio(vec![0.0; 4_800])).unwrap();
        let iq = burst(&mut tx);
        let ramp = Burst::new(RATE).ramp_len();
        assert!(iq[0].norm() < 0.01, "first sample {}", iq[0].norm());
        assert!(
            iq[iq.len() - 1].norm() < 0.01,
            "last sample {}",
            iq[iq.len() - 1].norm()
        );
        for (k, s) in iq[ramp..iq.len() - ramp].iter().enumerate() {
            let unmodulated = 1.0 / (1.0 + MODULATION_DEPTH);
            assert!(
                (s.norm() - unmodulated).abs() < 1e-5,
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
    fn tx_rejects_a_frame_payload_and_a_backlog_past_the_bound() {
        let mut tx = transmitter();
        assert!(matches!(
            tx.submit(TxPayload::Frame(vec![0x7E])),
            Err(EncoderError::WrongPayload {
                takes: Carries::Audio,
                ..
            })
        ));
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
        let built = AmModulator::new(&AmEncoderSettings {
            bandwidth_hz: 96_000.0,
        });
        assert!(matches!(built, Err(EncoderError::Settings { .. })));
    }
}
