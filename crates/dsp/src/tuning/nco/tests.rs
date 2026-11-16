use super::*;
use crate::{math::db::Decibels, testutil::complex_noise};

fn reference(turns: f64) -> Complex<f32> {
    let (sin, cos) = (std::f64::consts::TAU * turns.rem_euclid(1.0)).sin_cos();
    Complex::new(cos as f32, sin as f32)
}

#[test]
fn long_runs_match_double_precision_phase_without_amplitude_drift() {
    for frequency in [
        0.0,
        0.001,
        -0.001,
        187_501.25,
        -731_251.0,
        9_999_999.0,
        -10_000_000.0,
    ] {
        let rate = 20_000_000.0;
        let mut nco = Nco::new(frequency, rate);
        let turns = f64::from(frequency) / f64::from(rate);
        let mut worst = 0.0f32;
        for index in 0..2_000_000 {
            let actual = nco.next_sample();
            let expected = reference(f64::from(index) * turns);
            worst = worst.max((actual - expected).norm());
        }
        assert!(worst < 4e-7, "frequency {frequency}: error {worst}");
    }
}

#[test]
fn lookup_interpolation_stays_below_minus_125_dbc() {
    let len = 65_536;
    for bin in [1, 341, 16_383, 32_767, 65_533] {
        let mut nco = Nco::new(bin as f32, len as f32);
        let mut samples: Vec<_> = (0..len).map(|_| nco.next_sample()).collect();
        crate::math::fft::FftPair::new(len).forward(&mut samples);
        let carrier = samples[bin].norm();
        let spur = samples
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != bin)
            .map(|(_, sample)| sample.norm())
            .fold(0.0f32, f32::max);
        let dbc = (spur / carrier).amplitude_to_db();
        assert!(dbc < -125.0, "bin {bin}: spur {dbc} dBc");
    }
}

#[test]
fn retuning_preserves_phase_and_reset_restarts_at_unity() {
    let rate = 48_000.0;
    let mut nco = Nco::new(137.0, rate);
    let mut turns = 0.0;
    for frequency in [137.0, -913.0, 0.0, 24_000.0, 1.0] {
        nco.set_freq(frequency, rate);
        for _ in 0..4_099 {
            assert!((nco.next_sample() - reference(turns)).norm() < 4e-7);
            turns += f64::from(frequency) / f64::from(rate);
        }
    }
    nco.reset();
    assert_eq!(nco.next_sample(), Complex::new(1.0, 0.0));
}

#[test]
fn ragged_mixing_matches_one_shot() {
    let mut nco = Nco::new(731.0, 48_000.0);
    let mut samples = vec![Complex::new(1.0, 0.5); 4_099];
    let mut reference = nco.clone();
    let mut expected = samples.clone();
    reference.mix(&mut expected);
    for chunk in samples.chunks_mut(17) {
        nco.mix(chunk);
    }
    assert_eq!(samples, expected);
}

fn noise(len: usize) -> Vec<Complex<f32>> {
    complex_noise(len, 1.0, 0x9E37_79B9_7F4A_7C15)
}

fn started(frequency: f32, lead: usize) -> Nco {
    let mut nco = Nco::new(frequency, 20_000_000.0);
    for _ in 0..lead {
        let _ = nco.next_sample();
    }
    nco
}

#[test]
fn block_mixing_is_bit_identical_to_mixing_sample_by_sample() {
    let input = noise(37);
    for frequency in [0.0, 187_501.25, -731_251.0, 9_999_999.0, -10_000_000.0] {
        for len in 0..=37 {
            let source = &input[..len];
            let mut single = started(frequency, len * 7);
            let expected: Vec<_> = source
                .iter()
                .map(|&x| {
                    let mut sample = [x];
                    single.mix(&mut sample);
                    sample[0]
                })
                .collect();
            let mut copied = started(frequency, len * 7);
            let mut out = vec![Complex::new(0.0, 0.0); len];
            copied.mix_into(source, &mut out);
            let mut in_place = started(frequency, len * 7);
            let mut samples = source.to_vec();
            in_place.mix(&mut samples);
            assert_eq!(out, expected, "{frequency} Hz, {len} samples");
            assert_eq!(samples, expected, "{frequency} Hz, {len} samples in place");
            assert_eq!(copied.phase, single.phase);
            assert_eq!(in_place.phase, single.phase);
        }
    }
}

#[test]
fn skipping_samples_matches_mixing_them() {
    let input = noise(700);
    for skipped in [0, 1, 5, 127, 128, 300] {
        let mut mixed = started(-731_251.0, 3);
        let mut expected = input.clone();
        mixed.mix(&mut expected);
        let mut skipping = started(-731_251.0, 3);
        skipping.skip(skipped);
        let mut got = input[skipped..].to_vec();
        skipping.mix(&mut got);
        assert_eq!(got, expected[skipped..], "after skipping {skipped}");
        assert_eq!(skipping.phase, mixed.phase);
    }
}

