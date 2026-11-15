use num_complex::Complex;

use super::*;

fn schedule(channels: usize, hops: usize) -> HopSequence {
    HopSequence::from_m_sequence(channels, 0.01, 64, hops, 9).unwrap()
}

#[test]
fn hopping_and_dehopping_is_the_identity() {
    let sequence = schedule(16, 32);
    let mut wave: Vec<Complex<f32>> = (0..4_000)
        .map(|k| {
            let a = f32::from(u8::try_from(k % 7).unwrap_or(0)) - 3.0;
            let b = f32::from(u8::try_from(k % 5).unwrap_or(0)) - 2.0;
            Complex::new(a * 0.1, b * 0.1)
        })
        .collect();
    let original = wave.clone();
    FhssMod::new(sequence.clone()).hop(&mut wave);
    let hopped = wave.clone();
    FhssDemod::new(sequence).dehop(&mut wave);
    for (k, (&got, &want)) in wave.iter().zip(&original).enumerate() {
        assert!((got - want).norm() < 1e-5, "sample {k}: {got} vs {want}");
    }
    let moved = hopped
        .iter()
        .zip(&original)
        .filter(|&(&a, &b)| (a - b).norm() > 1e-3)
        .count();
    assert!(moved > original.len() / 2, "only {moved} samples moved");
}

#[test]
fn each_dwell_sits_on_its_own_carrier() {
    let sequence = schedule(8, 16);
    let dwell = sequence.dwell_samples();
    let mut wave = vec![Complex::new(1.0f32, 0.0); dwell * 8];
    FhssMod::new(sequence.clone()).hop(&mut wave);
    for hop in 0..8 {
        let at = hop * dwell + dwell / 2;
        let step = wave[at + 1] * wave[at].conj();
        let measured = f64::from(step.arg()) / std::f64::consts::TAU;
        let want = sequence.offset_cycles(sequence.channel(hop));
        assert!(
            (measured - want).abs() < 1e-4,
            "hop {hop} on channel {}: measured {measured}, planned {want}",
            sequence.channel(hop)
        );
    }
}

#[test]
fn the_generated_schedule_visits_its_whole_plan() {
    for channels in [3usize, 4, 5, 8, 16, 32] {
        let hops = channels * 8;
        let sequence = schedule(channels, hops);
        assert_eq!(
            sequence.visits(hops),
            channels,
            "{channels} channels over {hops} hops"
        );
        let demod = FhssDemod::new(sequence);
        for channel in 0..channels {
            let dwells = demod.dwells_on(channel, hops);
            assert!(
                (2..=24).contains(&dwells),
                "{channels} channels: channel {channel} takes {dwells} of {hops} dwells"
            );
        }
    }
}

#[test]
fn the_channel_plan_is_centred_on_baseband() {
    let sequence = schedule(9, 16);
    assert!(sequence.offset_cycles(4).abs() < 1e-12);
    assert!((sequence.offset_cycles(8) + sequence.offset_cycles(0)).abs() < 1e-12);
    assert!((sequence.occupied_cycles() - 8.0 * 0.01).abs() < 1e-12);
    let even = schedule(8, 16);
    assert!((even.offset_cycles(3) + even.offset_cycles(4)).abs() < 1e-12);
}

#[test]
fn a_short_order_wraps() {
    let sequence = HopSequence::new(4, 0.02, 16, vec![2, 0, 3]).unwrap();
    assert_eq!(sequence.channel(0), 2);
    assert_eq!(sequence.channel(3), 2);
    assert_eq!(sequence.channel(7), 0);
    assert_eq!(sequence.span_samples(), 48);
}

#[test]
fn degenerate_schedules_are_rejected_with_the_right_error() {
    assert_eq!(
        HopSequence::new(1, 0.01, 16, vec![0]).unwrap_err(),
        FhssError::DegenerateSchedule
    );
    assert_eq!(
        HopSequence::new(4, 0.01, 0, vec![0]).unwrap_err(),
        FhssError::DegenerateSchedule
    );
    assert_eq!(
        HopSequence::new(4, 0.01, 16, vec![]).unwrap_err(),
        FhssError::EmptyOrder
    );
    assert_eq!(
        HopSequence::new(4, 0.01, 16, vec![0, 9]).unwrap_err(),
        FhssError::ChannelOutOfRange(9)
    );
    assert!(matches!(
        HopSequence::from_m_sequence(4, 0.01, 16, 8, 99).unwrap_err(),
        FhssError::Sequence(_)
    ));
}

#[test]
fn the_generator_refuses_a_degenerate_plan_instead_of_drawing_for_it() {
    for channels in [0usize, 1] {
        assert_eq!(
            HopSequence::from_m_sequence(channels, 0.01, 16, 8, 7).unwrap_err(),
            FhssError::DegenerateSchedule
        );
    }
    assert_eq!(
        HopSequence::from_m_sequence(4, 0.01, 0, 8, 7).unwrap_err(),
        FhssError::DegenerateSchedule
    );
    assert_eq!(
        HopSequence::from_m_sequence(4, 0.01, 16, 0, 7).unwrap_err(),
        FhssError::EmptyOrder
    );
}
