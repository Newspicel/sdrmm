use num_complex::Complex;
use sdrmm_wire::RemoteIdPhy;

use super::{cck, plcp::psdu_bytes, quadrant, template};

#[test]
fn the_barker_template_spans_one_microsecond() {
    let template = template();
    assert_eq!(template.len(), 20);
    let sum: f32 = template.iter().sum();
    assert!((sum - 20.0 / 11.0).abs() < 1e-5, "{sum}");
    assert!(template.iter().all(|value| value.abs() <= 1.0 + 1e-6));
}

#[test]
fn dqpsk_quadrants_follow_the_gray_table() {
    let at = |degrees: f32| quadrant(Complex::from_polar(1.0, degrees.to_radians()));
    assert_eq!(at(5.0), [false, false]);
    assert_eq!(at(92.0), [false, true]);
    assert_eq!(at(178.0), [true, true]);
    assert_eq!(at(-88.0), [true, false]);
}

#[test]
fn length_in_microseconds_gives_bytes_at_every_rate() {
    assert_eq!(psdu_bytes(RemoteIdPhy::Dsss1m, 800, 0), 100);
    assert_eq!(psdu_bytes(RemoteIdPhy::Dsss2m, 400, 0), 100);
    assert_eq!(psdu_bytes(RemoteIdPhy::Cck5m5, 146, 0), 100);
    assert_eq!(psdu_bytes(RemoteIdPhy::Cck11m, 73, 0x80), 99);
    assert_eq!(psdu_bytes(RemoteIdPhy::Cck11m, 73, 0), 100);
}

#[test]
fn cck_codewords_are_distinct_and_unit_power() {
    let words: Vec<_> = (0..64u8)
        .map(|label| cck::codeword(0.0, cck::phases(RemoteIdPhy::Cck11m, label)))
        .collect();
    for (a, first) in words.iter().enumerate() {
        assert!(first.iter().all(|chip| (chip.norm() - 1.0).abs() < 1e-6));
        for second in &words[a + 1..] {
            let overlap: Complex<f32> = first.iter().zip(second).map(|(x, y)| x * y.conj()).sum();
            assert!(overlap.norm() < 7.9);
        }
    }
}
