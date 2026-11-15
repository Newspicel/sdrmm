use std::{f64::consts::PI, ops::RangeInclusive};

use super::{
    floor::POWER_FLOOR_DB,
    lane::{RAMP, signed_bin},
    matching::MATCH_COHERENCE,
    *,
};
use crate::{
    math::complex::narrow,
    math::window::hann_weight,
    testutil::{XorShift32, complex_tone},
};

const RATE: f64 = 2048.0;
const PLAIN: StitchOptions = StitchOptions {
    equalise: false,
    flatten: false,
    snr_blend: false,
    spur_reject: false,
    match_phase: true,
};
const EQUALISE: StitchOptions = StitchOptions {
    equalise: true,
    ..PLAIN
};
const FLATTEN: StitchOptions = StitchOptions {
    flatten: true,
    ..EQUALISE
};
const SNR_BLEND: StitchOptions = StitchOptions {
    snr_blend: true,
    ..PLAIN
};
const SPUR_REJECT: StitchOptions = StitchOptions {
    spur_reject: true,
    ..PLAIN
};

fn plain(lanes: usize) -> Stitcher {
    with_options(lanes, PLAIN)
}

fn with_options(lanes: usize, options: StitchOptions) -> Stitcher {
    let mut stitcher = Stitcher::new(lanes, RATE).expect("stitcher");
    stitcher.set_options(options);
    stitcher
}

fn tone(len: usize, hz: f64, gain: Complex<f32>) -> Vec<Complex<f32>> {
    complex_tone(hz / RATE, len)
        .into_iter()
        .map(|sample| gain * sample)
        .collect()
}

fn lanes_hearing(
    hz: f64,
    offsets: &[f64],
    len: usize,
    gains: &[Complex<f32>],
) -> Vec<Vec<Complex<f32>>> {
    offsets
        .iter()
        .zip(gains)
        .map(|(offset, gain)| tone(len, hz - offset, *gain))
        .collect()
}

fn stitch(stitcher: &mut Stitcher, lanes: &[Vec<Complex<f32>>]) -> Vec<Complex<f32>> {
    let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
    let mut out = Vec::new();
    let written = stitcher.process(&views, &mut out).expect("lanes");
    assert_eq!(written, out.len());
    out
}

fn fit(out: &[Complex<f32>], hz: f64, lanes: usize, skip: usize) -> (Complex<f32>, f32) {
    let rate = RATE * lanes as f64;
    let origin = (STITCH_FFT / 4) as f64 / RATE;
    let expected = |j: usize| {
        let t = origin + j as f64 / rate;
        Complex::from_polar(1.0f32, (TAU * hz * t) as f32)
    };
    let span = &out[skip..];
    let amplitude = span
        .iter()
        .enumerate()
        .map(|(j, y)| y * expected(j + skip).conj())
        .sum::<Complex<f32>>()
        / span.len() as f32;
    let residual = span
        .iter()
        .enumerate()
        .map(|(j, y)| (y - amplitude * expected(j + skip)).norm_sqr())
        .sum::<f32>()
        / span.len() as f32;
    (amplitude, residual)
}

fn noise(len: usize, seed: u32) -> Vec<Complex<f32>> {
    let mut rng = XorShift32(seed);
    let mut next = move || 0.5 * rng.next_f32();
    (0..len).map(|_| Complex::new(next(), next())).collect()
}

fn louder(samples: Vec<Complex<f32>>, gain: f32) -> Vec<Complex<f32>> {
    samples.into_iter().map(|sample| sample * gain).collect()
}

fn rolled_off_db(bin: i64) -> f64 {
    -3.0 * (bin as f64 / 870.0).powi(4)
}

