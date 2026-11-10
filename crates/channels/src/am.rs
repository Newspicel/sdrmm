use crate::registry::Native;
use sdrmm_wire::{ClickProfile, Control, KindSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
pub use model::*;
use std::{f64::consts::TAU, sync::LazyLock};

use num_complex::Complex;
use sdrmm_dsp::{Agc, Decimator, Deemphasis, RealDecimator, design_lowpass};
use sdrmm_modem::analog::{AmDemod, AmDetector, AmMode, AmParams as AmWaveform, AmRx};
use sdrmm_wire::{CallGate, ChannelDescriptor, ChannelSettings, DecoderFamily};

use crate::{
    AUDIO_RATE, ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, ChannelTx,
    NativeChannel, NativeTx, TxPayload, check_input_rate, clamp_full_scale,
    tx::{Burst, TxQueue},
    voice_leveller,
};

const AUDIO_TAPS: usize = 129;
const CHANNEL_TAPS: usize = 129;
const SYNC_LOOP_BW: f64 = 1e-3;
const ROLLOFF_HZ: f64 = 2_500.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "am".to_owned(),
    name: "AM".to_owned(),
    summary: "AM voice, airband and shortwave".to_owned(),
    family: DecoderFamily::ANALOG_VOICE,
    bandwidth_hz: 10_000.0,
    input_rate_hz: 48_000.0,
    has_audio: true,
    calls: Some(CallGate::Squelch),
    click: ClickProfile::Detector,
    levels_audio: true,
    analog_voice: true,
    ..ChannelDescriptor::default()
});

pub struct AmChannel {
    demod: AmDemod,
    rolloff: Deemphasis,
    leveller: Agc,
}

/// A detector's RC load: one pole at [`ROLLOFF_HZ`]. The channel's noise is white and speech
/// is not, so tilting the treble down takes more hiss than voice.
fn rolloff() -> Deemphasis {
    Deemphasis::new(f64::from(AUDIO_RATE), (1e6 / (TAU * ROLLOFF_HZ)) as f32)
}

fn params(settings: &ChannelSettings) -> Result<AmParams, ChannelError> {
    crate::settings_of::<AmParams>(settings)
}

fn check_bandwidth(p: &AmParams) -> Result<(), ChannelError> {
    let rate = DESCRIPTOR.input_rate_hz;
    if p.bandwidth_hz.is_finite() && p.bandwidth_hz > 0.0 && p.bandwidth_hz < rate {
        Ok(())
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "am bandwidth must be in (0, {rate}) Hz, got {}",
            p.bandwidth_hz
        )))
    }
}

pub(crate) fn channel_filter(p: &AmParams) -> Result<ChannelFilter, ChannelError> {
    check_bandwidth(p)?;
    let cutoff = p.bandwidth_hz / 2.0 / DESCRIPTOR.input_rate_hz;
    Ok(ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(CHANNEL_TAPS, cutoff),
        1,
    )))
}

fn audio_lowpass(p: &AmParams) -> Result<RealDecimator, ChannelError> {
    check_bandwidth(p)?;
    let cutoff = p.bandwidth_hz / 2.0 / DESCRIPTOR.input_rate_hz;
    Ok(RealDecimator::new(&design_lowpass(AUDIO_TAPS, cutoff), 1))
}

fn waveform(p: &AmParams) -> Result<AmWaveform, ChannelError> {
    check_bandwidth(p)?;
    let mut waveform = AmWaveform::new(
        AmMode::FullCarrier {
            depth: f64::from(MODULATION_DEPTH),
        },
        p.bandwidth_hz / 2.0 / DESCRIPTOR.input_rate_hz,
    );
    waveform.audio_taps = AUDIO_TAPS;
    Ok(waveform)
}

fn demodulator(p: &AmParams) -> Result<AmDemod, ChannelError> {
    let detector = if p.sync {
        AmDetector::Synchronous {
            loop_bw: SYNC_LOOP_BW,
        }
    } else {
        AmDetector::Envelope
    };
    Ok(AmDemod::new(
        &waveform(p)?,
        &AmRx {
            predetection: false,
            ..AmRx::new(detector)
        },
    ))
}

