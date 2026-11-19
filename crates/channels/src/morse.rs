use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{EventContract, KindSpec, NumberSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;
use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_modem::morse::{MorseChunk, MorseReader};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily};

use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};

const CHANNEL_TAPS: usize = 257;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "morse".to_owned(),
    name: "Morse (CW)".to_owned(),
    summary: "One Morse signal to text".to_owned(),
    family: DecoderFamily::AMATEUR,
    verification: sdrmm_wire::Verification::OwnModulator,
    bandwidth_hz: 400.0,
    input_rate_hz: 8_000.0,
    has_audio: false,
    emits: vec![MorseText::KIND],
    ..ChannelDescriptor::default()
});

#[derive(Clone)]
pub struct MorseChannel {
    reader: MorseReader,
    chunks: Vec<MorseChunk>,
}

fn params(settings: &ChannelSettings) -> Result<MorseParams, ChannelError> {
    sdrmm_decoder::settings_of::<MorseParams>(settings)
}

fn check_params(p: &MorseParams) -> Result<(), ChannelError> {
    let rate = DESCRIPTOR.input_rate_hz;
    if !(p.bandwidth_hz.is_finite() && p.bandwidth_hz > 0.0 && p.bandwidth_hz < rate / 2.0) {
        return Err(ChannelError::InvalidSettings(format!(
            "morse bandwidth must be in (0, {}) Hz, got {}",
            rate / 2.0,
            p.bandwidth_hz
        )));
    }
    if let Some(wpm) = p.wpm
        && !(wpm.is_finite() && wpm > 0.0)
    {
        return Err(ChannelError::InvalidSettings(format!(
            "morse wpm must be positive, got {wpm}"
        )));
    }
    Ok(())
}

pub(crate) fn occupied_band(p: &MorseParams) -> (f64, f64) {
    let half = p.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(p: &MorseParams) -> Result<FilterSpec, ChannelError> {
    check_params(p)?;
    let (_, half) = occupied_band(p);
    Ok(FilterSpec::lowpass(CHANNEL_TAPS, half))
}

impl NativeChannel for MorseChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let p = &params(&settings)?;
        check_params(p)?;
        Ok(Self {
            reader: MorseReader::new(ctx.input_rate, p.wpm),
            chunks: Vec::new(),
        })
    }
}

impl Channel for MorseChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let p = &params(&settings)?;
        check_params(p)?;
        self.reader.set_wpm(p.wpm);
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        for sample in iq {
            self.reader.push(sample.norm(), &mut self.chunks);
        }
        for MorseChunk { text, wpm } in self.chunks.drain(..) {
            out.events.push(DecoderEvent::of(&MorseText { text, wpm }));
        }
    }
}

fn morse_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "Morse (CW)",
            &[Modulation::Ook, Modulation::Carrier],
            HzRange::new(0.0, 2_500.0),
            "a keyed carrier at hand speed",
        )
        .symbol_rate(HzRange::new(4.0, 80.0)),
    ]
}

pub(crate) fn morse_decoder() -> Native<MorseParams> {
    let decoder = Native::new::<MorseChannel>(occupied_band)
        .filter(channel_filter)
        .signatures(morse_signatures)
        .form(morse_form)
        .kinds(morse_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS).recipes(synth::RECIPES),
        _ => decoder,
    }
}

pub(crate) fn morse_form() -> Vec<SettingField> {
    vec![
        SettingField::new("bandwidth_hz", "BW", NumberSpec::hz(50.0, 3_000.0, 50.0))
            .title("CW filter bandwidth"),
        SettingField::new(
            "wpm",
            "Speed",
            NumberSpec::new(5.0, 60.0, 1.0).unit("WPM").optional("auto"),
        )
        .title("Morse speed, empty to auto-track"),
    ]
}