fn multitone(offsets: &[f64], len: usize, gains: &[Complex<f32>]) -> Vec<Vec<Complex<f32>>> {
    let mut fft = FftPair::new(STITCH_FFT);
    offsets
        .iter()
        .zip(gains)
        .map(|(offset, gain)| {
            let mut period: Vec<Complex<f32>> = (0..STITCH_FFT)
                .map(|index| {
                    let bin = signed_bin(index, STITCH_FFT);
                    let absolute = bin as f64 + offset;
                    let phase = PI * absolute * absolute / STITCH_FFT as f64;
                    let tone = Complex::from_polar(rolled_off_db(bin).db_to_amplitude(), phase);
                    gain * narrow(tone)
                })
                .collect();
            fft.inverse_scaled(&mut period);
            (0..len).map(|n| period[n % STITCH_FFT]).collect()
        })
        .collect()
}

fn psd(samples: &[Complex<f32>], size: usize) -> Vec<f64> {
    let window: Vec<f32> = (0..size).map(|i| hann_weight(i, size) as f32).collect();
    let mut fft = FftPair::new(size);
    let mut power = vec![0.0f64; size];
    let mut chunk = vec![Complex::default(); size];
    let mut count = 0.0;
    for start in (0..=samples.len() - size).step_by(size / 2) {
        for ((slot, sample), weight) in chunk.iter_mut().zip(&samples[start..]).zip(&window) {
            *slot = sample * weight;
        }
        fft.forward(&mut chunk);
        for (sum, value) in power.iter_mut().zip(&chunk) {
            *sum += f64::from(value.norm_sqr());
        }
        count += 1.0;
    }
    power.iter().map(|sum| sum / count).collect()
}

fn band_db(power: &[f64], bins: RangeInclusive<i64>) -> f64 {
    let len = power.len() as i64;
    let (sum, count) = bins.fold((0.0, 0.0), |(sum, count), bin| {
        (sum + power[bin.rem_euclid(len) as usize], count + 1.0)
    });
    (sum / count).power_to_db()
}

fn share_at_centre(stitcher: &Stitcher, lane: usize) -> f32 {
    stitcher.lanes[lane]
        .taps
        .iter()
        .find(|tap| tap.out_bin == 0)
        .map_or(0.0, |tap| tap.share)
}

fn share_sums(stitcher: &Stitcher) -> Vec<f32> {
    let mut sums = vec![0.0; stitcher.out_len];
    for tap in stitcher.lanes.iter().flat_map(|lane| &lane.taps) {
        sums[tap.out_bin] += tap.share;
    }
    sums
}

#[test]
fn a_tone_in_one_lane_lands_where_it_belongs() {
    let offsets = auto_offsets(3, RATE);
    let hz = offsets[2] + 100.0;
    let mut stitcher = plain(3);
    let heard = [
        Complex::default(),
        Complex::default(),
        Complex::new(1.0, 0.0),
    ];
    let out = stitch(&mut stitcher, &lanes_hearing(hz, &offsets, 20_000, &heard));
    let (amplitude, residual) = fit(&out, hz, 3, 3 * STITCH_FFT);
    assert!(
        (amplitude - Complex::new(1.0, 0.0)).norm() < 0.02,
        "{amplitude}"
    );
    assert!(residual < 1e-3, "{residual}");
}

#[test]
fn a_manual_layout_off_the_bin_grid_still_lands_the_tone() {
    let offsets = [-700.3, 650.7];
    let hz = offsets[0] - 123.4;
    let mut stitcher = plain(2);
    stitcher.retune(&offsets).expect("layout");
    let heard = [Complex::new(1.0, 0.0), Complex::default()];
    let out = stitch(&mut stitcher, &lanes_hearing(hz, &offsets, 20_000, &heard));
    let centre = stitcher.output_center_offset_hz();
    let (amplitude, residual) = fit(&out, hz - centre, 2, 2 * STITCH_FFT);
    assert!(
        (amplitude - Complex::new(1.0, 0.0)).norm() < 0.02,
        "{amplitude}"
    );
    assert!(residual < 1e-3, "{residual}");
}

