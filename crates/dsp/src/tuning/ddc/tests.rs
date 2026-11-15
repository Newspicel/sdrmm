use super::{
    plan::{StageFilter, integer_decimation},
    *,
};
use crate::{
    filter::fir::in_ragged_blocks,
    math::complex::widen,
    math::db::Decibels,
    testutil::{complex_tone, rms_c},
};

const FS_IN: f64 = 2_048_000.0;
const FS_OUT: f64 = 48_000.0;
const BLOCK: usize = 16_384;

fn tone_at_rate(freq_hz: f64, rate: f64, len: usize) -> Vec<Complex<f32>> {
    complex_tone(freq_hz / rate, len)
}

/// The level of the component at `freq_hz` in `samples` taken at `rate`, in dB.
pub(crate) fn tone_level_db(samples: &[Complex<f32>], freq_hz: f64, rate: f64) -> f64 {
    let sum: Complex<f64> = samples
        .iter()
        .enumerate()
        .map(|(n, &s)| {
            let phase = -std::f64::consts::TAU * freq_hz * n as f64 / rate;
            widen(s) * Complex::from_polar(1.0, phase)
        })
        .sum();
    (sum.norm() / samples.len() as f64).amplitude_to_db()
}

#[test]
fn the_start_up_transient_ends_within_the_reported_settling() {
    for (input_rate, output_rate) in [
        (2_400_000.0, 48_000.0),
        (1_024_000.0, 37_000.0),
        (48_000.0, 48_000.0),
    ] {
        let mut ddc = Ddc::new(input_rate, output_rate, 0.0).unwrap();
        let mut out = Vec::new();
        ddc.process(&vec![Complex::new(1.0, 0.0); 200_000], &mut out);
        let settled = &out[ddc.settling()..];
        let worst = settled
            .iter()
            .map(|s| (s.re - 1.0).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst < 1e-3,
            "{input_rate} to {output_rate}: {worst} after {}",
            ddc.settling()
        );
    }
}

#[test]
fn neighbours_outside_the_channel_stay_a_hundred_db_down() {
    for (input_rate, output_rate) in [(2_400_000.0, 48_000.0), (10_000_000.0, 48_000.0)] {
        for offset in [0.6, 0.75, 1.0, 2.0, 7.0, 21.0, 49.0] {
            for sign in [1.0, -1.0] {
                let tone = tone_at_rate(sign * offset * output_rate, input_rate, 120_000);
                let mut ddc = Ddc::new(input_rate, output_rate, 0.0).unwrap();
                let mut out = Vec::new();
                ddc.process(&tone, &mut out);
                let leak = rms_c(&out[out.len() / 2..]).amplitude_to_db();
                assert!(
                    leak < -96.0,
                    "{input_rate} to {output_rate}: {} Hz leaks at {leak} dB",
                    sign * offset * output_rate
                );
            }
        }
    }
}

fn tone(freq_hz: f64, len: usize) -> Vec<Complex<f32>> {
    tone_at_rate(freq_hz, FS_IN, len)
}

fn run(ddc: &mut Ddc, input: &[Complex<f32>]) -> Vec<Complex<f32>> {
    in_ragged_blocks(input, &[BLOCK], |chunk, out| ddc.process(chunk, out))
}

fn mean_freq_hz(out: &[Complex<f32>], rate: f64) -> f64 {
    let mut sum = 0.0f64;
    for pair in out.windows(2) {
        sum += f64::from((pair[1] * pair[0].conj()).arg());
    }
    sum / (out.len() - 1) as f64 * rate / std::f64::consts::TAU
}

#[test]
fn reset_does_not_splice_old_filter_history_into_a_fresh_signal() {
    for (input_rate, output_rate) in [(240_000.0, 48_000.0), (240_000.0, 44_100.0)] {
        let mut used = Ddc::new(input_rate, output_rate, 1234.0).expect("rates");
        let mut fresh = used.clone();
        let mut actual = Vec::new();
        used.process(&vec![Complex::new(1.0, 0.5); 4001], &mut actual);
        used.reset();
        let signal = tone_at_rate(5678.0, input_rate, 4096);
        used.process(&signal, &mut actual);
        let mut expected = Vec::new();
        fresh.process(&signal, &mut expected);
        assert_eq!(actual, expected);
    }
}