fn morse_kinds() -> Vec<KindSpec> {
    vec![morse_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests {
    use sdrmm_wire::ChannelParams;

    use super::*;

    use sdrmm_decoder_test_support::settings;

    const RATE: f64 = 8_000.0;
    const CALL: &str = "CQ CQ DE DL1ABC K";
    const IDLE_FLUSH_S: f64 = 3.0;
    const UNKNOWN: &str = "*";
    const MAX_CHUNK_CHARS: usize = 64;

    fn channel(wpm: Option<f32>) -> MorseChannel {
        MorseChannel::new(
            ChannelCtx { input_rate: RATE },
            settings(
                ChannelParams::of(&MorseParams {
                    bandwidth_hz: 400.0,
                    wpm,
                })
                .expect("morse settings"),
            ),
        )
        .unwrap()
    }

    fn burst(text: &str, wpm: f32, tone_hz: f64) -> Vec<Complex<f32>> {
        let mut iq = sdrmm_modem::synth::silence((0.5 * RATE) as usize);
        iq.extend(sdrmm_modem::synth::morse::transmission(
            text, wpm, tone_hz, RATE,
        ));
        iq.extend(sdrmm_modem::synth::silence(
            ((IDLE_FLUSH_S + 0.5) * RATE) as usize,
        ));
        iq
    }

    fn decode_ragged(chan: &mut MorseChannel, iq: &[Complex<f32>]) -> (String, f32) {
        let mut out = ChannelOutputs::default();
        let (mut text, mut wpm) = (String::new(), 0.0);
        let mut pos = 0;
        for len in [997usize, 1, 4_096, 65, 2_048, 7, 1_024].iter().cycle() {
            if pos >= iq.len() {
                break;
            }
            let end = (pos + len).min(iq.len());
            out.reset();
            chan.process(&iq[pos..end], &mut out);
            for event in &out.events {
                let Some(m) = event.read::<MorseText>() else {
                    panic!("morse channel emitted {}", event.kind)
                };
                assert!(!m.text.is_empty(), "empty chunk");
                text.push_str(&m.text);
                wpm = m.wpm;
            }
            assert!(out.audio_pcm.is_empty(), "morse must not produce audio");
            pos = end;
        }
        (text, wpm)
    }

    fn decode(chan: &mut MorseChannel, iq: &[Complex<f32>]) -> String {
        decode_ragged(chan, iq).0
    }

    #[test]
    fn decodes_a_call_at_dc_and_at_an_offset() {
        for tone_hz in [0.0, 120.0] {
            let iq = burst(CALL, 20.0, tone_hz);
            let spec = channel_filter(&MorseParams::default()).expect("default params are valid");
            let mut filter =
                sdrmm_decoder_test_support::built_filter(spec, DESCRIPTOR.input_rate_hz);
            let mut filtered = Vec::new();
            filter.process(&iq, &mut filtered);
            let text = decode(&mut channel(None), &filtered);
            assert_eq!(text, CALL, "tone at {tone_hz} Hz");
        }
    }

    #[test]
    fn tracks_speed_from_the_signal() {
        for truth in [12.0, 35.0] {
            let mut chan = channel(None);
            let (text, wpm) = decode_ragged(&mut chan, &burst(CALL, truth, 0.0));
            assert_eq!(text, CALL, "at {truth} wpm");
            let error = (wpm - truth).abs() / truth;
            assert!(error < 0.2, "estimated {wpm} wpm for {truth} wpm sending");
        }
    }

    #[test]
    fn fixed_speed_tolerates_sloppy_sending() {
        for actual in [16.0, 24.0] {
            let mut chan = channel(Some(20.0));
            let (text, wpm) = decode_ragged(&mut chan, &burst(CALL, actual, 0.0));
            assert_eq!(text, CALL, "stated 20 wpm, sent {actual}");
            assert!((wpm - 20.0).abs() < 0.5, "fixed speed reported as {wpm}");
        }
    }

    #[test]
    fn continuous_sending_is_chunked_at_the_buffer_size() {
        let sent = "E".repeat(MAX_CHUNK_CHARS + 4);
        let mut chan = channel(None);
        let mut out = ChannelOutputs::default();
        chan.process(&burst(&sent, 30.0, 0.0), &mut out);
        assert!(out.events.len() > 1, "not chunked: {:?}", out.events);
        let joined: String = out
            .events
            .iter()
            .map(|e| match e.read::<MorseText>() {
                Some(m) => m.text,
                None => panic!("morse channel emitted {}", e.kind),
            })
            .collect();
        assert_eq!(joined, sent);
    }

    #[test]
    fn punctuation_and_digits_round_trip() {
        let text = "73 DE OM! QRV 14.060 =+/?,";
        assert_eq!(decode(&mut channel(None), &burst(text, 22.0, 0.0)), text);
    }

    #[test]
    fn word_gaps_become_single_spaces_and_letter_gaps_do_not() {
        assert_eq!(
            decode(&mut channel(None), &burst("EE E", 18.0, 0.0)),
            "EE E"
        );
    }

    #[test]
    fn unknown_element_runs_are_reported_not_dropped() {
        let dot = (1.2 * RATE / 20.0) as usize;
        let mut env = Vec::new();
        for i in 0..9 {
            if i > 0 {
                env.resize(env.len() + dot, 0.0);
            }
            env.resize(env.len() + dot, 1.0);
        }
        let mut iq = sdrmm_modem::synth::silence((0.5 * RATE) as usize);
        iq.extend(sdrmm_modem::synth::ook(&env, 0.0, RATE));
        iq.extend(sdrmm_modem::synth::silence(
            ((IDLE_FLUSH_S + 0.5) * RATE) as usize,
        ));
        assert_eq!(decode(&mut channel(None), &iq), UNKNOWN);
    }

    #[test]
    fn pure_noise_decodes_to_nothing() {
        let mut iq = sdrmm_modem::synth::silence((20.0 * RATE) as usize);
        sdrmm_modem::synth::add_noise(&mut iq, 0x0c0f_fee1, 0.3);
        let mut channel = channel(None);
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(1_024) {
            channel.process(block, &mut out);
        }
        assert!(out.events.is_empty(), "noise decoded to {:?}", out.events);
    }

    #[test]
    fn element_fit_separates_a_keyed_carrier_from_sliced_noise() {
        let mut keyed = channel(None);
        let mut out = ChannelOutputs::default();
        keyed.process(&burst(CALL, 20.0, 0.0), &mut out);
        assert!(
            keyed.reader.element_fit() < 0.1,
            "a keyed carrier scored {}",
            keyed.reader.element_fit()
        );

        let mut iq = sdrmm_modem::synth::silence((20.0 * RATE) as usize);
        sdrmm_modem::synth::add_noise(&mut iq, 0x0c0f_fee1, 0.3);
        let mut noisy = channel(None);
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(1_024) {
            noisy.process(block, &mut out);
        }
        assert!(
            noisy.reader.element_fit() > 0.5,
            "noise scored {}",
            noisy.reader.element_fit()
        );
    }

    #[test]
    fn ragged_block_splits_give_identical_results() {
        let iq = burst(CALL, 20.0, 0.0);
        let mut whole = ChannelOutputs::default();
        channel(None).process(&iq, &mut whole);
        let one_shot: Vec<&DecoderEvent> = whole.events.iter().collect();
        let mut chan = channel(None);
        let (ragged, _) = decode_ragged(&mut chan, &iq);
        let joined: String = one_shot
            .iter()
            .map(|e| match e.read::<MorseText>() {
                Some(m) => m.text,
                None => panic!("morse channel emitted {}", e.kind),
            })
            .collect();
        assert_eq!(joined, ragged);
        assert_eq!(ragged, CALL);
    }

    #[test]
    fn mismatched_params_variant_is_rejected() {
        let mut chan = channel(None);
        let err = chan.apply(settings(ChannelParams::bare("rtty")));
        assert!(matches!(err, Err(ChannelError::InvalidSettings(_))));
        let built = MorseChannel::new(
            ChannelCtx { input_rate: RATE },
            settings(ChannelParams::bare("rtty")),
        );
        assert!(matches!(built, Err(ChannelError::InvalidSettings(_))));
    }

    #[test]
    fn out_of_range_params_are_rejected() {
        for p in [
            MorseParams {
                bandwidth_hz: 0.0,
                wpm: None,
            },
            MorseParams {
                bandwidth_hz: f64::NAN,
                wpm: None,
            },
            MorseParams {
                bandwidth_hz: 400.0,
                wpm: Some(0.0),
            },
        ] {
            assert!(
                matches!(check_params(&p), Err(ChannelError::InvalidSettings(_))),
                "{p:?} must be rejected"
            );
        }
    }

    #[test]
    fn wrong_input_rate_is_rejected() {
        let built = MorseChannel::new(
            ChannelCtx {
                input_rate: 48_000.0,
            },
            settings(ChannelParams::of(&MorseParams::default()).expect("morse settings")),
        );
        assert!(matches!(built, Err(ChannelError::InvalidSettings(_))));
    }
}