#[test]
fn the_output_centres_between_the_outermost_lanes() {
    let offsets = [0.0, 1740.0];
    let hz = 1500.0;
    let mut stitcher = plain(2);
    stitcher.retune(&offsets).expect("layout");
    assert_eq!(stitcher.output_center_offset_hz(), 870.0);
    let heard = [Complex::default(), Complex::new(1.0, 0.0)];
    let out = stitch(&mut stitcher, &lanes_hearing(hz, &offsets, 20_000, &heard));
    let (amplitude, residual) = fit(&out, hz - 870.0, 2, 2 * STITCH_FFT);
    assert!(
        (amplitude - Complex::new(1.0, 0.0)).norm() < 0.02,
        "{amplitude}"
    );
    assert!(residual < 1e-3, "{residual}");
    assert_eq!(plain(3).output_center_offset_hz(), 0.0);
}

#[test]
fn the_output_runs_at_lanes_times_the_input() {
    let mut stitcher = plain(4);
    let len = 50 * HOP;
    let out = stitch(&mut stitcher, &vec![vec![Complex::default(); len]; 4]);
    assert_eq!(out.len(), 4 * (len - HOP));
    assert_eq!(stitcher.output_rate(), 4.0 * RATE);
}

#[test]
fn a_tone_in_an_overlap_survives_a_lane_with_its_own_phase_and_gain() {
    let offsets = auto_offsets(2, RATE);
    let hz = 3.0;
    let gains = [Complex::new(1.0, 0.0), Complex::from_polar(0.6, 2.3)];
    let mut stitcher = plain(2);
    let out = stitch(&mut stitcher, &lanes_hearing(hz, &offsets, 60_000, &gains));
    let skip = out.len() / 2;
    let (amplitude, residual) = fit(&out, hz, 2, skip);
    assert!(
        (amplitude - Complex::new(1.0, 0.0)).norm() < 0.02,
        "{amplitude}"
    );
    assert!(residual < 1e-3, "{residual}");
}

#[test]
fn auto_offsets_sit_side_by_side_on_the_bin_grid() {
    for lanes in 2..=6 {
        let offsets = auto_offsets(lanes, RATE);
        let bin_hz = RATE / STITCH_FFT as f64;
        for (low, high) in offsets.iter().zip(offsets.iter().rev()) {
            assert!((low + high).abs() < 1e-9);
        }
        for offset in &offsets {
            assert!((offset / bin_hz - (offset / bin_hz).round()).abs() < 1e-9);
        }
        for pair in offsets.windows(2) {
            let step = pair[1] - pair[0];
            assert!(step > 0.0 && step < (STITCH_KEEP / 2.0 + RAMP / 2.0) * 2.0 * RATE);
            assert!(step >= (STITCH_KEEP - RAMP) * RATE);
        }
    }
}

#[test]
fn a_gap_between_lanes_stays_empty() {
    let mut stitcher = plain(2);
    assert_eq!(
        stitcher.retune(&[-1.2 * RATE, 1.2 * RATE]),
        Err(StitchError::NoOverlap(0, 1))
    );
    let out = stitch(&mut stitcher, &[noise(30_000, 1), noise(30_000, 7)]);
    let n = 4096;
    let mut chunk: Vec<Complex<f32>> = out[out.len() - n..]
        .iter()
        .enumerate()
        .map(|(i, y)| y * hann_weight(i, n) as f32)
        .collect();
    FftPair::new(n).forward(&mut chunk);
    let (mut gap, mut band) = (0.0f32, 0.0f32);
    for (index, value) in chunk.iter().enumerate() {
        let f = signed_bin(index, n) as f64 / n as f64 * 2.0;
        if f.abs() < 0.6 {
            gap += value.norm_sqr();
        } else if f.abs() > 0.8 {
            band += value.norm_sqr();
        }
    }
    assert!(gap < band * 1e-4, "gap {gap} band {band}");
}

