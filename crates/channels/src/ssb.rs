use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{ClickProfile, Control, KindSpec, NumberSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod encoder;
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub(crate) use encoder::ssb_encoder;
pub use encoder::{SsbEncoderSettings, SsbModulator};
pub use model::*;
use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::Agc;
use sdrmm_modem::analog::{
    Sideband as EngineSideband, SsbDemod, SsbDetector, SsbMethod, SsbParams as SsbWaveform,
};
use sdrmm_wire::{CallGate, ChannelDescriptor, ChannelSettings, DecoderFamily, Sideband};

use sdrmm_decoder::{
    AUDIO_RATE, Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
    voice_leveller,
};
use sdrmm_dsp::audio::clip::clamp_full_scale;

pub(crate) const PASSBAND_LOW_HZ: f64 = 100.0;
const FILTER_TAPS: usize = 257;
const AUDIO_TAPS: usize = FILTER_TAPS;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "ssb".to_owned(),
    name: "SSB".to_owned(),
    summary: "Single sideband voice, USB or LSB".to_owned(),
    family: DecoderFamily::ANALOG_VOICE,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: 3_000.0,
    input_rate_hz: 48_000.0,
    has_audio: true,
    calls: Some(CallGate::Squelch),
    click: ClickProfile::Detector,
    levels_audio: true,
    analog_voice: true,
    ..ChannelDescriptor::default()
});

pub struct SsbChannel {
    demod: SsbDemod,
    leveller: Agc,
}

fn params(settings: &ChannelSettings) -> Result<SsbParams, ChannelError> {
    sdrmm_decoder::settings_of::<SsbParams>(settings)
}

pub(crate) fn waveform(p: &SsbParams) -> Result<SsbWaveform, ChannelError> {
    let rate = DESCRIPTOR.input_rate_hz;
    if !(p.bandwidth_hz.is_finite()
        && p.bandwidth_hz > PASSBAND_LOW_HZ
        && p.bandwidth_hz < rate / 2.0)
    {
        return Err(ChannelError::InvalidSettings(format!(
            "ssb bandwidth must be in ({PASSBAND_LOW_HZ}, {}) Hz, got {}",
            rate / 2.0,
            p.bandwidth_hz
        )));
    }
    Ok(SsbWaveform {
        sideband: match p.sideband {
            Sideband::Usb => EngineSideband::Upper,
            Sideband::Lsb => EngineSideband::Lower,
        },
        method: SsbMethod::Hilbert,
        low_cut: PASSBAND_LOW_HZ / rate,
        bandwidth: p.bandwidth_hz / rate,
        band_taps: FILTER_TAPS,
        audio_taps: FILTER_TAPS,
    })
}

fn demodulator(p: &SsbParams) -> Result<SsbDemod, ChannelError> {
    Ok(SsbDemod::new(&waveform(p)?, SsbDetector::Filter, false))
}

impl NativeChannel for SsbChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let p = params(&settings)?;
        Ok(Self {
            demod: demodulator(&p)?,
            leveller: voice_leveller(),
        })
    }
}

impl Channel for SsbChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let p = params(&settings)?;
        self.demod = demodulator(&p)?;
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.demod.process(iq, &mut out.audio_pcm);
        self.leveller.process(&mut out.audio_pcm);
        clamp_full_scale(&mut out.audio_pcm);
        if !out.audio_pcm.is_empty() {
            out.audio_rate = AUDIO_RATE;
        }
    }
}

fn channel_filter(p: &SsbParams) -> Result<FilterSpec, ChannelError> {
    let band = waveform(p)?.band();
    let rate = DESCRIPTOR.input_rate_hz;
    Ok(FilterSpec::band(
        band.taps,
        band.low * rate,
        band.high * rate,
    ))
}

fn ssb_signatures() -> Vec<SignalSignature> {
    vec![SignalSignature::new(
        "SSB voice",
        &[Modulation::Ssb],
        HzRange::new(1_500.0, 4_000.0),
        "one sideband, no carrier",
    )]
}

/// Takes the heard sideband and width, and tunes to the suppressed carrier at the band edge.
fn ssb_tune(signal: &sdrmm_wire::IdentSignal, p: &mut SsbParams) -> f64 {
    p.sideband = signal.sideband.unwrap_or(Sideband::Usb);
    p.bandwidth_hz = signal.bandwidth_hz.clamp(1500.0, 6000.0);
    match p.sideband {
        Sideband::Usb => -p.bandwidth_hz / 2.0,
        Sideband::Lsb => p.bandwidth_hz / 2.0,
    }
}

