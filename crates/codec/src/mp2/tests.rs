use std::{thread, time::Duration};

use num_complex::Complex;

use super::*;

const MONO: &[u8] = include_bytes!("../../../../fixtures/broadcast_audio/tone_48k_mono.mp2");
const STEREO: &[u8] = include_bytes!("../../../../fixtures/broadcast_audio/tone_32k_stereo.mp2");

fn wait_for(audio: &mut LayerTwoAudio, frames: u32, out: &mut Vec<f32>) {
    let limit = std::time::Instant::now() + Duration::from_secs(3);
    while audio.frames_ok + audio.frames_bad < frames {
        assert!(
            std::time::Instant::now() < limit,
            "audio worker did not return {frames} frames"
        );
        audio.drain(out);
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn mono_mpeg_layer_two_matches_the_independent_decoded_reference() {
    let mut audio = LayerTwoAudio::new().expect("audio worker");
    for block in MONO.chunks(17) {
        audio.push(block);
    }
    let mut out = Vec::new();
    wait_for(&mut audio, 20, &mut out);
    assert_eq!(audio.frames_ok, 20);
    assert_eq!(audio.frames_bad, 0);
    let expected: Vec<f32> =
        include_bytes!("../../../../fixtures/broadcast_audio/tone_48k_mono.f32")
            .as_chunks::<4>()
            .0
            .iter()
            .map(|sample| f32::from_le_bytes(*sample))
            .collect();
    assert_eq!(out.len(), expected.len() * 2);
    let mut error = 0.0;
    let mut power = 0.0;
    for (pair, expected) in out.as_chunks::<2>().0.iter().zip(expected) {
        assert_eq!(pair[0], pair[1]);
        error += f64::from((pair[0] - expected).powi(2));
        power += f64::from(expected.powi(2));
    }
    assert!(
        error / power < 1e-5,
        "relative mean square error {}",
        error / power
    );
}

#[test]
fn stereo_is_resampled_without_mixing_the_channels() {
    let mut audio = LayerTwoAudio::new().expect("audio worker");
    audio.push(STEREO);
    let mut out = Vec::new();
    wait_for(&mut audio, 10, &mut out);
    assert_eq!(audio.frames_ok, 10);
    assert_eq!(audio.frames_bad, 0);
    assert!(out.len().abs_diff(10 * 1152 * 3) <= 2);
    let tone = |channel: usize, hz: f32| -> f32 {
        out.as_chunks::<2>()
            .0
            .iter()
            .skip(2000)
            .enumerate()
            .map(|(n, pair)| {
                Complex::from_polar(
                    pair[channel],
                    std::f32::consts::TAU * hz * n as f32 / OUTPUT_RATE as f32,
                )
            })
            .sum::<Complex<f32>>()
            .norm()
    };
    assert!(tone(0, 700.0) > 100.0 * tone(0, 1300.0));
    assert!(tone(1, 1300.0) > 100.0 * tone(1, 700.0));
}

#[test]
fn damaged_headers_reacquire_and_queue_overflow_is_reported() {
    let mut audio = LayerTwoAudio::new().expect("audio worker");
    audio.push(&[0; 79]);
    audio.push(&MONO[..192]);
    let mut out = Vec::new();
    wait_for(&mut audio, 2, &mut out);
    assert_eq!(audio.frames_ok, 1);
    assert_eq!(audio.frames_bad, 1);
    assert_eq!(
        audio.error,
        Some("Lost MPEG Layer II frame synchronization")
    );
    for _ in 0..100 {
        audio.push(MONO);
    }
    assert_eq!(audio.error, Some("Broadcast audio input queue overflow"));
    assert!(audio.frames_bad > 1);
    audio.reset();
    assert_eq!(audio.frames_bad, 0);
    assert_eq!(audio.buffered(), 0);
    assert!(audio.error.is_none());
}

#[test]
fn stale_audio_cannot_cross_a_service_change() {
    let mut audio = LayerTwoAudio::new().expect("audio worker");
    audio.push(MONO);
    audio.reset();
    audio.push(&STEREO[..576]);
    let mut out = Vec::new();
    wait_for(&mut audio, 1, &mut out);
    assert_eq!(audio.frames_ok, 1);
    assert!(out.len().abs_diff(3456) <= 2);
}

#[test]
fn protected_audio_rejects_crc_damage_and_recovers() {
    let mut frame = [0; 192];
    frame[..6].copy_from_slice(&[0xff, 0xfc, 0x44, 0xc0, 0xfe, 0xb3]);
    let mut damaged = frame;
    damaged[6] ^= 1;
    let mut audio = LayerTwoAudio::new().expect("audio worker");
    audio.push(&damaged);
    audio.push(&frame);
    let mut out = Vec::new();
    wait_for(&mut audio, 2, &mut out);
    assert_eq!(audio.frames_ok, 1);
    assert_eq!(audio.frames_bad, 1);
    assert_eq!(audio.error, Some("MPEG Layer II audio CRC mismatch"));
    assert_eq!(out.len(), 2 * 2304);
    assert!(out.iter().all(|&sample| sample == 0.0));
}

#[test]
fn frame_loss_resets_audio_synthesis_history() {
    let mut audio = LayerTwoAudio::new().expect("audio worker");
    audio.push(&MONO[..384]);
    let mut out = Vec::new();
    wait_for(&mut audio, 2, &mut out);
    let first = out[..2304].to_vec();
    out.clear();
    audio.push(&[0; 17]);
    audio.push(&MONO[..192]);
    wait_for(&mut audio, 4, &mut out);
    assert_eq!(audio.frames_ok, 3);
    assert_eq!(audio.frames_bad, 1);
    assert_eq!(out, first);
}

#[test]
fn a_stream_decodes_the_same_audio_however_the_bytes_arrive() {
    let decode = |chunk: usize| {
        let mut stream = Mp2Stream::new().expect("stream");
        let mut pcm = Vec::new();
        let mut errors = Vec::new();
        for block in MONO.chunks(chunk) {
            stream.push(block, |frame| match frame {
                Ok((header, samples)) => {
                    assert_eq!(samples.len(), header.output_samples());
                    pcm.extend_from_slice(samples);
                }
                Err(error) => errors.push(error),
            });
        }
        (pcm, errors)
    };
    let (whole, errors) = decode(MONO.len());
    assert!(errors.is_empty());
    assert_eq!(whole.len(), 20 * 2304);
    assert_eq!(decode(173), (whole, Vec::new()));
}

#[test]
fn a_stream_reports_lost_sync_only_after_a_frame() {
    let mut stream = Mp2Stream::new().expect("stream");
    let mut errors = Vec::new();
    stream.push(&[0; 40], |frame| errors.extend(frame.err()));
    assert!(errors.is_empty());
    stream.push(&MONO[..192], |frame| errors.extend(frame.err()));
    stream.push(&[0; 40], |frame| errors.extend(frame.err()));
    assert_eq!(errors, ["Lost MPEG Layer II frame synchronization"]);
}

#[test]
fn layer_two_decoding_stays_ahead_of_realtime() {
    let mut decoder = Mp2Decoder::new().expect("decoder");
    let header = Header::read(MONO).expect("header");
    let mut pcm = vec![0.0; PCM_SAMPLES];
    let start = std::time::Instant::now();
    for _ in 0..100 {
        decoder
            .decode(&MONO[..header.length], header, &mut pcm)
            .expect("audio frame");
    }
    let budget = if cfg!(sanitized) { f64::INFINITY } else { 2.4 };
    assert!(start.elapsed().as_secs_f64() < budget);
}

#[test]
fn reserved_mpeg_headers_are_rejected() {
    for header in [
        [0xff, 0xfd, 0, 0],
        [0xff, 0xfd, 0x4c, 0],
        [0xff, 0xed, 0x44, 0],
        [0xff, 0xfb, 0x44, 0],
    ] {
        assert!(Header::read(&header).is_none());
    }
}