#[test]
fn block_size_does_not_change_the_output() {
    let offsets = auto_offsets(3, RATE);
    let len = 150 * HOP;
    let lanes = [noise(len, 3), louder(noise(len, 5), 2.0), noise(len, 9)];
    for options in [PLAIN, StitchOptions::default()] {
        let mut whole = with_options(3, options);
        let expected = stitch(&mut whole, &lanes);
        let mut pieces = with_options(3, options);
        pieces.retune(&offsets).expect("layout");
        let mut out = Vec::new();
        for start in (0..len).step_by(777) {
            let end = (start + 777).min(len);
            let views: Vec<&[Complex<f32>]> = lanes.iter().map(|lane| &lane[start..end]).collect();
            pieces.process(&views, &mut out).expect("lanes");
        }
        assert_eq!(out.len(), expected.len());
        for (a, b) in out.iter().zip(&expected) {
            assert!((a - b).norm() < 1e-5, "{options:?}");
        }
    }
}

#[test]
fn bad_setups_are_refused() {
    assert_eq!(Stitcher::new(1, RATE).err(), Some(StitchError::Lanes(1)));
    assert_eq!(Stitcher::new(17, RATE).err(), Some(StitchError::Lanes(17)));
    assert!(matches!(
        Stitcher::new(2, f64::NAN).err(),
        Some(StitchError::Rate(_))
    ));
    assert_eq!(Stitcher::new(2, 0.0).err(), Some(StitchError::Rate(0.0)));
}

#[test]
fn a_lane_count_mismatch_is_an_error() {
    let mut stitcher = plain(3);
    let lane = noise(4 * HOP, 2);
    let mut out = Vec::new();
    assert_eq!(
        stitcher.process(&[&lane, &lane], &mut out),
        Err(StitchError::LaneCount {
            expected: 3,
            got: 2
        })
    );
    assert!(out.is_empty());
    assert_eq!(
        stitcher.retune(&[0.0; 4]),
        Err(StitchError::LaneCount {
            expected: 3,
            got: 4
        })
    );
}

#[test]
fn lanes_of_unequal_length_are_an_error() {
    let mut stitcher = plain(2);
    let lane = noise(4 * HOP, 2);
    let mut out = Vec::new();
    assert_eq!(
        stitcher.process(&[&lane, &lane[1..]], &mut out),
        Err(StitchError::LaneLength {
            lane: 1,
            expected: 4 * HOP,
            got: 4 * HOP - 1
        })
    );
    assert!(out.is_empty());
}

#[test]
fn an_offset_out_of_range_is_refused_and_keeps_the_layout() {
    let mut stitcher = plain(2);
    stitcher.retune(&[0.0, 1740.0]).expect("layout");
    assert!(matches!(
        stitcher.retune(&[0.0, f64::NAN]),
        Err(StitchError::Offset(offset)) if offset.is_nan()
    ));
    assert_eq!(
        stitcher.retune(&[-1e300, 1e300]),
        Err(StitchError::Offset(-1e300))
    );
    assert_eq!(
        stitcher.retune(&[0.0, f64::INFINITY]),
        Err(StitchError::Offset(f64::INFINITY))
    );
    assert_eq!(stitcher.output_center_offset_hz(), 870.0);
    assert_eq!(stitcher.retune(&[1e9, 1e9 + 1740.0]), Ok(()));
    assert_eq!(stitcher.output_center_offset_hz(), 1e9 + 870.0);
}

#[test]
fn lanes_without_overlap_are_refused() {
    let mut stitcher = plain(2);
    assert_eq!(
        stitcher.retune(&[-1.2 * RATE, 1.2 * RATE]),
        Err(StitchError::NoOverlap(0, 1))
    );
    assert_eq!(
        stitcher.retune(&[1.2 * RATE, -1.2 * RATE]),
        Err(StitchError::NoOverlap(1, 0))
    );
    assert_eq!(
        stitcher.retune(&[-907.0, 906.0]),
        Err(StitchError::NoOverlap(0, 1))
    );
    assert_eq!(stitcher.retune(&[-900.0, 900.0]), Ok(()));
    assert_eq!(stitcher.retune(&auto_offsets(2, RATE)), Ok(()));
}

