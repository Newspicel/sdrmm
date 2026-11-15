use super::*;
use crate::testutil::{XorShift32, complex_tone};

const RATE: f64 = 48_000.0;
const THRESHOLD_DB: f32 = -30.0;
const HYSTERESIS_DB: f32 = 2.0;
const HOLD_S: f32 = 0.1;
/// Power of a complex sample whose parts are uniform in [-1, 1).
const UNIFORM_POWER: f32 = 2.0 / 3.0;

impl Squelch {
    fn process_alone(&mut self, iq: &[Complex<f32>]) -> bool {
        self.process(iq, &[])
    }
}

fn squelch() -> Squelch {
    Squelch::new(RATE, THRESHOLD_DB, HYSTERESIS_DB, HOLD_S)
}

fn tone_at_db(db: f32, len: usize) -> Vec<Complex<f32>> {
    let amp = db.db_to_amplitude();
    complex_tone(0.02, len).iter().map(|v| v * amp).collect()
}

#[test]
fn noise_floor_below_threshold_stays_closed() {
    let mut sq = squelch();
    let mut rng = XorShift32(0xdead_beef);
    for _ in 0..100 {
        let block = noise_at_db(-60.0, 480, &mut rng);
        assert!(!sq.process_alone(&block), "opened on noise floor");
    }
}

#[test]
fn burst_above_threshold_opens() {
    let mut sq = squelch();
    assert!(sq.process_alone(&tone_at_db(-10.0, 480)));
}

fn noise_at_db(db: f32, len: usize, rng: &mut XorShift32) -> Vec<Complex<f32>> {
    let scale = (db.db_to_power() / UNIFORM_POWER).sqrt();
    (0..len)
        .map(|_| Complex::new(rng.next_f32(), rng.next_f32()) * scale)
        .collect()
}

#[test]
fn dithering_inside_hysteresis_band_never_chatters() {
    let mut sq = squelch();
    assert!(sq.process_alone(&tone_at_db(-10.0, 480)));
    for i in 0..100 {
        let db = if i % 2 == 0 { -30.5 } else { -31.5 };
        assert!(
            sq.process_alone(&tone_at_db(db, 480)),
            "closed at block {i}"
        );
    }

    let mut sq = squelch();
    for i in 0..100 {
        let db = if i % 2 == 0 { -30.5 } else { -31.5 };
        assert!(
            !sq.process_alone(&tone_at_db(db, 480)),
            "opened at block {i}"
        );
    }
}

#[test]
fn a_floor_just_under_the_threshold_closes_the_gate_after_a_burst() {
    let mut sq = squelch();
    let mut rng = XorShift32(0xdead_beef);
    for _ in 0..100 {
        assert!(!sq.process_alone(&noise_at_db(-33.0, 480, &mut rng)));
    }
    assert!(sq.process_alone(&tone_at_db(-10.0, 480)));
    let mut open = true;
    for _ in 0..50 {
        open = sq.process_alone(&noise_at_db(-33.0, 480, &mut rng));
    }
    assert!(!open, "the gate wedged open on the noise floor");
}

#[test]
fn closes_only_after_hold_time() {
    let mut sq = squelch();
    assert!(sq.process_alone(&tone_at_db(-10.0, 480)));
    let silence = vec![Complex::new(0.0f32, 0.0); 480];
    let mut states = Vec::new();
    for _ in 0..15 {
        states.push(sq.process_alone(&silence));
    }
    assert!(states[8], "closed before the hold time");
    assert!(!states[12], "still open after the hold time");
}

#[test]
fn recovers_after_non_finite_sample() {
    let mut sq = squelch();
    assert!(sq.process_alone(&tone_at_db(-10.0, 480)));
    let mut poisoned = tone_at_db(-10.0, 480);
    poisoned[0] = Complex::new(f32::NAN, 0.0);
    let _ = sq.process_alone(&poisoned);
    let silence = vec![Complex::new(0.0f32, 0.0); 480];
    let mut open = true;
    for _ in 0..15 {
        open = sq.process_alone(&silence);
    }
    assert!(!open, "gate frozen open after NaN");

    let mut sq = squelch();
    let mut poisoned = tone_at_db(-60.0, 480);
    poisoned[0] = Complex::new(f32::NAN, f32::NAN);
    assert!(!sq.process_alone(&poisoned));
    assert!(
        sq.process_alone(&tone_at_db(-10.0, 480)),
        "gate frozen closed after NaN"
    );
}

#[test]
fn set_threshold_moves_the_open_point() {
    let mut sq = squelch();
    assert!(!sq.process_alone(&tone_at_db(-40.0, 4_800)));
    sq.set_threshold_db(-50.0);
    assert!(sq.process_alone(&tone_at_db(-40.0, 4_800)));
}

