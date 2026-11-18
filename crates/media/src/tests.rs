use std::{thread, time::Duration};

use sdrmm_codec::aac::{AudioConfig as DrmAudio, Coding as DrmCoding};

use super::*;

fn decode(codec: &Codec, data: &[u8], chunk: usize) -> Vec<(Option<i64>, Payload)> {
    let mut decoder = decoder::Decoder::new(codec).expect("decoder");
    let mut out = Vec::new();
    for block in data.chunks(chunk) {
        decoder.push(block, None, &mut out).expect("decode");
    }
    decoder.finish(&mut out).expect("flush");
    out
}

#[test]
fn dvb_aac_and_dolby_produce_stereo_pcm_across_pes_boundaries() {
    for (kind, data) in [
        (
            Codec::Aac,
            include_bytes!("../../../fixtures/broadcast_audio/tone.aac").as_slice(),
        ),
        (
            Codec::Ac3,
            include_bytes!("../../../fixtures/broadcast_audio/tone.ac3").as_slice(),
        ),
        (
            Codec::Eac3,
            include_bytes!("../../../fixtures/broadcast_audio/tone.eac3").as_slice(),
        ),
    ] {
        let reference = decode(&kind, data, data.len());
        let fragmented = decode(&kind, data, 73);
        assert_eq!(reference.len(), fragmented.len());
        let mut energy = [0.0_f64; 2];
        let mut samples = 0;
        for ((_, a), (_, b)) in reference.iter().zip(&fragmented) {
            let (Payload::Audio(a), Payload::Audio(b)) = (a, b) else {
                panic!("audio");
            };
            assert_eq!(a, b);
            for pair in a.as_chunks::<2>().0 {
                energy[0] += f64::from(pair[0]).powi(2);
                energy[1] += f64::from(pair[1]).powi(2);
                samples += 1;
            }
        }
        assert!((18000..24000).contains(&samples), "{kind:?}: {samples}");
        assert!(
            energy[0] > 100.0 && energy[1] > 20.0,
            "{kind:?}: {energy:?}"
        );
        assert!(energy[0] > energy[1] * 3.0 && energy[0] < energy[1] * 5.0);
    }
}

#[test]
fn dvb_mp2_decodes_in_pure_rust_across_pes_boundaries() {
    let data = include_bytes!("../../../fixtures/broadcast_audio/tone_48k_mono.mp2");
    let pcm = |chunk: usize| -> Vec<Vec<f32>> {
        decode(&Codec::Mp2, data, chunk)
            .into_iter()
            .map(|(_, payload)| match payload {
                Payload::Audio(pcm) => pcm,
                _ => panic!("audio"),
            })
            .collect()
    };
    let reference = pcm(data.len());
    assert_eq!(reference, pcm(73));
    assert_eq!(reference.len(), 20);
    assert!(reference.iter().all(|frame| frame.len() == 2 * 1152));
}

#[test]
fn mp2_frames_carry_their_pes_time_onwards() {
    let data = include_bytes!("../../../fixtures/broadcast_audio/tone_48k_mono.mp2");
    let mut decoder = decoder::Decoder::new(&Codec::Mp2).expect("decoder");
    let mut out = Vec::new();
    decoder
        .push(&data[..300], Some(9_000), &mut out)
        .expect("decode");
    decoder
        .push(&data[300..576], Some(50_000), &mut out)
        .expect("decode");
    let times: Vec<_> = out.iter().map(|(pts, _)| *pts).collect();
    assert_eq!(times, [Some(9_000), Some(11_160), Some(50_000)]);
}

#[test]
fn mpeg2_h264_and_hevc_decode_reordered_color_pictures_across_pes_boundaries() {
    for (kind, data) in [
        (
            Codec::Mpeg2,
            include_bytes!("../../../fixtures/broadcast_audio/pattern.m2v").as_slice(),
        ),
        (
            Codec::H264,
            include_bytes!("../../../fixtures/broadcast_audio/pattern.h264").as_slice(),
        ),
        (
            Codec::H265,
            include_bytes!("../../../fixtures/broadcast_audio/pattern.hevc").as_slice(),
        ),
    ] {
        let reference = decode(&kind, data, data.len());
        let fragmented = decode(&kind, data, 173);
        assert_eq!(
            reference.len(),
            if kind == Codec::Mpeg2 { 12 } else { 10 },
            "{kind:?}"
        );
        assert_eq!(reference.len(), fragmented.len());
        for ((_, a), (_, b)) in reference.iter().zip(&fragmented) {
            let (Payload::Video(a), Payload::Video(b)) = (a, b) else {
                panic!("video");
            };
            assert_eq!(a, b);
            assert_eq!((a.width, a.height), (160, 96));
            assert_eq!(a.rgb.len(), 160 * 96 * 3);
            assert!(a.rgb.iter().any(|&v| v > 200));
            assert!(a.rgb.iter().any(|&v| v < 20));
        }
    }
}

