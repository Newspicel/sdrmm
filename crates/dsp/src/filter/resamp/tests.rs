use super::*;
use crate::{
    filter::fir::in_ragged_blocks,
    math::complex::widen,
    testutil::{complex_tone, rms_c, tone_peak_and_snr},
};

#[test]
fn ragged_resampling_matches_double_precision_convolution() {
    let input: Vec<_> = (0..16_381)
        .map(|index| {
            Complex::new(
                ((index * 37) % 251) as f32 / 251.0 - 0.5,
                ((index * 71) % 257) as f32 / 257.0 - 0.5,
            )
        })
        .collect();
    for ratio in [
        0.2,
        0.75,
        0.768,
        0.96,
        48_000.0 / 44_100.0,
        1.2,
        0.8736,
        std::f64::consts::GOLDEN_RATIO - 1.0,
        std::f64::consts::GOLDEN_RATIO,
    ] {
        let mut resampler = FracResampler::new(ratio);
        let taps = resampler.taps_per_phase;
        let mut history = vec![Complex::new(0.0, 0.0); taps - 1];
        history.extend_from_slice(&input);
        let mut expected = Vec::new();
        for (sample, row, fraction) in schedule(&resampler.clock, taps - 1, history.len()) {
            let mut sum = Complex::new(0.0f64, 0.0);
            for (index, value) in history[sample + 1 - taps..=sample].iter().enumerate() {
                let a = f64::from(resampler.rows[row * taps + index]);
                let b = f64::from(resampler.rows[(row + 1) * taps + index]);
                sum += widen(*value) * (a + (b - a) * fraction);
            }
            expected.push(sum);
        }
        let mut actual = Vec::new();
        let mut block = Vec::new();
        for input in input.chunks(997) {
            resampler.process(input, &mut block);
            actual.extend_from_slice(&block);
        }
        assert_eq!(actual.len(), expected.len(), "ratio={ratio}");
        for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            let actual = widen(*actual);
            assert!(
                (actual - expected).norm() < 1e-6,
                "ratio={ratio} sample={index}"
            );
        }
    }
}

#[expect(clippy::maybe_infinite_iter, reason = "take_while ends each schedule")]
fn schedule(clock: &Clock, first: usize, len: usize) -> Vec<(usize, usize, f64)> {
    match *clock {
        Clock::Exact {
            phases,
            whole,
            rest,
            ..
        } => (0..)
            .map(|index: usize| index * (whole * phases + rest))
            .map(|ticks| (first + ticks / phases, ticks % phases, 0.0))
            .take_while(|&(sample, ..)| sample < len)
            .collect(),
        Clock::Free { step, .. } => (0..)
            .map(|index| first as f64 + f64::from(index) * step)
            .take_while(|&time| (time as usize) < len)
            .map(|time| {
                let phase = time.fract() * FREE_PHASES as f64;
                (time as usize, phase as usize, phase.fract())
            })
            .collect(),
    }
}

#[test]
fn small_rational_ratios_run_on_an_exact_clock() {
    for (ratio, phases) in [(48_000.0 / 44_100.0, 160), (0.2, 1), (0.75, 3), (1.2, 6)] {
        let clock = FracResampler::new(ratio).clock;
        assert!(
            matches!(clock, Clock::Exact { phases: p, .. } if p == phases),
            "{ratio}: {clock:?}"
        );
    }
    for ratio in [0.8736, 0.618_033_988_7] {
        let clock = FracResampler::new(ratio).clock;
        assert!(matches!(clock, Clock::Free { .. }), "{ratio}: {clock:?}");
    }
}

#[test]
fn out_of_band_tone_at_5x_downsample_suppressed_over_50_db() {
    let mut r = FracResampler::new(48_000.0 / 240_000.0);
    let input = complex_tone(30_000.0 / 240_000.0, 5 * 4096);
    let mut out = Vec::new();
    r.process(&input, &mut out);
    let rms = rms_c(&out[512..]);
    assert!(rms < 3.16e-3, "alias leak rms {rms}");
}

