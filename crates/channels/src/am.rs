use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{ClickProfile, Control, KindSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod encoder;
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub(crate) use encoder::am_encoder;
pub use encoder::{AmEncoderSettings, AmModulator};
pub use model::*;
use std::{f64::consts::TAU, sync::LazyLock};

use num_complex::Complex;
use sdrmm_dsp::{Agc, Deemphasis};
use sdrmm_modem::analog::{AmDemod, AmDetector, AmMode, AmParams as AmWaveform, AmRx};
use sdrmm_wire::{CallGate, ChannelDescriptor, ChannelSettings, DecoderFamily};

use sdrmm_decoder::{
    AUDIO_RATE, Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
    voice_leveller,
};
use sdrmm_dsp::audio::clip::clamp_full_scale;

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
    sdrmm_decoder::settings_of::<AmParams>(settings)
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

pub(crate) fn channel_filter(p: &AmParams) -> Result<FilterSpec, ChannelError> {
    check_bandwidth(p)?;
    Ok(FilterSpec::lowpass(CHANNEL_TAPS, p.bandwidth_hz / 2.0))
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

impl Channel for AmChannel {
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
    let decoder =
        Native::new::<AmChannel>(|p: &AmParams| (-p.bandwidth_hz / 2.0, p.bandwidth_hz / 2.0))
            .filter(channel_filter)
            .signatures(am_signatures)
            .tune(am_tune)
            .form(am_form)
            .kinds(am_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

const BANDWIDTHS_HZ: [f64; 3] = [5_000.0, 8_000.0, 10_000.0];

pub(crate) fn am_form() -> Vec<SettingField> {
    vec![
        SettingField::bandwidth(&BANDWIDTHS_HZ),
        SettingField::new("sync", "Sync", Control::Toggle)
            .title("Lock to the carrier; clearer under fading"),
    ]
}

fn am_kinds() -> Vec<KindSpec> {
    vec![]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use sdrmm_wire::ChannelParams;

    use super::*;
    use sdrmm_decoder_test_support::{am_iq, dominant_tone, rms, run_ragged, settings, tone_power};

    const RATE: f64 = 48_000.0;

    fn channel(p: &AmParams) -> AmChannel {
        AmChannel::new(
            ChannelCtx { input_rate: RATE },
            settings(ChannelParams::of(p).expect("am settings")),
        )
        .unwrap()
    }

    #[test]
    fn demodulates_1_khz_tone_over_ragged_blocks() {
        let mut chan = channel(&AmParams {
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
        let mut chan = channel(&AmParams {
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
                &mut channel(&AmParams {
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
        let mut chan = channel(&AmParams::default());
        let err = chan.apply(settings(ChannelParams::bare("wfm")));
        assert!(matches!(err, Err(ChannelError::InvalidSettings(_))));
    }
}