#[test]
fn ragged_and_empty_blocks_preserve_output_across_all_resampling_paths() {
    for (input_rate, output_rate) in [
        (20_000_000.0, 48_000.0),
        (20_000_000.0, 240_000.0),
        (240_000.0, 48_000.0),
        (240_000.0, 44_100.0),
        (48_000.0, 48_000.0),
        (48_000.0, 96_000.0),
    ] {
        let signal = tone_at_rate(731.0, input_rate, 16_389);
        let mut whole = Ddc::new(input_rate, output_rate, 123.0).expect("rates");
        let mut ragged = whole.clone();
        let mut expected = Vec::new();
        whole.process(&signal, &mut expected);
        let mut actual = in_ragged_blocks(&signal, &[1, 17, 2048, 3, 4099, 0], |chunk, out| {
            ragged.process(chunk, out);
        });
        assert_eq!(actual, expected, "{input_rate} -> {output_rate}");
        ragged.reset();
        ragged.process(&signal, &mut actual);
        assert_eq!(actual, expected, "reset {input_rate} -> {output_rate}");
    }
}

#[test]
fn rejects_rates_that_are_not_positive_and_finite() {
    for (input, output) in [
        (f64::NAN, 48_000.0),
        (48_000.0, f64::INFINITY),
        (0.0, 48_000.0),
        (48_000.0, -1.0),
    ] {
        assert!(
            matches!(
                Ddc::new(input, output, 0.0),
                Err(DdcError::InvalidRates { .. })
            ),
            "{input}→{output}"
        );
    }
}

#[test]
fn upsampling_leaves_no_image_of_a_tone_near_the_input_edge() {
    let (fs_in, fs_out) = (12_000.0f64, 48_000.0f64);
    let tone_hz = 0.4 * fs_in;
    let mut ddc = Ddc::new(fs_in, fs_out, 0.0).unwrap();
    let collected = run(&mut ddc, &tone_at_rate(tone_hz, fs_in, 24_000));
    let settled = &collected[4_096..];
    let level = |hz: f64| tone_level_db(settled, hz, fs_out);
    let tone = level(tone_hz);
    for image in [tone_hz - fs_in, tone_hz + fs_in] {
        let below = tone - level(image);
        assert!(below > 60.0, "image at {image} Hz only {below:.1} dB down");
    }
}

#[test]
fn upsampling_keeps_the_tone_and_delivers_the_output_rate() {
    for (fs_in, fs_out) in [
        (2_000_000.0f64, 2_400_000.0f64),
        (2_048_000.0, 2_400_000.0),
        (2_400_000.0, 16_000_000.0),
    ] {
        let offset = 0.1 * fs_in;
        let mut ddc = Ddc::new(fs_in, fs_out, offset).unwrap();
        let total_in = (fs_in / 8.0) as usize;
        let collected = run(
            &mut ddc,
            &tone_at_rate(offset + 0.05 * fs_in, fs_in, total_in),
        );
        let ideal = (total_in as f64 * fs_out / fs_in) as i64;
        assert!(
            (collected.len() as i64 - ideal).abs() <= 2,
            "{fs_in}→{fs_out}: got {} S, ideal {ideal}",
            collected.len()
        );
        let settled = &collected[1024..];
        let rms = rms_c(settled);
        assert!((0.97..1.03).contains(&rms), "{fs_in}→{fs_out}: rms {rms}");
        let freq = mean_freq_hz(settled, fs_out);
        let want = 0.05 * fs_in;
        assert!(
            (freq - want).abs() < 0.001 * want,
            "{fs_in}→{fs_out}: tone at {freq} Hz, wanted {want} Hz"
        );
    }
}

#[test]
fn tone_at_offset_lands_at_dc() {
    let offset = 400_000.0;
    let mut ddc = Ddc::new(FS_IN, FS_OUT, offset).unwrap();
    let collected = run(&mut ddc, &tone(offset, 262_144));
    let settled = &collected[512..];
    for (i, y) in settled.iter().enumerate() {
        let mag = y.norm();
        assert!((0.97..1.03).contains(&mag), "sample {i}: |y| = {mag}");
    }
    let freq = mean_freq_hz(settled, FS_OUT);
    assert!(freq.abs() < 2.0, "residual frequency {freq} Hz");
}