#[test]
fn downsample_5x_keeps_frequency_and_snr() {
    let mut r = FracResampler::new(48_000.0 / 240_000.0);
    let input = complex_tone(100.0 / (4096.0 * 5.0), 5 * 4096 + 1024);
    let mut out = Vec::new();
    r.process(&input, &mut out);
    let (peak, snr) = tone_peak_and_snr(&out[64..64 + 4096]);
    assert_eq!(peak, 100, "output frequency shifted");
    assert!(snr > 40.0, "snr {snr} dB");
}

#[test]
fn awkward_ratio_44100_to_48000_keeps_frequency_and_snr() {
    let mut r = FracResampler::new(48_000.0 / 44_100.0);
    let input = complex_tone(1500.0 / 44_100.0, 4400);
    let mut out = Vec::new();
    r.process(&input, &mut out);
    let (peak, snr) = tone_peak_and_snr(&out[64..64 + 4096]);
    assert_eq!(peak, 128, "output frequency shifted");
    assert!(snr > 40.0, "snr {snr} dB");
}

#[test]
fn long_run_output_count_matches_ratio() {
    for (ratio, total_in, ideal_out, block) in [
        (48_000.0 / 240_000.0, 1_200_000usize, 240_000i64, 7_777usize),
        (48_000.0 / 44_100.0, 441_000, 480_000, 9_999),
    ] {
        let mut r = FracResampler::new(ratio);
        let input = complex_tone(0.01, total_in);
        let mut out = Vec::new();
        let mut count = 0i64;
        for chunk in input.chunks(block) {
            r.process(chunk, &mut out);
            count += out.len() as i64;
        }
        assert!(
            (count - ideal_out).abs() <= 2,
            "ratio {ratio}: got {count}, ideal {ideal_out}"
        );
    }
}

#[test]
fn a_kept_band_stays_clean_with_fewer_taps() {
    let dab = 2_048_000.0 / 2_400_000.0;
    let kept = FracResampler::keeping(dab, 1_536_000.0 / 2_048_000.0);
    assert!(kept.taps_per_phase * 2 < FracResampler::new(dab).taps_per_phase);
    let (ratio, keep) = (0.6, 0.75);
    let mut r = FracResampler::keeping(ratio, keep);
    let inside = complex_tone(1_228.0 / 4_096.0 * ratio, 8 * 4096);
    let mut out = Vec::new();
    r.process(&inside, &mut out);
    let (peak, snr) = tone_peak_and_snr(&out[256..256 + 4096]);
    assert_eq!(peak, 1_228, "output frequency shifted");
    assert!(snr > 40.0, "kept band snr {snr} dB");
    let mut r = FracResampler::keeping(ratio, keep);
    let folding = complex_tone(0.45, 8 * 4096);
    r.process(&folding, &mut out);
    let rms = rms_c(&out[256..]);
    assert!(
        rms < 3.16e-3,
        "a tone folding into the kept band leaks at rms {rms}"
    );
}

#[test]
fn keeping_the_whole_band_is_the_plain_resampler() {
    let ratio = 0.853;
    assert_eq!(
        FracResampler::keeping(ratio, 0.95).taps_per_phase,
        FracResampler::new(ratio).taps_per_phase
    );
    for (ratio, keep) in [(1.2, 0.5), (0.8, 1.5), (0.8, -0.1), (0.8, f64::NAN)] {
        assert_eq!(
            FracResampler::keeping(ratio, keep).taps_per_phase,
            FracResampler::new(ratio).taps_per_phase,
            "ratio {ratio} keep {keep}"
        );
    }
}