const MARGIN_DB: f32 = 8.0;

fn auto() -> Squelch {
    let mut sq = Squelch::new(RATE, 0.0, HYSTERESIS_DB, HOLD_S);
    sq.set_auto_margin_db(Some(MARGIN_DB));
    sq
}

#[test]
fn the_threshold_settles_a_margin_above_the_noise_it_hears() {
    for noise_db in [-70.0f32, -50.0, -30.0] {
        let mut sq = auto();
        let mut open = true;
        for _ in 0..200 {
            open = sq.process_alone(&tone_at_db(noise_db, 480));
        }
        assert!(!open, "gate open on the noise floor at {noise_db} dB");
        let landed = sq.threshold_db();
        assert!(
            (landed - (noise_db + MARGIN_DB)).abs() < 1.5,
            "threshold landed at {landed:.1} dB for a {noise_db} dB floor"
        );
    }
}

#[test]
fn a_burst_above_the_learned_floor_opens_the_gate() {
    let mut sq = auto();
    for _ in 0..200 {
        assert!(!sq.process_alone(&tone_at_db(-60.0, 480)));
    }
    assert!(
        sq.process_alone(&tone_at_db(-40.0, 480)),
        "burst did not open it"
    );
}

#[test]
fn a_long_transmission_does_not_squelch_itself() {
    let mut sq = auto();
    for _ in 0..200 {
        let _ = sq.process_alone(&tone_at_db(-60.0, 480));
    }
    assert!(sq.process_alone(&tone_at_db(-40.0, 480)));
    for i in 0..3_000 {
        assert!(
            sq.process_alone(&tone_at_db(-40.0, 480)),
            "closed at block {i}"
        );
    }
}

#[test]
fn the_threshold_follows_a_floor_that_creeps_up() {
    let mut sq = auto();
    for _ in 0..200 {
        let _ = sq.process_alone(&tone_at_db(-80.0, 480));
    }
    for step in 0..12_000 {
        let db = -80.0 + 20.0 * step as f32 / 12_000.0;
        assert!(
            !sq.process_alone(&tone_at_db(db, 480)),
            "opened on its own noise"
        );
    }
    let landed = sq.threshold_db();
    assert!(
        (landed - (-60.0 + MARGIN_DB)).abs() < 1.5,
        "threshold stayed at {landed:.1} dB"
    );
}

#[test]
fn auto_and_manual_thresholds_hand_over_to_each_other() {
    let mut sq = auto();
    for _ in 0..200 {
        let _ = sq.process_alone(&tone_at_db(-60.0, 480));
    }
    let learned = sq.threshold_db();

    sq.set_auto_margin_db(None);
    sq.set_threshold_db(-90.0);
    assert_eq!(sq.threshold_db(), -90.0);
    assert!(
        sq.process_alone(&tone_at_db(-60.0, 480)),
        "manual gate did not open"
    );

    sq.set_auto_margin_db(Some(MARGIN_DB));
    assert!(
        (sq.threshold_db() - learned).abs() < 1.0,
        "auto restarted at {:.1} instead of resuming {learned:.1}",
        sq.threshold_db()
    );
}

#[test]
fn a_reset_forgets_the_floor_the_channel_taught_it() {
    let mut sq = auto();
    for _ in 0..200 {
        let _ = sq.process_alone(&tone_at_db(-30.0, 480));
    }
    sq.reset();
    let mut open = true;
    for _ in 0..200 {
        open = sq.process_alone(&tone_at_db(-70.0, 480));
    }
    assert!(!open);
    assert!(
        sq.threshold_db() < -50.0,
        "the old floor survived a reset: {:.1} dB",
        sq.threshold_db()
    );
}

#[test]
fn an_automatic_gate_recovers_after_a_non_finite_sample() {
    let mut sq = auto();
    let mut poisoned = tone_at_db(-60.0, 480);
    poisoned[0] = Complex::new(f32::NAN, f32::NAN);
    let _ = sq.process_alone(&poisoned);
    for _ in 0..200 {
        let _ = sq.process_alone(&tone_at_db(-60.0, 480));
    }
    assert!(sq.threshold_db().is_finite());
    assert!(
        sq.process_alone(&tone_at_db(-30.0, 480)),
        "gate wedged after NaN"
    );
}

const BAND_HZ: f64 = 6_250.0;

fn guarded() -> Squelch {
    auto().with_guard_band(-BAND_HZ, BAND_HZ)
}

