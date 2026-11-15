use super::*;
use crate::{
    audio::noise::testing::{RATE, tone_amplitude},
    testutil::{XorShift32, real_tone, rms_r},
};

fn run_denoiser(denoiser: &mut SpectralDenoiser, input: &[f32], block: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(input.len());
    for chunk in input.chunks(block) {
        let mut buf = chunk.to_vec();
        denoiser.process(&mut buf).unwrap();
        out.extend_from_slice(&buf);
    }
    out
}

#[test]
fn denoiser_returns_one_sample_for_every_sample_it_is_given() {
    let mut denoiser = SpectralDenoiser::new(0.5);
    let input = real_tone(1_000.0 / RATE, 48_000);
    let (mut produced, mut pos) = (0, 0);
    for len in [37usize, 1_024, 511, 4_096].iter().cycle() {
        if pos >= input.len() {
            break;
        }
        let end = (pos + len).min(input.len());
        let mut buf = input[pos..end].to_vec();
        denoiser.process(&mut buf).unwrap();
        assert_eq!(buf.len(), end - pos);
        produced += buf.len();
        pos = end;
    }
    assert_eq!(produced, input.len());
}

#[test]
fn denoiser_at_zero_strength_reconstructs_the_input() {
    let mut denoiser = SpectralDenoiser::new(0.0);
    let input = real_tone(1_000.0 / RATE, 24_000);
    let output = run_denoiser(&mut denoiser, &input, 480);
    let latency = denoiser.latency();
    for n in 2_000..input.len() - latency {
        let error = (output[n + latency] - input[n]).abs();
        assert!(
            error < 1e-3,
            "sample {n}: {} vs {}",
            output[n + latency],
            input[n]
        );
    }
}

const BURST: usize = (RATE * 0.2) as usize;

fn bursts_at(amplitude: f32, len: usize) -> Vec<f32> {
    let mut rng = XorShift32(0x2C41_66B7);
    real_tone(1_000.0 / RATE, len)
        .iter()
        .enumerate()
        .map(|(n, t)| {
            let on = (n / BURST).is_multiple_of(2);
            (if on { t * amplitude } else { 0.0 }) + rng.next_f32() * 0.1
        })
        .collect()
}

fn bursts_in_hiss(len: usize) -> Vec<f32> {
    bursts_at(0.4, len)
}

#[test]
fn denoiser_quietens_the_gaps_and_keeps_the_bursts() {
    let input = bursts_in_hiss(480_000);
    let mut denoiser = SpectralDenoiser::new(1.0);
    let output = run_denoiser(&mut denoiser, &input, 960);
    let latency = denoiser.latency();

    let inside = |index: usize| gap_window(index, latency);
    let tone_before = tone_amplitude(&input[inside(4)], 1_000.0);
    let tone_after = tone_amplitude(&output[inside(4)], 1_000.0);
    assert!(
        tone_after > 0.7 * tone_before,
        "speech was eaten: {tone_before} -> {tone_after}"
    );

    let gap_before = rms_r(&input[inside(5)]);
    let gap_after = rms_r(&output[inside(5)]);
    assert!(
        gap_after < 0.4 * gap_before,
        "hiss survived: {gap_before} -> {gap_after}"
    );
}

fn gap_window(index: usize, latency: usize) -> std::ops::Range<usize> {
    let start = index * BURST + BURST / 4 + latency;
    start..start + BURST / 2
}

fn level_variation(x: &[f32], block: usize) -> f32 {
    let levels: Vec<f32> = x.chunks_exact(block).map(rms_r).collect();
    let mean = levels.iter().sum::<f32>() / levels.len() as f32;
    let spread = levels
        .iter()
        .map(|l| f64::from((l - mean) * (l - mean)))
        .sum::<f64>()
        / levels.len() as f64;
    (spread.sqrt() as f32) / mean
}