#[test]
fn ragged_blocks_match_one_shot() {
    let input = complex_tone(0.021, 30_000);
    let mut whole = FracResampler::new(48_000.0 / 44_100.0);
    let mut expected = Vec::new();
    whole.process(&input, &mut expected);

    let mut ragged = FracResampler::new(48_000.0 / 44_100.0);
    let got = in_ragged_blocks(&input, &[1, 7, 64, 3, 129, 1024, 17], |chunk, out| {
        ragged.process(chunk, out);
    });
    assert_eq!(expected.len(), got.len());
    for (i, (a, b)) in expected.iter().zip(&got).enumerate() {
        assert_eq!(a, b, "sample {i}");
    }
}

#[test]
fn reset_leaves_the_state_a_fresh_resampler_would_have() {
    let ratio = 48_000.0 / 8_000.0;
    let input = complex_tone(0.037, 4_000);

    let mut fresh = FracResampler::new(ratio);
    let mut expected = Vec::new();
    fresh.process(&input, &mut expected);

    let mut reused = FracResampler::new(ratio);
    let mut scratch = Vec::new();
    reused.process(&complex_tone(0.11, 2_500), &mut scratch);
    reused.reset();
    let mut got = Vec::new();
    reused.process(&input, &mut got);

    assert_eq!(expected.len(), got.len(), "reset shifted the output phase");
    for (i, (a, b)) in expected.iter().zip(&got).enumerate() {
        assert_eq!(a, b, "sample {i}: fresh {a}, reset {b}");
    }
}

/// Error power, against output power, of a tone carried through a trimmed resampler whose
/// ratio steps from `from` to `to` halfway, after the one complex gain that best fits.
fn trimmed_tone_error_db(cycles: f64, from: f64, to: f64) -> f64 {
    let tone = |n: f64| Complex::from_polar(1.0, std::f64::consts::TAU * cycles * n);
    let input: Vec<Complex<f32>> = (0..40_000)
        .map(|n| {
            let z = tone(f64::from(n));
            Complex::new(z.re as f32, z.im as f32)
        })
        .collect();
    let mut resampler = FracResampler::trimmable(0.9);
    resampler.set_ratio(from).unwrap();
    let mut first = Vec::new();
    resampler.process(&input[..20_000], &mut first);
    resampler.set_ratio(to).unwrap();
    let mut second = Vec::new();
    resampler.process(&input[20_000..], &mut second);
    let (step_from, step_to) = (from.recip(), to.recip());
    let at = |m: usize| {
        let early = m.min(first.len()) as f64;
        let late = m.saturating_sub(first.len()) as f64;
        tone(step_from * early + step_to * late)
    };
    let output: Vec<_> = first.iter().chain(&second).copied().collect();
    let settled = 200..output.len() - 200;
    let gain = settled
        .clone()
        .map(|m| Complex::new(f64::from(output[m].re), f64::from(output[m].im)) * at(m).conj())
        .sum::<Complex<f64>>()
        / settled.len() as f64;
    let (mut error, mut power) = (0.0, 0.0);
    for m in settled {
        let y = Complex::new(f64::from(output[m].re), f64::from(output[m].im));
        error += (y - gain * at(m)).norm_sqr();
        power += y.norm_sqr();
    }
    10.0 * (error / power).log10()
}

#[test]
fn a_trimmed_ratio_follows_its_clock_cleanly_to_the_kept_band_edge() {
    for cycles in [0.05, -0.3, 0.44] {
        let error_db = trimmed_tone_error_db(cycles, 1.0 / (1.0 + 40e-6), 1.0 / (1.0 - 25e-6));
        assert!(
            error_db < -50.0,
            "{cycles} cycles a sample: {error_db:.1} dB"
        );
    }
}

#[test]
fn only_a_trimmable_resampler_takes_a_new_ratio() {
    assert_eq!(
        FracResampler::new(0.5).set_ratio(0.6),
        Err(crate::FixedRatio)
    );
    let mut trimmed = FracResampler::trimmable(0.9);
    assert_eq!(trimmed.set_ratio(0.0), Err(crate::FixedRatio));
    assert_eq!(trimmed.set_ratio(1.0 + 1e-5), Ok(()));
}