#[test]
fn noise_equalisation_removes_a_6_db_step_in_a_noise_only_overlap() {
    let len = 330 * HOP;
    let lanes = [noise(len, 11), louder(noise(len, 23), 2.0)];
    let step_db = |options| {
        let mut stitcher = with_options(2, options);
        let out = stitch(&mut stitcher, &lanes);
        let power = psd(&out[200 * 2 * HOP..], 1024);
        let step = band_db(&power, 25..=150) - band_db(&power, -150..=-25);
        (step, stitcher.lane_state(0), stitcher.lane_state(1))
    };
    let (before, _, _) = step_db(PLAIN);
    let (after, quiet, loud) = step_db(EQUALISE);
    assert!(before > 5.0, "{before}");
    assert!(after.abs() < 1.0, "{after}");
    assert!(
        (loud.noise_db - quiet.noise_db - 6.02).abs() < 0.3,
        "{quiet:?} {loud:?}"
    );
    assert!((quiet.gain_db - 3.01).abs() < 0.3, "{quiet:?}");
    assert!((loud.gain_db + 3.01).abs() < 0.3, "{loud:?}");
}

#[test]
fn rolloff_flattening_fills_the_seam_dip() {
    let offsets = auto_offsets(2, RATE);
    let gains = [Complex::new(1.0, 0.0), Complex::from_polar(0.8, 1.1)];
    let lanes = multitone(&offsets, 260 * HOP, &gains);
    let dip = |options| {
        let mut stitcher = with_options(2, options);
        let out = stitch(&mut stitcher, &lanes);
        let start = 220 * 2 * HOP;
        let mut period = out[start..start + 2 * STITCH_FFT].to_vec();
        let len = period.len() as i64;
        FftPair::new(period.len()).forward(&mut period);
        let level =
            |bin: i64| f64::from(period[bin.rem_euclid(len) as usize].norm_sqr()).power_to_db();
        let mut levels: Vec<f64> = (-1600..=1600).map(level).collect();
        levels.sort_by(f64::total_cmp);
        let reference = levels[levels.len() / 2];
        let lowest = (-60..=60).map(level).fold(f64::INFINITY, f64::min);
        reference - lowest
    };
    let rolled = dip(EQUALISE);
    let flattened = dip(FLATTEN);
    assert!(rolled > 2.0, "{rolled}");
    assert!(flattened < 1.0, "{flattened}");
}

#[test]
fn a_spur_in_one_lane_does_not_reach_the_output() {
    let offsets = auto_offsets(2, RATE);
    let spur_hz = -20.0;
    let len = 360 * HOP;
    let mut lanes = [noise(len, 5), noise(len, 13)];
    let spur = tone(len, spur_hz - offsets[0], Complex::new(0.29, 0.0));
    for (sample, extra) in lanes[0].iter_mut().zip(&spur) {
        *sample += extra;
    }
    let run = |options| {
        let mut stitcher = with_options(2, options);
        let out = stitch(&mut stitcher, &lanes);
        let power = psd(&out[160 * 2 * HOP..], 8192);
        let spurs = (
            stitcher.lane_state(0).spur_bins,
            stitcher.lane_state(1).spur_bins,
        );
        (band_db(&power, -40..=-40), spurs)
    };
    let (kept, _) = run(PLAIN);
    for options in [SPUR_REJECT, StitchOptions::default()] {
        let (rejected, spurs) = run(options);
        assert!(kept - rejected > 20.0, "kept {kept} rejected {rejected}");
        assert_eq!(spurs, (1, 0), "{options:?}");
    }
}