#[test]
fn tone_1_2x_output_rate_away_suppressed_over_50_db() {
    let offset = 400_000.0;
    let mut ddc = Ddc::new(FS_IN, FS_OUT, offset).unwrap();
    let collected = run(&mut ddc, &tone(offset + 1.2 * FS_OUT, 262_144));
    let rms = rms_c(&collected[512..]);
    assert!(rms < 3.16e-3, "leak rms {rms}");
}

#[test]
fn quotient_below_two_still_suppresses_folding_blockers_over_50_db() {
    for (fs_in, fs_out, blocker_hz) in [
        (460_000.0, 240_000.0, 145_000.0),
        (76_800.0, 48_000.0, 29_000.0),
    ] {
        let mut ddc = Ddc::new(fs_in, fs_out, 0.0).unwrap();
        let collected = run(&mut ddc, &tone_at_rate(blocker_hz, fs_in, 262_144));
        let rms = rms_c(&collected[512..]);
        assert!(rms < 3.16e-3, "{fs_in}→{fs_out}: blocker leak rms {rms}");

        let mut ddc = Ddc::new(fs_in, fs_out, 0.0).unwrap();
        let inband = run(&mut ddc, &tone_at_rate(0.35 * fs_out, fs_in, 262_144));
        let rms = rms_c(&inband[512..]);
        assert!(
            (0.97..1.03).contains(&rms),
            "{fs_in}→{fs_out}: in-band rms {rms}"
        );
    }
}

#[test]
fn exact_long_run_output_rate() {
    for (fs_in, fs_out) in [
        (2_048_000.0f64, 48_000.0f64),
        (2_400_000.0, 240_000.0),
        (20_000_000.0, 240_000.0),
        (19_920_000.0, 240_000.0),
    ] {
        let mut ddc = Ddc::new(fs_in, fs_out, 0.0).unwrap();
        let total_in = fs_in as usize;
        let input = vec![Complex::new(1.0f32, 0.0); total_in];
        let mut out = Vec::new();
        let mut count = 0i64;
        for chunk in input.chunks(BLOCK) {
            ddc.process(chunk, &mut out);
            count += out.len() as i64;
        }
        let ideal = fs_out as i64;
        assert!(
            (count - ideal).abs() <= 2,
            "{fs_in}→{fs_out}: got {count} S/s, ideal {ideal}"
        );
    }
}

#[test]
fn a_stage_by_two_is_a_halfband_only_where_it_is_cheaper() {
    let early = stage_filter(2_048_000.0, 2, 48_000.0);
    assert!(matches!(early, StageFilter::Halfband { .. }), "{early:?}");
    let last = stage_filter(96_000.0, 2, 48_000.0);
    assert!(matches!(last, StageFilter::Lowpass { .. }), "{last:?}");
    assert!(matches!(
        stage_filter(2_048_000.0, 3, 48_000.0),
        StageFilter::Lowpass { .. }
    ));
}

#[test]
fn a_band_converter_keeps_its_band_clean_with_a_shorter_last_stage() {
    for (input_rate, band_rate) in [(1_920_000.0, 240_000.0), (19_200_000.0, 48_000.0)] {
        let output_rate = 2.0 * band_rate;
        let whole = Ddc::new(input_rate, band_rate, 0.0).expect("rates");
        let banded = Ddc::for_band(input_rate, output_rate, band_rate, 0.0).expect("rates");
        let settle_s = |ddc: &Ddc, rate: f64| ddc.settling() as f64 / rate;
        assert!(settle_s(&banded, output_rate) < settle_s(&whole, band_rate));
        for relative in [
            0.0, 0.25, -0.4, 0.4, 0.6, -0.8, 1.2, 1.9, 2.2, -2.6, 4.1, 7.0,
        ] {
            let mut ddc = Ddc::for_band(input_rate, output_rate, band_rate, 0.0).expect("rates");
            let input = tone_at_rate(relative * band_rate, input_rate, 400_000);
            let output = run(&mut ddc, &input);
            let settled = &output[ddc.settling()..];
            let db = f64::from(rms_c(settled)).amplitude_to_db();
            let folded = (relative * band_rate / output_rate + 0.5).rem_euclid(1.0) - 0.5;
            let at = folded * output_rate / band_rate;
            if relative.abs() <= 0.4 {
                assert!(db.abs() < 1e-3, "{input_rate} {relative}: passband {db} dB");
            } else if at.abs() < 0.5 {
                assert!(
                    db < -99.5,
                    "{input_rate} {relative}: alias at {at} is {db} dB"
                );
            }
        }
    }
}

