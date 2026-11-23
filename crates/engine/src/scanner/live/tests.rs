use std::{sync::Arc, time::Duration};

use super::*;

const MARGIN: f32 = 12.0;
const RESUME: Duration = Duration::from_millis(1_500);
const LOUD: f32 = 30.0;
const QUIET: f32 = 0.0;

fn board(pick: Pick, priority: &[f64]) -> Board {
    Board::new(
        vec![118.0e6, 118.1e6, 118.2e6, 118.3e6],
        priority,
        25_000.0,
        (MARGIN, RESUME),
        pick,
    )
}

fn at(start: Instant, ms: u64) -> Instant {
    start + Duration::from_millis(ms)
}

const OPEN: [bool; 4] = [false; 4];

#[test]
fn a_held_call_stays_while_busy_and_lets_go_after_the_resume_delay() {
    let mut b = board(Pick::Listed, &[]);
    let t = Instant::now();
    assert_eq!(b.frame(&[QUIET; 4], &OPEN, t), Move::Stay);
    assert_eq!(
        b.frame(&[QUIET, LOUD, QUIET, QUIET], &OPEN, t),
        Move::Hold(1)
    );
    assert_eq!(
        b.frame(&[QUIET, LOUD, QUIET, QUIET], &OPEN, at(t, 500)),
        Move::Stay
    );
    assert_eq!(b.frame(&[QUIET; 4], &OPEN, at(t, 600)), Move::Stay);
    assert_eq!(b.frame(&[QUIET; 4], &OPEN, at(t, 2_000)), Move::Stay);
    assert_eq!(b.frame(&[QUIET; 4], &OPEN, at(t, 2_100)), Move::Release);
    assert_eq!(b.parked(), 1, "the decoder stays where it last listened");
}

#[test]
fn a_quiet_hold_jumps_at_once_to_a_call_already_on_the_air_elsewhere() {
    let mut b = board(Pick::Listed, &[]);
    let t = Instant::now();
    assert_eq!(
        b.frame(&[LOUD, QUIET, QUIET, LOUD], &OPEN, t),
        Move::Hold(0)
    );
    assert_eq!(
        b.frame(&[QUIET, QUIET, QUIET, LOUD], &OPEN, at(t, 100)),
        Move::Stay
    );
    assert_eq!(
        b.frame(&[QUIET, QUIET, QUIET, LOUD], &OPEN, at(t, 200)),
        Move::Stay,
        "a short fade keeps the call"
    );
    assert_eq!(
        b.frame(&[QUIET, QUIET, QUIET, LOUD], &OPEN, at(t, 400)),
        Move::Hold(3),
        "no resume wait while another channel talks"
    );
}

#[test]
fn a_busy_priority_channel_takes_over_a_held_call() {
    let mut b = board(Pick::Listed, &[118.2e6]);
    let t = Instant::now();
    assert_eq!(
        b.frame(&[LOUD, QUIET, QUIET, QUIET], &OPEN, t),
        Move::Hold(0)
    );
    assert_eq!(
        b.frame(&[LOUD, QUIET, LOUD, QUIET], &OPEN, at(t, 50)),
        Move::Hold(2)
    );
    assert_eq!(
        b.frame(&[LOUD, QUIET, LOUD, QUIET], &OPEN, at(t, 100)),
        Move::Stay
    );
}

#[test]
fn strongest_picks_the_loudest_and_listed_the_first() {
    let snrs = [20.0, QUIET, 40.0, QUIET];
    let t = Instant::now();
    assert_eq!(
        board(Pick::Loudest, &[]).frame(&snrs, &OPEN, t),
        Move::Hold(2)
    );
    assert_eq!(
        board(Pick::Listed, &[]).frame(&snrs, &OPEN, t),
        Move::Hold(0)
    );
}

#[test]
fn locked_out_and_spill_neighbours_are_never_taken() {
    let mut b = Board::new(
        vec![118.0e6, 118.00833e6, 118.5e6],
        &[],
        10_000.0,
        (MARGIN, RESUME),
        Pick::Listed,
    );
    let t = Instant::now();
    assert_eq!(
        b.frame(&[LOUD, 20.0, LOUD], &[false, false, true], t),
        Move::Hold(0)
    );
    assert_eq!(
        b.frame(&[QUIET, 20.0, LOUD], &[false, false, true], at(t, 1_000)),
        Move::Stay,
        "neither the neighbour lit by the held call nor a lockout"
    );
}

#[test]
fn fresh_lists_only_what_just_went_busy() {
    let mut b = board(Pick::Listed, &[]);
    let t = Instant::now();
    b.frame(&[LOUD, QUIET, QUIET, QUIET], &OPEN, t);
    assert_eq!(b.fresh(), &[0]);
    b.frame(&[LOUD, QUIET, LOUD, QUIET], &OPEN, t);
    assert_eq!(b.fresh(), &[2]);
    b.frame(&[LOUD, QUIET, LOUD, QUIET], &OPEN, t);
    assert!(b.fresh().is_empty());
}

#[test]
fn a_plan_frames_whole_only_with_both_end_bands_inside_the_rate() {
    assert!(frames_whole((100.0e6, 100.2e6), 25_000.0, 240_000.0));
    assert!(!frames_whole((100.0e6, 100.22e6), 25_000.0, 240_000.0));
}

#[test]
fn the_middle_target_is_the_one_nearest_the_centre_of_the_plan() {
    assert_eq!(middle(&[1.0, 2.0, 3.0, 10.0]), 2);
    assert_eq!(middle(&[1.0, 9.0, 10.0]), 1);
    assert_eq!(middle(&[5.0]), 0);
    assert_eq!(middle(&[]), 0);
}

#[test]
fn a_plan_fits_only_with_every_band_inside_the_window() {
    let snap = SpectrumSnapshot {
        seq: 1,
        timestamp: 0,
        center_hz: 100e6,
        span_hz: 1_000_000.0,
        db: Arc::from(vec![-90.0f32; 1024]),
    };
    assert!(fits(&[99.6e6, 100.48e6], &snap, 25_000.0));
    assert!(!fits(&[99.6e6, 100.49e6], &snap, 25_000.0));
    assert!(!fits(&[], &snap, 25_000.0));
}

#[test]
fn a_jump_holds_through_other_calls_and_priority_until_it_lingers_out() {
    let mut b = board(Pick::Listed, &[118.3e6]);
    let t = Instant::now();
    assert_eq!(b.nearest(118.09e6), 1);
    b.pin(1);
    assert_eq!(
        b.frame(&[LOUD, QUIET, QUIET, LOUD], &OPEN, t),
        Move::Stay,
        "neither a call nor a priority channel takes a jump"
    );
    assert_eq!(b.parked(), 1);
    assert_eq!(
        b.frame(&[LOUD, QUIET, QUIET, LOUD], &OPEN, at(t, 9_900)),
        Move::Stay
    );
    assert_eq!(
        b.frame(&[LOUD, QUIET, QUIET, LOUD], &OPEN, at(t, 10_000)),
        Move::Release
    );
    assert_eq!(
        b.frame(&[LOUD, QUIET, QUIET, LOUD], &OPEN, at(t, 10_100)),
        Move::Hold(3),
        "once let go, the scan picks again"
    );
}