fn noise_with(db: f32, tone: Option<(f64, f32)>, rng: &mut XorShift32) -> Vec<Complex<f32>> {
    let mut block = noise_at_db(db, 480, rng);
    if let Some((freq, tone_db)) = tone {
        let amp = tone_db.db_to_amplitude();
        for (x, t) in block.iter_mut().zip(complex_tone(freq / RATE, 480)) {
            *x += t * amp;
        }
    }
    block
}

fn settle_on_noise(sq: &mut Squelch, db: f32, rng: &mut XorShift32) {
    for _ in 0..500 {
        let block = noise_at_db(db, 480, rng);
        let _ = sq.process(&block, &block);
    }
}

#[test]
fn a_floor_that_steps_up_does_not_open_the_gate() {
    let mut sq = guarded();
    let mut rng = XorShift32(0x5eed_0001);
    settle_on_noise(&mut sq, -70.0, &mut rng);
    for i in 0..100 {
        let block = noise_at_db(-50.0, 480, &mut rng);
        assert!(!sq.process(&block, &block), "opened at block {i}");
    }
}

#[test]
fn a_floor_that_steps_up_mid_call_closes_the_gate_when_the_call_ends() {
    let mut sq = guarded();
    let mut rng = XorShift32(0x5eed_0004);
    settle_on_noise(&mut sq, -70.0, &mut rng);
    for _ in 0..50 {
        let block = noise_with(-70.0, Some((1_000.0, -30.0)), &mut rng);
        assert!(sq.process(&block, &block));
    }
    for _ in 0..50 {
        let block = noise_with(-50.0, Some((1_000.0, -30.0)), &mut rng);
        assert!(sq.process(&block, &block));
    }
    let mut open = true;
    for _ in 0..50 {
        let block = noise_at_db(-50.0, 480, &mut rng);
        open = sq.process(&block, &block);
    }
    assert!(!open, "the stepped floor held the gate open");
}

#[test]
fn a_long_signal_inside_the_band_holds_a_guarded_gate_open() {
    let mut sq = guarded();
    let mut rng = XorShift32(0x5eed_0002);
    settle_on_noise(&mut sq, -70.0, &mut rng);
    for i in 0..3_000 {
        let block = noise_with(-70.0, Some((1_000.0, -40.0)), &mut rng);
        assert!(sq.process(&block, &block), "closed at block {i}");
    }
}

#[test]
fn a_signal_beside_the_channel_leaves_the_threshold_alone() {
    let mut sq = guarded();
    let mut rng = XorShift32(0x5eed_0003);
    settle_on_noise(&mut sq, -70.0, &mut rng);
    let before = sq.threshold_db();
    for _ in 0..200 {
        let inside = noise_at_db(-70.0, 480, &mut rng);
        let wide = noise_with(-70.0, Some((14_000.0, -30.0)), &mut rng);
        assert!(!sq.process(&inside, &wide));
    }
    assert!(
        (sq.threshold_db() - before).abs() < 1.0,
        "threshold moved from {before:.1} to {:.1} dB",
        sq.threshold_db()
    );
}

#[test]
fn a_wide_guard_watches_only_the_band_its_squelch_keeps() {
    let wide_rate = 2.0 * RATE;
    let level = |tone_hz: f64| {
        let mut guard =
            GuardMeter::new(wide_rate, flat_bandwidth_hz(RATE) / 2.0, -BAND_HZ, BAND_HZ)
                .expect("guard");
        let mut rng = XorShift32(0x5eed_0005);
        for _ in 0..400 {
            let mut block = noise_at_db(-70.0, 960, &mut rng);
            let above = complex_tone(tone_hz / wide_rate, 960);
            let below = complex_tone(-tone_hz / wide_rate, 960);
            for ((x, a), b) in block.iter_mut().zip(above).zip(below) {
                *x += (a + b) * 0.03;
            }
            guard.process(&block);
        }
        guard.level().power_to_db()
    };
    let quiet = level(0.0);
    let beyond = level(0.8 * RATE / 2.0 + RATE / 2.0);
    let inside = level(0.3 * RATE);
    assert!(
        (beyond - quiet).abs() < 1.0,
        "aliases moved the guard {quiet} to {beyond}"
    );
    assert!(
        inside > quiet + 10.0,
        "a neighbour in the guard band left it at {inside}"
    );
}

#[test]
fn a_band_that_fills_the_channel_has_no_guard() {
    assert!(GuardMeter::new(240_000.0, 96_000.0, -100_000.0, 100_000.0).is_none());
    assert!(GuardMeter::new(RATE, flat_bandwidth_hz(RATE) / 2.0, -BAND_HZ, BAND_HZ).is_some());
}