#[test]
fn resetting_a_fragmented_parser_recovers_at_the_next_complete_stream() {
    let bytes = include_bytes!("../../../fixtures/broadcast_audio/tone.aac");
    let mut decoder = decoder::Decoder::new(&Codec::Aac).expect("decoder");
    let mut output = Vec::new();
    decoder
        .push(&bytes[..17], Some(0), &mut output)
        .expect("partial header");
    decoder.recover().expect("reset parser");
    decoder
        .push(bytes, Some(90000), &mut output)
        .expect("complete stream");
    decoder.finish(&mut output).expect("flush");
    let reference = decode(&Codec::Aac, bytes, bytes.len());
    assert_eq!(output.len(), reference.len());
    for ((_, actual), (_, expected)) in output.iter().zip(&reference) {
        let (Payload::Audio(actual), Payload::Audio(expected)) = (actual, expected) else {
            panic!("audio")
        };
        assert_eq!(actual, expected);
    }
}

fn drm_config(sbr: bool, rate_hz: u32) -> DrmAudio {
    DrmAudio {
        coding: DrmCoding::Aac,
        sbr,
        mode: sdrmm_codec::aac::AudioMode::Mono,
        rate_hz,
        rate_code: 0,
        text: false,
        surround: 0,
        config: [0; sdrmm_codec::aac::MAX_CONFIG],
        config_length: 0,
    }
}

fn drm_pcm(frames: &[u8], config: &DrmAudio) -> Vec<f32> {
    let codec = Codec::fdk(
        &sdrmm_codec::aac::aac_specific_config(config).expect("config"),
        None,
    );
    let raw: Vec<Vec<u8>> = length_prefixed(frames)
        .into_iter()
        .map(|frame| sdrmm_codec::aac::raw_frame(frame, config).expect("frame"))
        .collect();
    decode_audio(&codec, raw.iter().map(Vec::as_slice))
        .expect("decode")
        .concat()
}

#[test]
fn drm_aac_frames_match_the_reference_decoder() {
    let pcm = drm_pcm(
        include_bytes!("../../../fixtures/drm/drm_lc_mono_48k.drm"),
        &drm_config(false, 48_000),
    );
    let reference: Vec<f32> = include_bytes!("../../../fixtures/drm/drm_lc_mono_48k.pcm")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect();
    assert!(pcm.len().abs_diff(reference.len()) < 100);
    let best = (-128_i32..=128)
        .map(|lag| {
            let mut error = 0.0;
            let mut power = 0.0;
            for i in 12_000..pcm.len().min(reference.len()) - 4096 {
                let actual = pcm[(i as i32 + 2 * lag) as usize];
                let target = reference[i] * std::f32::consts::FRAC_1_SQRT_2;
                error += f64::from(actual - target).powi(2);
                power += f64::from(target).powi(2);
            }
            error / power
        })
        .fold(f64::MAX, f64::min);
    assert!(best < 0.001, "relative error {best}");
}

#[test]
fn drm_he_aac_frames_carry_their_tone() {
    let pcm = drm_pcm(
        include_bytes!("../../../fixtures/drm/drm_he_mono_24k.drm"),
        &drm_config(true, 12_000),
    );
    assert!(
        pcm.len().abs_diff(30 * 1920 * 2 * 2) < 4096,
        "{}",
        pcm.len()
    );
    let left: Vec<f32> = pcm.iter().step_by(2).copied().collect();
    let (frequency, ratio) =
        sdrmm_modem_test_support::audio::dominant_tone(&left[24_000..], f64::from(AUDIO_RATE));
    assert!((frequency - 700.0).abs() < 5.0, "{frequency} Hz");
    assert!(ratio > 10.0, "{ratio}");
}