#[test]
fn a_tone_both_lanes_hear_is_not_a_spur() {
    let offsets = auto_offsets(2, RATE);
    let len = 360 * HOP;
    let heard = [Complex::new(0.29, 0.0); 2];
    let mut lanes = lanes_hearing(-20.0, &offsets, len, &heard);
    for (lane, seed) in lanes.iter_mut().zip([5, 13]) {
        for (sample, extra) in lane.iter_mut().zip(noise(len, seed)) {
            *sample += extra;
        }
    }
    let run = |options| {
        let mut stitcher = with_options(2, options);
        let out = stitch(&mut stitcher, &lanes);
        let power = psd(&out[160 * 2 * HOP..], 8192);
        let spurs = (
            stitcher.lane_state(0).spur_bins,
            stitcher.lane_state(1).spur_bins,
        );
        (band_db(&power, -40..=-40), spurs)
    };
    let (plain, _) = run(PLAIN);
    let (kept, spurs) = run(StitchOptions::default());
    assert_eq!(spurs, (0, 0));
    assert!((plain - kept).abs() < 1.0, "plain {plain} kept {kept}");
}

#[test]
fn snr_blend_prefers_the_quieter_lane() {
    let len = 100 * HOP;
    let lanes = [noise(len, 3), louder(noise(len, 7), 2.0)];
    let mut stitcher = with_options(2, SNR_BLEND);
    stitch(&mut stitcher, &lanes);
    let (quiet, loud) = (share_at_centre(&stitcher, 0), share_at_centre(&stitcher, 1));
    assert!(loud < 0.3, "{loud}");
    assert!((quiet + loud - 1.0).abs() < 1e-3, "{quiet} {loud}");
    let mut even = with_options(2, PLAIN);
    stitch(&mut even, &lanes);
    assert!((share_at_centre(&even, 1) - 0.5).abs() < 0.01);
}

#[test]
fn snr_blend_keeps_the_crossfade_sum() {
    let len = 100 * HOP;
    let lanes = [noise(len, 3), louder(noise(len, 7), 2.0)];
    let layout = [-900.0, 900.0];
    let mut blended = with_options(2, SNR_BLEND);
    blended.retune(&layout).expect("layout");
    stitch(&mut blended, &lanes);
    let mut even = plain(2);
    even.retune(&layout).expect("layout");
    let (blended, even) = (share_sums(&blended), share_sums(&even));
    assert!(even.iter().any(|sum| *sum > 0.01 && *sum < 0.9));
    for (a, b) in blended.iter().zip(&even) {
        assert!((a - b).abs() < 1e-4, "{a} {b}");
    }
}

#[test]
fn option_changes_apply_at_once() {
    let len = 100 * HOP;
    let lanes = [noise(len, 3), louder(noise(len, 7), 2.0)];
    let mut stitcher = with_options(2, SNR_BLEND);
    stitch(&mut stitcher, &lanes);
    stitcher.set_options(StitchOptions {
        equalise: true,
        ..SNR_BLEND
    });
    assert!((share_at_centre(&stitcher, 1) - 0.5).abs() < 0.05);
    assert!((stitcher.lane_state(1).gain_db + 3.01).abs() < 0.3);
    stitcher.set_options(SNR_BLEND);
    assert!(share_at_centre(&stitcher, 1) < 0.3);
    assert_eq!(stitcher.lane_state(1).gain_db, 0.0);
    stitcher.set_options(PLAIN);
    assert!((share_at_centre(&stitcher, 1) - 0.5).abs() < 0.01);
}

#[test]
fn lane_state_reports_the_match() {
    let offsets = auto_offsets(2, RATE);
    let gains = [Complex::new(1.0, 0.0), Complex::from_polar(0.5, 1.1)];
    let lanes = multitone(&offsets, 120 * HOP, &gains);
    let mut stitcher = plain(2);
    stitch(&mut stitcher, &lanes);
    let state = stitcher.lane_state(1);
    assert!(
        (state.phase_deg + 1.1f32.to_degrees()).abs() < 1.0,
        "{state:?}"
    );
    assert!((state.gain_db - 6.02).abs() < 0.2, "{state:?}");
    assert!(
        state.coherence.is_some_and(|value| value > 0.9),
        "{state:?}"
    );
    assert_eq!(stitcher.lane_state(0).coherence, None);
    assert_eq!(stitcher.lane_state(0).phase_deg, 0.0);
    assert_eq!(stitcher.lane_state(2), LaneState::default());
}