impl NativeChannel for AmChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let p = params(&settings)?;
        Ok(Self {
            demod: demodulator(&p)?,
            rolloff: rolloff(),
            leveller: voice_leveller(),
        })
    }
}

impl ChannelRx for AmChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let p = params(&settings)?;
        self.demod = demodulator(&p)?;
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.demod.process(iq, &mut out.audio_pcm);
        self.rolloff.process(&mut out.audio_pcm);
        self.leveller.process(&mut out.audio_pcm);
        clamp_full_scale(&mut out.audio_pcm);
        if !out.audio_pcm.is_empty() {
            out.audio_rate = AUDIO_RATE;
        }
    }
}

const MODULATION_DEPTH: f32 = 0.8;

pub struct AmTx {
    queue: TxQueue<f32>,
    audio_lp: RealDecimator,
    filtered: Vec<f32>,
    burst: Burst,
}

impl NativeTx for AmTx {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let p = params(&settings)?;
        Ok(Self {
            queue: TxQueue::new(DESCRIPTOR.type_id.as_str(), f64::from(AUDIO_RATE)),
            audio_lp: audio_lowpass(&p)?,
            filtered: Vec::new(),
            burst: Burst::new(ctx.input_rate),
        })
    }
}

impl ChannelTx for AmTx {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.audio_lp = audio_lowpass(&params(&settings)?)?;
        Ok(())
    }

    fn submit(&mut self, payload: TxPayload) -> Result<(), ChannelError> {
        let TxPayload::Audio(pcm) = payload else {
            return Err(ChannelError::InvalidPayload(
                "am carries audio, not frames".to_owned(),
            ));
        };
        self.queue.accept(pcm.len())?;
        self.audio_lp.process(&pcm, &mut self.filtered);
        clamp_full_scale(&mut self.filtered);
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

const AIRBAND: &[HzRange] = &[HzRange::mhz(118.0, 137.0)];

fn am_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "AM voice",
            &[Modulation::Am],
            HzRange::new(2_000.0, 12_000.0),
            "amplitude modulation over a carrier",
        ),
        SignalSignature::new(
            "Airband voice (AM)",
            &[Modulation::Am],
            HzRange::new(4_000.0, 10_000.0),
            "amplitude modulation over a carrier in the aeronautical band",
        )
        .on(AIRBAND),
    ]
}

/// Opens the filter a quarter past the heard width.
fn am_tune(signal: &sdrmm_wire::IdentSignal, p: &mut AmParams) -> f64 {
    p.bandwidth_hz = (signal.bandwidth_hz * 1.25).clamp(4000.0, 30_000.0);
    0.0
}

pub(crate) fn am_decoder() -> Native<AmParams> {
    Native::new::<AmChannel>(|p: &AmParams| (-p.bandwidth_hz / 2.0, p.bandwidth_hz / 2.0))
        .filter(channel_filter)
        .tx::<AmTx>()
        .signatures(am_signatures)
        .tune(am_tune)
        .form(am_form)
        .kinds(am_kinds)
}

pub(crate) fn am_form() -> Vec<SettingField> {
    vec![
        SettingField::bandwidth(&[5_000.0, 8_000.0, 10_000.0]),
        SettingField::new("sync", "Sync", Control::Toggle)
            .title("Lock to the carrier; clearer under fading"),
    ]
}