#[test]
fn xhe_aac_with_four_to_one_sbr_and_pvc_matches_the_reference_decoder() {
    let codec = Codec::fdk(
        include_bytes!("../../../fixtures/drm/xhe_4to1_pvc_38k.asc"),
        Some("xHE-AAC"),
    );
    let mut decoder = decoder::Decoder::new(&codec).expect("decoder");
    let mut out = Vec::new();
    for unit in length_prefixed(include_bytes!("../../../fixtures/drm/xhe_4to1_pvc_38k.aus")) {
        decoder.push(unit, None, &mut out).expect("decode");
    }
    decoder.finish(&mut out).expect("flush");
    let left: Vec<f32> = out
        .into_iter()
        .flat_map(|(_, payload)| match payload {
            Payload::Audio(pcm) => pcm,
            _ => panic!("audio"),
        })
        .step_by(2)
        .collect();
    let reference: Vec<f32> = include_bytes!("../../../fixtures/drm/xhe_4to1_pvc_38k_48k.pcm")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect();
    let priming = 10_496 * AUDIO_RATE as usize / 38_400;
    let best = (priming - 64..=priming + 64)
        .map(|lag| {
            let mut error = 0.0;
            let mut power = 0.0;
            for (actual, target) in left[lag..].iter().zip(&reference[..reference.len() - 256]) {
                let target = target * std::f32::consts::FRAC_1_SQRT_2;
                error += f64::from(actual - target).powi(2);
                power += f64::from(target).powi(2);
            }
            error / power
        })
        .fold(f64::MAX, f64::min);
    assert!(best < 0.0001, "relative error {best}");
}

fn played(media: &mut Media, done: impl Fn(&Media) -> bool) -> ChannelOutputs {
    let mut out = ChannelOutputs::default();
    for _ in 0..2000 {
        media.drain(&mut out);
        if done(media) {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    out
}

fn drm_units(config: &DrmAudio) -> (Codec, Vec<Vec<u8>>) {
    let codec = Codec::fdk(
        &sdrmm_codec::aac::aac_specific_config(config).expect("config"),
        None,
    );
    let units = length_prefixed(include_bytes!("../../../fixtures/drm/drm_lc_mono_48k.drm"))
        .into_iter()
        .map(|frame| sdrmm_codec::aac::raw_frame(frame, config).expect("frame"))
        .collect();
    (codec, units)
}

#[test]
fn an_audio_gap_conceals_its_length_and_keeps_the_audio_around_it() {
    let (codec, units) = drm_units(&drm_config(false, 48_000));
    let pcm = |gap: bool| {
        let mut media = Media::new().expect("worker");
        for (index, unit) in units[..12].iter().enumerate() {
            if gap && index == 6 {
                media.audio_gap(960, 1, "lost");
            }
            media.push(&codec, unit, None);
        }
        let out = played(&mut media, |media| media.audio_frames >= 12);
        assert_eq!(media.audio_frames, 12, "{:?}", media.audio_error);
        assert!(media.input_idle());
        out.audio_pcm
    };
    let plain = pcm(false);
    let gapped = pcm(true);
    assert_eq!(gapped.len(), plain.len() + 960 * 2);
    let energy = |pcm: &[f32]| {
        pcm.iter()
            .map(|s| f64::from(*s) * f64::from(*s))
            .sum::<f64>()
    };
    assert!(energy(&plain) > 0.0);
    assert!((energy(&gapped) - energy(&plain)).abs() < energy(&plain) / 1e6);
}

#[test]
fn a_named_codec_leads_its_errors_with_its_name() {
    let (_, units) = drm_units(&drm_config(false, 48_000));
    let mut media = Media::new().expect("worker");
    let codec = Codec::fdk(&[0xF8, 0x80], Some("xHE-AAC"));
    media.push(&codec, &units[0], None);
    played(&mut media, |media| media.audio_errors > 0);
    let error = media.audio_error.expect("an error");
    assert!(error.starts_with("xHE-AAC: "), "{error}");
}

#[test]
fn a_unit_too_long_for_one_chunk_is_refused_and_counted() {
    let mut media = Media::new().expect("worker");
    let codec = Codec::fdk(&[0x11, 0x88], None);
    media.push(&codec, &[0; CHUNK_BYTES + 1], None);
    assert_eq!(media.audio_errors, 1);
    assert_eq!(
        media.audio_error.as_deref(),
        Some("Broadcast media input overflow")
    );
}

#[test]
fn length_prefixed_units_stop_at_a_truncated_one() {
    assert_eq!(
        length_prefixed(&[0, 2, 7, 8, 0, 1, 9, 0, 5, 1]),
        [&[7, 8][..], &[9][..]]
    );
}
