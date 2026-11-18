use sdrmm_wire::{
    AudioAgcMode, AudioFilterSettings, ClickRemovalSettings, DenoiseSettings,
    NotchSettings as Notch,
};

use super::*;
use sdrmm_modem_test_support::audio::tone_amplitude;

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
        let mut chain =
            AudioChain::new(1, &settings, profile, Arc::new(FixtureNets)).expect("chain builds");
        run(&mut chain, &input)[8_000..]
            .iter()
            .fold(0.0f32, |a, s| a.max(s.abs()))
    };
    assert!(peak_with(ClickProfile::Discriminator) > 1.0);
    assert!(peak_with(ClickProfile::Detector) < 0.3);
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

struct FixtureNets;

impl DenoiseNets for FixtureNets {
    fn net(&self, model: DenoiseModel) -> Result<Arc<Net>, NeuralDenoiseError> {
        match model {
            DenoiseModel::Dpdfnet2 => fixture_net(),
            other @ DenoiseModel::Dpdfnet8 => Err(NeuralDenoiseError(format!(
                "{} is not downloaded",
                other.name()
            ))),
        }
    }
}

fn fixture_net() -> Result<Arc<Net>, NeuralDenoiseError> {
    let bytes =
        sdrmm_test_support::denoise_model(DenoiseModel::Dpdfnet2).map_err(NeuralDenoiseError)?;
    Ok(Arc::new(Net::load(&bytes)?))
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len() as f64).sqrt() as f32
}

/// The neural denoiser on the real model, which only a host that downloads it can run.
mod denoiser {
    use super::*;

    const RATE: f64 = AUDIO_RATE as f64;

    fn noise(len: usize, amplitude: f32, seed: u64) -> Vec<f32> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                amplitude * ((state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    fn vowel(len: usize) -> Vec<f32> {
        (0..len)
            .map(|n| {
                let t = n as f64 / RATE;
                let syllable = (std::f64::consts::PI * t * 3.0).sin().abs();
                let pitch = 120.0 + 20.0 * (std::f64::consts::TAU * 0.7 * t).sin();
                let voice: f64 = (1..=25)
                    .map(|k| {
                        let f = pitch * f64::from(k);
                        let formant = (-((f - 700.0) / 300.0).powi(2)).exp()
                            + 0.6 * (-((f - 1_200.0) / 400.0).powi(2)).exp()
                            + 0.3 * (-((f - 2_500.0) / 500.0).powi(2)).exp();
                        formant * (std::f64::consts::TAU * f * t).sin()
                    })
                    .sum();
                (0.3 * syllable * voice) as f32
            })
            .collect()
    }

    fn run(denoiser: &mut NeuralDenoiser, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len());
        for block in input.chunks(997) {
            let mut block = block.to_vec();
            denoiser.process(&mut block).expect("inference runs");
            out.extend_from_slice(&block);
        }
        out
    }

    #[test]
    fn it_returns_one_sample_for_every_sample_it_is_given() {
        let mut denoiser =
            NeuralDenoiser::new(fixture_net().expect("model loads"), AUDIO_RATE, 1.0)
                .expect("denoiser builds");
        for len in [1usize, 159, 480, 997, 4_800] {
            let mut block = noise(len, 0.1, 7);
            denoiser.process(&mut block).expect("inference runs");
            assert_eq!(block.len(), len);
        }
    }

    #[test]
    fn it_quietens_noise_with_nobody_talking() {
        let input = noise(96_000, 0.1, 11);
        let output = run(
            &mut NeuralDenoiser::new(fixture_net().expect("model loads"), AUDIO_RATE, 1.0)
                .expect("denoiser builds"),
            &input,
        );
        let before = rms(&input[48_000..]);
        let after = rms(&output[48_000..]);
        assert!(
            after < before * 0.1,
            "noise only fell from {before} to {after}"
        );
    }

    #[test]
    fn it_keeps_a_voice_while_it_takes_the_noise() {
        let voice = vowel(144_000);
        let hiss = noise(voice.len(), 0.05, 3);
        let noisy: Vec<f32> = voice.iter().zip(&hiss).map(|(v, n)| v + n).collect();
        let output = run(
            &mut NeuralDenoiser::new(fixture_net().expect("model loads"), AUDIO_RATE, 1.0)
                .expect("denoiser builds"),
            &noisy,
        );
        let kept = rms(&output[48_000..]);
        let voiced = rms(&voice[48_000..]);
        assert!(kept > voiced * 0.3, "voice fell from {voiced} to {kept}");
    }

    #[test]
    fn zero_strength_hands_back_the_input_delayed_by_its_latency() {
        let input = vowel(48_000);
        let mut denoiser =
            NeuralDenoiser::new(fixture_net().expect("model loads"), AUDIO_RATE, 0.0)
                .expect("denoiser builds");
        let output = run(&mut denoiser, &input);
        let settled = 24_000;
        let residual = |lag: usize| {
            let err: f32 = output[settled..]
                .iter()
                .zip(&input[settled - lag..])
                .map(|(a, b)| (a - b).powi(2))
                .sum();
            (err / (output.len() - settled) as f32).sqrt()
        };
        let best = (0..6_000)
            .min_by(|&a, &b| residual(a).total_cmp(&residual(b)))
            .expect("lags to try");
        assert_eq!(best, denoiser.latency());
        assert!(
            residual(best) < 0.05 * rms(&input),
            "residual {}",
            residual(best)
        );
    }
}