fn am_kinds() -> Vec<KindSpec> {
    vec![]
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use sdrmm_wire::ChannelParams;

    use super::*;
    use crate::{nfm::NfmParams, wfm::WfmParams};
    use crate::{
        synth::{burst, tone_audio},
        testutil::{am_iq, dominant_tone, rms, run_ragged, settings, tone_power},
    };

    const RATE: f64 = 48_000.0;

    fn channel(p: AmParams) -> AmChannel {
        AmChannel::new(
            ChannelCtx { input_rate: RATE },
            settings(ChannelParams::of(&p).expect("am settings")),
        )
        .unwrap()
    }

    #[test]
    fn demodulates_1_khz_tone_over_ragged_blocks() {
        let mut chan = channel(AmParams {
            bandwidth_hz: 10_000.0,
            ..AmParams::default()
        });
        let audio = run_ragged(&mut chan, &am_iq(RATE, 1_000.0, 0.5, 48_000));
        let window = &audio[4_000..16_000];
        let (freq, ratio) = dominant_tone(window, RATE);
        assert!((995.0..1_005.0).contains(&freq), "dominant {freq} Hz");
        assert!(ratio > 10.0, "tone-to-rest ratio {ratio}");
        let amplitude = rms(window);
        assert!((0.2..0.3).contains(&amplitude), "rms {amplitude}");
    }

    #[test]
    fn apply_reconfigures_bandwidth() {
        let mut chan = channel(AmParams {
            bandwidth_hz: 10_000.0,
            ..AmParams::default()
        });
        chan.apply(settings(
            ChannelParams::of(&AmParams {
                bandwidth_hz: 6_000.0,
                ..AmParams::default()
            })
            .expect("am settings"),
        ))
        .unwrap();
        let audio = run_ragged(&mut chan, &am_iq(RATE, 1_000.0, 0.5, 48_000));
        let window = &audio[4_000..16_000];
        let (freq, ratio) = dominant_tone(window, RATE);
        assert!((995.0..1_005.0).contains(&freq), "dominant {freq} Hz");
        assert!(ratio > 10.0, "tone-to-rest ratio {ratio}");
        let amplitude = rms(window);
        assert!((0.2..0.3).contains(&amplitude), "rms {amplitude}");
    }

    fn faded_carrier_iq(offset_hz: f64, len: usize) -> Vec<Complex<f32>> {
        am_iq(RATE, 1_000.0, 0.8, len)
            .into_iter()
            .enumerate()
            .map(|(k, s)| {
                let carrier_loss = Complex::new(-0.7, 0.0);
                let shift = Complex::from_polar(1.0, (TAU * offset_hz * k as f64 / RATE) as f32);
                (s + carrier_loss) * shift
            })
            .collect()
    }

    fn harmonic_ratio(audio: &[f32]) -> f64 {
        tone_power(audio, 2_000.0, RATE) / tone_power(audio, 1_000.0, RATE)
    }

    #[test]
    fn sync_keeps_a_faded_carrier_undistorted_where_the_envelope_fails() {
        let iq = faded_carrier_iq(150.0, 96_000);
        let demod = |sync| {
            let audio = run_ragged(
                &mut channel(AmParams {
                    sync,
                    ..AmParams::default()
                }),
                &iq,
            );
            harmonic_ratio(&audio[48_000..])
        };
        let envelope = demod(false);
        let sync = demod(true);
        assert!(envelope > 0.05, "envelope distortion {envelope}");
        assert!(sync < 0.01, "sync distortion {sync}");
    }

    #[test]
    fn mismatched_params_variant_is_rejected() {
        let mut chan = channel(AmParams::default());
        let err = chan.apply(settings(
            ChannelParams::new("wfm", &WfmParams::default()).expect("wfm settings"),
        ));
        assert!(matches!(err, Err(ChannelError::InvalidSettings(_))));
    }

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

    fn transmitter() -> AmTx {
        AmTx::new(ctx(), tx_params()).unwrap()
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
            Err(ChannelError::InvalidPayload(_))
        ));
        let over = (crate::tx::MAX_QUEUE_S * f64::from(AUDIO_RATE)) as usize + 1;
        assert!(matches!(
            tx.submit(TxPayload::Audio(vec![0.0; over])),
            Err(ChannelError::InvalidPayload(_))
        ));
        let mut block = [Complex::new(0.0, 0.0); 16];
        assert_eq!(tx.generate(&mut block), 0);
    }

    #[test]
    fn tx_rejects_mismatched_params_and_input_rate() {
        let built = AmTx::new(
            ctx(),
            settings(ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings")),
        );
        assert!(matches!(built, Err(ChannelError::InvalidSettings(_))));
        let built = AmTx::new(
            ChannelCtx {
                input_rate: 240_000.0,
            },
            tx_params(),
        );
        assert!(matches!(built, Err(ChannelError::InvalidSettings(_))));
    }
}