#[test]
fn a_band_converter_refuses_a_band_wider_than_its_output() {
    assert!(Ddc::for_band(1_920_000.0, 240_000.0, 480_000.0, 0.0).is_err());
    assert!(Ddc::for_band(1_920_000.0, 240_000.0, f64::NAN, 0.0).is_err());
}

#[test]
fn every_plan_keeps_the_passband_flat_and_the_neighbours_a_hundred_db_down() {
    for (input_rate, output_rate) in [
        (2_048_000.0, 64_000.0),
        (20_480_000.0, 40_000.0),
        (61_440_000.0, 48_000.0),
        (8_000_000.0, 12_500.0),
        (2_000_000.0, 44_100.0),
    ] {
        for relative in [0.0, 0.25, -0.4, 0.4, 0.6, -0.6, 0.75, 1.5, 7.0] {
            let mut ddc = Ddc::new(input_rate, output_rate, 0.0).expect("rates");
            let input = tone_at_rate(relative * output_rate, input_rate, 200_000);
            let output = run(&mut ddc, &input);
            let db = f64::from(rms_c(&output[ddc.settling()..])).amplitude_to_db();
            if relative.abs() <= 0.4 {
                assert!(db.abs() < 1e-3, "{input_rate} {relative}: passband {db} dB");
            } else {
                assert!(db < -99.5, "{input_rate} {relative}: neighbour {db} dB");
            }
        }
    }
}

#[test]
fn prime_ratios_keep_passband_and_reject_aliases() {
    for input_rate in [19_920_000.0, 20_000_000.0, 8_000_000.0, 3_200_000.0] {
        let output_rate = 240_000.0;
        let offset = 100_000.0;
        let intermediate = input_rate / integer_decimation(input_rate / output_rate) as f64;
        for relative in [
            0.0,
            0.35 * output_rate,
            -0.35 * output_rate,
            0.4 * output_rate,
            -0.4 * output_rate,
            0.6 * output_rate,
            -0.6 * output_rate,
            intermediate - 0.35 * output_rate,
            intermediate + 0.35 * output_rate,
            input_rate / 2.0 - offset - 1000.0,
        ] {
            let mut ddc = Ddc::new(input_rate, output_rate, offset).expect("rates");
            let input = tone_at_rate(offset + relative, input_rate, 262_144);
            let output = run(&mut ddc, &input);
            let settled = &output[512..];
            let rms = rms_c(settled);
            if relative.abs() <= 0.4 * output_rate {
                assert!(
                    (0.97..1.03).contains(&rms),
                    "{input_rate} {relative}: passband {rms}"
                );
                assert!((mean_freq_hz(settled, output_rate) - relative).abs() < 3.0);
            } else {
                assert!(rms < 3.16e-3, "{input_rate} {relative}: alias {rms}");
            }
        }
    }
}

#[test]
fn set_offset_retunes_within_one_block() {
    let (f1, f2) = (300_000.0, -250_000.0);
    let mut ddc = Ddc::new(FS_IN, FS_OUT, f1).unwrap();
    let mut out = Vec::new();

    let phase1 = tone(f1, 20 * BLOCK);
    let mut settled = Vec::new();
    for (i, chunk) in phase1.chunks(BLOCK).enumerate() {
        ddc.process(chunk, &mut out);
        if i >= 1 {
            settled.extend_from_slice(&out);
        }
    }
    for y in &settled {
        assert!(
            (0.9..1.1).contains(&y.norm()),
            "pre-retune |y| = {}",
            y.norm()
        );
    }

    ddc.set_offset(f2);
    let phase2 = tone(f2, 20 * BLOCK);
    settled.clear();
    for (i, chunk) in phase2.chunks(BLOCK).enumerate() {
        ddc.process(chunk, &mut out);
        if i >= 1 {
            settled.extend_from_slice(&out);
        }
    }
    for y in &settled {
        assert!(
            (0.9..1.1).contains(&y.norm()),
            "post-retune |y| = {}",
            y.norm()
        );
    }
    let freq = mean_freq_hz(&settled, FS_OUT);
    assert!(freq.abs() < 2.0, "post-retune residual frequency {freq} Hz");
}