#[test]
fn denoiser_leaves_a_steady_residual_rather_than_musical_noise() {
    let mut rng = XorShift32(0x51A7_33C1);
    let input: Vec<f32> = (0..480_000).map(|_| rng.next_f32() * 0.2).collect();
    let mut denoiser = SpectralDenoiser::new(1.0);
    let output = run_denoiser(&mut denoiser, &input, 960);

    let before = level_variation(&input[240_000..], 512);
    let after = level_variation(&output[240_000..], 512);
    assert!(
        after < 3.0 * before,
        "the residual warbles: {before} -> {after}"
    );
}

#[test]
fn denoiser_strength_scales_the_suppression_it_applies() {
    let input = bursts_in_hiss(480_000);
    let residual = |strength: f32| {
        let mut denoiser = SpectralDenoiser::new(strength);
        let output = run_denoiser(&mut denoiser, &input, 960);
        rms_r(&output[gap_window(5, denoiser.latency())])
    };
    let raw = rms_r(&input[gap_window(5, 0)]);
    let (off, half, full) = (residual(0.0), residual(0.5), residual(1.0));
    assert!(
        (off / raw - 1.0).abs() < 0.05,
        "zero strength changed {raw} to {off}"
    );
    assert!(
        half < 0.6 * off,
        "half strength did nothing: {off} -> {half}"
    );
    assert!(
        full < 0.5 * half,
        "full strength did nothing: {half} -> {full}"
    );
    assert!(
        full > 0.05 * raw,
        "the gain floor was breached: {full} of {raw}"
    );
}

#[test]
fn denoiser_opens_up_again_within_a_frame_of_a_burst_starting() {
    let input = bursts_in_hiss(480_000);
    let mut denoiser = SpectralDenoiser::new(1.0);
    let output = run_denoiser(&mut denoiser, &input, 960);
    let onset = 6 * BURST + denoiser.latency();
    let span = (RATE * 0.05) as usize;
    let early = tone_amplitude(&output[onset + span..onset + 2 * span], 1_000.0);
    let settled = tone_amplitude(
        &output[onset + BURST / 2..onset + BURST / 2 + span],
        1_000.0,
    );
    assert!(
        early > 0.7 * settled,
        "the onset was swallowed: {early} of {settled}"
    );
}

#[test]
fn denoiser_keeps_a_burst_that_sits_close_to_the_noise() {
    let input = bursts_at(0.08, 480_000);
    let mut denoiser = SpectralDenoiser::new(1.0);
    let output = run_denoiser(&mut denoiser, &input, 960);
    let latency = denoiser.latency();
    let before = tone_amplitude(&input[gap_window(4, 0)], 1_000.0);
    let after = tone_amplitude(&output[gap_window(4, latency)], 1_000.0);
    assert!(
        after > 0.5 * before,
        "a weak signal was lost: {before} -> {after}"
    );
}
#[test]
fn denoiser_keeps_ahead_of_the_audio_rate() {
    let input = bursts_in_hiss(60 * RATE as usize);
    let mut denoiser = SpectralDenoiser::new(0.7);
    let mut buf = [0.0f32; 960];
    let started = std::time::Instant::now();
    for chunk in input.as_chunks::<960>().0 {
        buf.copy_from_slice(chunk);
        denoiser.process(&mut buf).unwrap();
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "a minute of audio took {:?}",
        started.elapsed()
    );
}

#[test]
fn denoiser_recovers_after_a_non_finite_sample() {
    let mut denoiser = SpectralDenoiser::new(0.8);
    let mut poisoned = vec![f32::NAN; 2_048];
    denoiser.process(&mut poisoned).unwrap();
    assert!(poisoned.iter().all(|s| s.is_finite()));
    let input = bursts_in_hiss(240_000);
    let output = run_denoiser(&mut denoiser, &input, 480);
    assert!(output.iter().all(|s| s.is_finite()), "state still poisoned");
    let amplitude = tone_amplitude(&output[gap_window(4, denoiser.latency())], 1_000.0);
    assert!(amplitude > 0.2, "audio never came back: {amplitude}");
}
