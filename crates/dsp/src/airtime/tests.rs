use num_complex::Complex;

use super::*;
use crate::testutil::{XorShift32, complex_tone};

fn noise(len: usize, power: f32, seed: u32) -> Vec<Complex<f32>> {
    let mut rng = XorShift32(seed);
    let scale = (power / (2.0 / 3.0)).sqrt();
    (0..len)
        .map(|_| Complex::new(rng.next_f32(), rng.next_f32()) * scale)
        .collect()
}

fn frames(iq: &[Complex<f32>], len: usize) -> Vec<Vec<f32>> {
    let mut power = PowerFrames::new(len);
    let mut out = Vec::new();
    for chunk in iq.chunks(1_000) {
        power.process(chunk, |frame| out.push(frame.to_vec()));
    }
    out
}

#[test]
fn frame_power_sums_to_the_sample_power() {
    let out = frames(&noise(64 * 4_000, 0.01, 7), 64);
    assert_eq!(out.len(), 4_000);
    let mean = out.iter().map(|f| f.iter().sum::<f32>()).sum::<f32>() / out.len() as f32;
    assert!((mean / 0.01 - 1.0).abs() < 0.03, "{mean}");
}

#[test]
fn a_tone_lands_in_its_shifted_bin() {
    let out = frames(&complex_tone(-0.25, 64 * 4), 64);
    for frame in &out {
        let peak = (0..64)
            .max_by(|&a, &b| frame[a].total_cmp(&frame[b]))
            .unwrap();
        assert_eq!(peak, 16);
        assert!((frame.iter().sum::<f32>() - 1.0).abs() < 0.01);
    }
}

fn band_db(frame: &[f32], bins: std::ops::Range<usize>) -> f32 {
    10.0 * frame[bins].iter().sum::<f32>().log10()
}

#[test]
fn the_floor_reads_noise_through_a_busy_channel() {
    let mut iq = noise(64 * 20_000, 1e-4, 3);
    for (index, sample) in iq.iter_mut().enumerate() {
        if (index / 640) % 5 < 3 {
            *sample += Complex::from_polar(0.1, index as f32 * 0.7);
        }
    }
    let mut floor = QuietFloor::new(band_degrees(52), 1.0);
    for frame in frames(&iq, 64) {
        floor.observe(band_db(&frame, 6..58));
    }
    let measured = floor.settle().unwrap();
    let truth = 10.0 * (1e-4f32 * 52.0 / 64.0).log10();
    assert!((measured - truth).abs() < 0.6, "{measured} vs {truth}");
}

#[test]
fn a_single_bin_floor_matches_the_mean_bin_power() {
    let mut floor = QuietFloor::new(band_degrees(1), 1.0);
    for frame in frames(&noise(64 * 4_000, 1e-4, 11), 64) {
        floor.observe(10.0 * frame[20].log10());
    }
    let measured = floor.settle().unwrap();
    let truth = 10.0 * (1e-4f32 / 64.0).log10();
    assert!((measured - truth).abs() < 1.5, "{measured} vs {truth}");
}

#[test]
fn the_floor_falls_at_once_and_rises_slowly() {
    let mut floor = QuietFloor::new(200.0, 0.5);
    for _ in 0..100 {
        floor.observe(-60.0);
    }
    let first = floor.settle().unwrap();
    for _ in 0..100 {
        floor.observe(-40.0);
    }
    assert!((floor.settle().unwrap() - (first + 0.5)).abs() < 1e-3);
    for _ in 0..100 {
        floor.observe(-80.0);
    }
    assert!(floor.settle().unwrap() < first - 15.0);
    assert!(QuietFloor::new(2.0, 1.0).settle().is_none());
}

fn block(
    bins: usize,
    on: &[(std::ops::RangeInclusive<usize>, std::ops::Range<u64>)],
    frame: u64,
) -> Vec<f32> {
    (0..bins)
        .map(|bin| {
            let lit = on
                .iter()
                .any(|(span, time)| span.contains(&bin) && time.contains(&frame));
            if lit { 10.0 } else { 0.1 }
        })
        .collect()
}

fn run(
    bins: usize,
    capacity: usize,
    on: &[(std::ops::RangeInclusive<usize>, std::ops::Range<u64>)],
    total: u64,
) -> (Vec<Burst>, u64) {
    let mut bursts = Bursts::new(bins, capacity, 1_000);
    let floor = vec![0.1; bins];
    let mut out = Vec::new();
    for frame in 0..total {
        bursts.frame(&block(bins, on, frame), &floor, 10.0, |burst| {
            out.push(burst)
        });
    }
    bursts.flush(|burst| out.push(burst));
    (out, bursts.dropped())
}

#[test]
fn a_block_of_energy_is_one_burst() {
    let (out, dropped) = run(64, 8, &[(10..=14, 5..35)], 60);
    assert_eq!(dropped, 0);
    assert_eq!(out.len(), 1);
    let burst = out[0];
    assert_eq!(
        (burst.first, burst.frames, burst.low, burst.high),
        (5, 30, 10, 14)
    );
    assert!((burst.centre - 12.0).abs() < 1e-3);
    assert!((burst.mean - 49.5).abs() < 1e-3);
    assert!((burst.peak - 50.0).abs() < 1e-3);
    assert!(!burst.edge && !burst.cut);
}

#[test]
fn separate_channels_are_separate_bursts() {
    let (mut out, _) = run(64, 8, &[(2..=4, 0..10), (40..=63, 3..20)], 30);
    out.sort_by_key(|burst| burst.low);
    assert_eq!(out.len(), 2);
    assert_eq!((out[0].low, out[0].high, out[0].frames), (2, 4, 10));
    assert_eq!((out[1].low, out[1].high, out[1].frames), (41, 62, 17));
    assert!(out[1].edge);
}

#[test]
fn a_one_frame_gap_does_not_split_a_burst() {
    let (out, _) = run(64, 8, &[(10..=14, 0..10), (10..=14, 11..20)], 30);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].frames, 20);
}

#[test]
fn blips_are_not_bursts() {
    let (out, _) = run(64, 8, &[(10..=10, 0..3), (30..=31, 5..7)], 30);
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn bursts_beyond_capacity_are_counted() {
    let (out, dropped) = run(
        64,
        2,
        &[(2..=4, 0..10), (20..=24, 0..10), (40..=44, 0..10)],
        20,
    );
    assert_eq!(out.len(), 2);
    assert_eq!(dropped, 10);
}

#[test]
fn an_endless_carrier_is_cut_into_pieces() {
    let mut bursts = Bursts::new(16, 4, 100);
    let floor = vec![0.1; 16];
    let mut out = Vec::new();
    for frame in 0..250 {
        bursts.frame(
            &block(16, &[(5..=6, 0..1_000)], frame),
            &floor,
            10.0,
            |burst| {
                out.push(burst);
            },
        );
    }
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|burst| burst.cut && burst.frames == 100));
}

#[test]
fn a_burst_against_unusable_bins_touches_the_edge() {
    let mut bursts = Bursts::new(16, 4, 1_000);
    let mut floor = vec![0.1; 16];
    floor[..3].fill(f32::INFINITY);
    let mut out = Vec::new();
    for frame in 0..20 {
        bursts.frame(
            &block(16, &[(3..=6, 0..10)], frame),
            &floor,
            10.0,
            |burst| {
                out.push(burst);
            },
        );
    }
    assert_eq!(out.len(), 1);
    assert!(out[0].edge);
    assert_eq!((out[0].low, out[0].high), (3, 6));
}