#[test]
fn unit_phasors_wrap_whole_turns_in_either_direction() {
    for turns in [
        0.0,
        0.25,
        -0.25,
        0.999_999,
        -0.999_999,
        3.125,
        -7.875,
        1e6 + 0.5,
    ] {
        let got = unit_phasor(turns);
        let want = reference(turns);
        assert!((got - want).norm() < 1e-6, "{turns} turns: {got} vs {want}");
    }
}

#[test]
fn real_mixing_matches_mixing_the_same_samples_as_complex() {
    let input: Vec<f32> = noise(301).iter().map(|sample| sample.re).collect();
    for frequency in [0.0, 187_501.25, -731_251.0, -10_000_000.0] {
        for lead in [0, 1, 3, 130] {
            let mut complex = started(frequency, lead);
            let mut expected: Vec<_> = input.iter().map(|&x| Complex::new(x, 0.0)).collect();
            complex.mix(&mut expected);
            let mut real = started(frequency, lead);
            let mut got = vec![Complex::new(0.0, 0.0); input.len()];
            for (source, target) in input.chunks(37).zip(got.chunks_mut(37)) {
                real.mix_real_into(source, target);
            }
            assert_eq!(got, expected, "{frequency} Hz after {lead}");
            assert_eq!(real.phase, complex.phase);
        }
    }
}

#[test]
fn block_boundaries_do_not_change_the_mix() {
    let input = noise(37 * 38 / 2 + 5);
    let mut whole = started(-731_251.0, 3);
    let mut expected = input.clone();
    whole.mix(&mut expected);
    let mut ragged = started(-731_251.0, 3);
    let mut out = vec![Complex::new(0.0, 0.0); input.len()];
    let mut start = 0;
    for len in (0..=37).chain([5]) {
        let end = start + len;
        ragged.mix_into(&input[start..end], &mut out[start..end]);
        start = end;
    }
    assert_eq!(start, input.len());
    assert_eq!(out, expected);
    assert_eq!(ragged.phase, whole.phase);
}

#[test]
fn an_invalid_frequency_mixes_to_nan_without_moving_the_phase() {
    let mut nco = started(1_000.0, 5);
    let phase = nco.phase;
    nco.set_freq(f32::NAN, 48_000.0);
    let mut samples = noise(11);
    nco.mix(&mut samples);
    let mut out = vec![Complex::new(0.0, 0.0); 11];
    nco.mix_into(&noise(11), &mut out);
    assert!(
        samples
            .iter()
            .chain(&out)
            .all(|s| s.re.is_nan() && s.im.is_nan())
    );
    assert_eq!(nco.phase, phase);
}

#[test]
fn mixing_a_tone_to_baseband_yields_dc() {
    let fs = 48_000.0;
    let f = 6_000.0;
    let mut src = Nco::new(f, fs);
    let tone: Vec<Complex<f32>> = (0..1024).map(|_| src.next_sample()).collect();

    let mut mixer = Nco::new(-f, fs);
    let mut out = vec![Complex::new(0.0, 0.0); tone.len()];
    mixer.mix_into(&tone, &mut out);

    let mean: Complex<f32> = out.iter().sum::<Complex<f32>>() / out.len() as f32;
    assert!(
        (mean.norm() - 1.0).abs() < 1e-3,
        "mean norm {}",
        mean.norm()
    );
}

#[test]
fn mixing_in_place_matches_mixing_into_a_buffer() {
    let fs = 48_000.0;
    let mut src = Nco::new(3_000.0, fs);
    let tone: Vec<Complex<f32>> = (0..4_096).map(|_| src.next_sample()).collect();

    let mut copied = vec![Complex::new(0.0, 0.0); tone.len()];
    Nco::new(-1_000.0, fs).mix_into(&tone, &mut copied);
    let mut in_place = tone.clone();
    Nco::new(-1_000.0, fs).mix(&mut in_place);

    let worst = copied
        .iter()
        .zip(&in_place)
        .map(|(a, b)| (a - b).norm())
        .fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "in-place mix diverged: {worst}");
}

#[test]
fn out_of_range_frequency_aliases_and_stays_bounded() {
    let fs = 48_000.0;
    let mut aliased = Nco::new(1.5 * fs, fs);
    let mut reference = Nco::new(-0.5 * fs, fs);
    let mut max_err = 0.0f32;
    for _ in 0..100_000 {
        let a = aliased.next_sample();
        let r = reference.next_sample();
        max_err = max_err.max((a - r).norm());
    }
    assert!(
        max_err < 1e-2,
        "aliased phasor diverged from reference: {max_err}"
    );
}

#[test]
fn a_double_precision_tuning_starts_at_the_requested_phase() {
    let turns = -0.004_123_456_789_012_3;
    let start = 1_234.567_8;
    let mut nco = Nco::new(0.25, 1.0);
    assert!((nco.next_sample() - Complex::new(1.0, 0.0)).norm() < 1e-6);
    nco.set_turns(turns);
    nco.set_phase(start * turns);
    let mut worst = 0.0f32;
    for index in 0..10_000 {
        let expected = reference((start + f64::from(index)) * turns);
        worst = worst.max((nco.next_sample() - expected).norm());
    }
    assert!(worst < 4e-7, "error {worst}");
}