pub(crate) fn ssb_decoder() -> Native<SsbParams> {
    let decoder = Native::new::<SsbChannel>(|p: &SsbParams| match p.sideband {
        Sideband::Usb => (PASSBAND_LOW_HZ, p.bandwidth_hz),
        Sideband::Lsb => (-p.bandwidth_hz, -PASSBAND_LOW_HZ),
    })
    .filter(channel_filter)
    .signatures(ssb_signatures)
    .tune(ssb_tune)
    .form(ssb_form)
    .kinds(ssb_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn ssb_form() -> Vec<SettingField> {
    vec![
        SettingField::new("sideband", "Side", Control::choice::<Sideband>()).title("Sideband"),
        SettingField::new("bandwidth_hz", "BW", NumberSpec::hz(200.0, 10_000.0, 100.0))
            .title("SSB bandwidth"),
    ]
}

fn ssb_kinds() -> Vec<KindSpec> {
    vec![]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests {
    use sdrmm_wire::ChannelParams;

    use super::*;
    use sdrmm_decoder::encoder::burst;
    use sdrmm_decoder::encoder::{Carries, EncoderError, Modulator, NativeModulator, TxPayload};
    use sdrmm_decoder_test_support::{
        complex_tone, component, dominant_tone, rms, run_ragged, settings, tone_power,
    };
    use sdrmm_modem::synth::tone_audio;

    const RATE: f64 = 48_000.0;

    fn two_tone(len: usize) -> Vec<f32> {
        tone_audio(700.0, 0.4, RATE, len)
            .iter()
            .zip(tone_audio(1_900.0, 0.4, RATE, len))
            .map(|(low, high)| low + high)
            .collect()
    }

    const fn ctx() -> ChannelCtx {
        ChannelCtx { input_rate: RATE }
    }

    fn voice_settings(sideband: Sideband) -> ChannelSettings {
        settings(
            ChannelParams::of(&SsbParams {
                sideband,
                bandwidth_hz: 2_700.0,
            })
            .expect("ssb settings"),
        )
    }

    fn channel(sideband: Sideband) -> SsbChannel {
        SsbChannel::new(ctx(), voice_settings(sideband)).unwrap()
    }

    fn modulator(sideband: Sideband) -> SsbModulator {
        SsbModulator::new(&SsbEncoderSettings {
            sideband,
            bandwidth_hz: 2_700.0,
        })
        .unwrap()
    }

    fn excite(sideband: Sideband, audio: Vec<f32>) -> Vec<Complex<f32>> {
        let mut tx = modulator(sideband);
        tx.submit(TxPayload::Audio(audio)).unwrap();
        burst(&mut tx)
    }

    fn settled(iq: &[Complex<f32>]) -> &[Complex<f32>] {
        &iq[2 * AUDIO_TAPS..iq.len() - 2 * AUDIO_TAPS]
    }

    #[test]
    fn usb_demodulates_positive_tone_over_ragged_blocks() {
        let mut chan = channel(Sideband::Usb);
        let audio = run_ragged(&mut chan, &complex_tone(1_000.0 / RATE, 48_000));
        let window = &audio[2_000..14_000];
        let (freq, ratio) = dominant_tone(window, RATE);
        assert!((995.0..1_005.0).contains(&freq), "dominant {freq} Hz");
        assert!(ratio > 10.0, "tone-to-rest ratio {ratio}");
        let amplitude = rms(window);
        assert!((0.2..0.3).contains(&amplitude), "rms {amplitude}");
    }

    #[test]
    fn a_weak_signal_is_levelled_like_a_strong_one() {
        let mut chan = channel(Sideband::Usb);
        let weak: Vec<Complex<f32>> = complex_tone(1_000.0 / RATE, 96_000)
            .iter()
            .map(|s| s * 0.01)
            .collect();
        let audio = run_ragged(&mut chan, &weak);
        let amplitude = rms(&audio[72_000..]);
        assert!((0.2..0.3).contains(&amplitude), "rms {amplitude}");
    }

    #[test]
    fn lsb_demodulates_negative_tone() {
        let mut chan = channel(Sideband::Lsb);
        let audio = run_ragged(&mut chan, &complex_tone(-1_000.0 / RATE, 48_000));
        let (freq, ratio) = dominant_tone(&audio[2_000..14_000], RATE);
        assert!((995.0..1_005.0).contains(&freq), "dominant {freq} Hz");
        assert!(ratio > 10.0, "tone-to-rest ratio {ratio}");
    }

    #[test]
    fn lsb_rejects_a_usb_tone_by_more_than_20_db() {
        let input = complex_tone(1_000.0 / RATE, 48_000);
        let mut usb = channel(Sideband::Usb);
        let usb_rms = rms(&run_ragged(&mut usb, &input)[2_000..14_000]);
        let mut lsb = channel(Sideband::Lsb);
        let lsb_rms = rms(&run_ragged(&mut lsb, &input)[2_000..14_000]);
        assert!(
            lsb_rms < usb_rms / 10.0,
            "lsb rms {lsb_rms} vs usb rms {usb_rms}"
        );
    }

    #[test]
    fn usb_recovers_a_two_tone_exciter_that_lsb_rejects() {
        let iq = excite(Sideband::Usb, two_tone(48_000));
        let mut usb = channel(Sideband::Usb);
        let audio = run_ragged(&mut usb, &iq);
        let window = &audio[2_000..14_000];
        let low = tone_power(window, 700.0, RATE);
        let high = tone_power(window, 1_900.0, RATE);
        assert!(low > 0.45, "700 Hz holds {low} of the audio power");
        assert!(high > 0.45, "1900 Hz holds {high} of the audio power");

        let mut lsb = channel(Sideband::Lsb);
        let rejected = rms(&run_ragged(&mut lsb, &iq)[2_000..14_000]);
        assert!(
            rejected < rms(window) / 10.0,
            "lsb rms {rejected} vs usb rms {}",
            rms(window)
        );
    }

    #[test]
    fn apply_narrower_bandwidth_keeps_demodulating() {
        let mut chan = channel(Sideband::Usb);
        chan.apply(settings(
            ChannelParams::of(&SsbParams {
                sideband: Sideband::Usb,
                bandwidth_hz: 2_000.0,
            })
            .expect("ssb settings"),
        ))
        .unwrap();
        let audio = run_ragged(&mut chan, &complex_tone(1_000.0 / RATE, 48_000));
        let (freq, ratio) = dominant_tone(&audio[2_000..14_000], RATE);
        assert!((995.0..1_005.0).contains(&freq), "dominant {freq} Hz");
        assert!(ratio > 10.0, "tone-to-rest ratio {ratio}");
    }

    #[test]
    fn mismatched_params_variant_is_rejected() {
        let mut chan = channel(Sideband::Usb);
        let err = chan.apply(settings(ChannelParams::bare("nfm")));
        assert!(matches!(err, Err(ChannelError::InvalidSettings(_))));
    }

    #[test]
    fn tx_puts_the_audio_on_one_side_of_dc_and_leaves_the_image_40_db_down() {
        for (sideband, sign) in [(Sideband::Usb, 1.0), (Sideband::Lsb, -1.0)] {
            for tone_hz in [700.0, 1_500.0, 2_200.0] {
                let iq = excite(sideband, tone_audio(tone_hz, 1.0, RATE, 48_000));
                let iq = settled(&iq);
                let wanted = component(iq, sign * tone_hz, RATE);
                let image = component(iq, -sign * tone_hz, RATE);
                assert!(
                    (wanted - 1.0).abs() < 0.03,
                    "{sideband:?} {tone_hz} Hz: wanted sideband {wanted}"
                );
                assert!(
                    image < 0.01,
                    "{sideband:?} {tone_hz} Hz: image {image} against {wanted}"
                );
            }
        }
    }

    #[test]
    fn tx_transmits_the_passband_the_receiver_keeps() {
        let level = |tone_hz: f64| {
            let iq = excite(Sideband::Usb, tone_audio(tone_hz, 1.0, RATE, 48_000));
            component(settled(&iq), tone_hz, RATE)
        };
        for tone_hz in [700.0, 2_200.0] {
            let passed = level(tone_hz);
            assert!(
                (passed - 1.0).abs() < 0.03,
                "{tone_hz} Hz passed at {passed}"
            );
        }
        for tone_hz in [PASSBAND_LOW_HZ, 2_700.0] {
            let edge = level(tone_hz);
            assert!((0.4..0.6).contains(&edge), "{tone_hz} Hz edge at {edge}");
        }
        let stopband = level(4_000.0);
        assert!(stopband < 0.01, "4 kHz leaked at {stopband}");
    }

    #[test]
    fn tx_never_leaves_full_scale() {
        let loud: Vec<f32> = two_tone(48_000).iter().map(|s| s * 2.5).collect();
        for s in excite(Sideband::Usb, loud) {
            assert!(s.norm() <= 1.0 + 1e-6, "envelope {}", s.norm());
        }
    }

    #[test]
    fn tx_sends_the_lower_sideband_below_the_carrier() {
        let mut tx = modulator(Sideband::Lsb);
        tx.submit(TxPayload::Audio(tone_audio(1_500.0, 1.0, RATE, 48_000)))
            .unwrap();
        let iq = burst(&mut tx);
        let iq = settled(&iq);
        assert!(component(iq, -1_500.0, RATE) > 0.9);
        assert!(component(iq, 1_500.0, RATE) < 0.01);
    }

    #[test]
    fn tx_radiates_nothing_until_audio_is_submitted() {
        let mut tx = modulator(Sideband::Usb);
        let mut block = [Complex::new(9.0, 9.0); 64];
        assert_eq!(tx.generate(&mut block), 0);
        assert_eq!(block[0], Complex::new(9.0, 9.0));
    }

    #[test]
    fn tx_rejects_a_frame_payload_and_a_backlog_past_the_bound() {
        let mut tx = modulator(Sideband::Usb);
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
    fn tx_refuses_a_bandwidth_below_its_passband() {
        let built = SsbModulator::new(&SsbEncoderSettings {
            sideband: Sideband::Usb,
            bandwidth_hz: 50.0,
        });
        assert!(matches!(built, Err(EncoderError::Settings { .. })));
    }
}