#[test]
fn a_retune_starts_the_floor_estimate_over() {
    let len = 100 * HOP;
    let lanes = [noise(len, 3), louder(noise(len, 7), 2.0)];
    let mut stitcher = with_options(2, EQUALISE);
    stitch(&mut stitcher, &lanes);
    assert!(stitcher.lane_state(1).gain_db < -2.5);
    stitcher.retune(&[-900.0, 900.0]).expect("layout");
    assert_eq!(stitcher.lane_state(1).gain_db, 0.0);
    assert_eq!(stitcher.lane_state(1).noise_db, POWER_FLOOR_DB);
    stitch(&mut stitcher, &lanes);
    assert!(stitcher.lane_state(1).gain_db < -2.5);
    stitcher.reset();
    assert_eq!(stitcher.lane_state(1).gain_db, 0.0);
    assert_eq!(stitcher.lane_state(1).noise_db, POWER_FLOOR_DB);
}

#[test]
fn equalise_takes_only_the_phase_of_the_match() {
    let offsets = auto_offsets(2, RATE);
    let gains = [Complex::new(1.0, 0.0), Complex::from_polar(0.5, 1.1)];
    let lanes = multitone(&offsets, 200 * HOP, &gains);
    let mut stitcher = with_options(2, EQUALISE);
    stitch(&mut stitcher, &lanes);
    let (low, high) = (stitcher.lane_state(0), stitcher.lane_state(1));
    assert!(
        (high.phase_deg + 1.1f32.to_degrees()).abs() < 1.0,
        "{high:?}"
    );
    assert!((low.gain_db + 3.01).abs() < 0.3, "{low:?}");
    assert!((high.gain_db - 3.01).abs() < 0.3, "{high:?}");
    assert!((stitcher.lanes[1].correction.norm() - 1.0).abs() < 1e-4);
}

#[test]
fn a_weak_match_holds_the_last_phase() {
    let offsets = auto_offsets(2, RATE);
    let gains = [Complex::new(1.0, 0.0), Complex::from_polar(0.5, 1.1)];
    let mut stitcher = with_options(2, EQUALISE);
    stitch(&mut stitcher, &multitone(&offsets, 100 * HOP, &gains));
    let locked = stitcher.lane_state(1).phase_deg;
    stitch(&mut stitcher, &[noise(100 * HOP, 4), noise(100 * HOP, 8)]);
    let held = stitcher.lane_state(1);
    assert!(
        held.coherence.is_some_and(|value| value < MATCH_COHERENCE),
        "{held:?}"
    );
    assert!((held.phase_deg - locked).abs() < 0.5, "{locked} {held:?}");
}

#[test]
fn without_phase_matching_lanes_keep_their_own_phase() {
    let offsets = auto_offsets(2, RATE);
    let gains = [Complex::new(1.0, 0.0), Complex::from_polar(0.6, 2.3)];
    let options = StitchOptions {
        match_phase: false,
        ..PLAIN
    };
    let mut stitcher = with_options(2, options);
    let out = stitch(&mut stitcher, &lanes_hearing(3.0, &offsets, 60_000, &gains));
    let (amplitude, _) = fit(&out, 3.0, 2, out.len() / 2);
    assert!(
        (amplitude - Complex::new(1.0, 0.0)).norm() > 0.3,
        "{amplitude}"
    );
    let state = stitcher.lane_state(1);
    assert_eq!((state.phase_deg, state.gain_db), (0.0, 0.0));
    assert!(
        state.coherence.is_some_and(|value| value > 0.9),
        "{state:?}"
    );
}
