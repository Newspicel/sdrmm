use std::f32::consts::{FRAC_PI_2, PI};

use num_complex::Complex;

use super::{WifiPhy, barker, cck_codeword, header_crc, psdu_bytes, quadrant, template};

#[test]
fn the_shared_barker_word_is_the_802_11_sequence() {
    assert_eq!(
        barker().unwrap(),
        [1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0]
    );
}

#[test]
fn the_barker_template_spans_one_microsecond() {
    let template = template(&barker().unwrap());
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
    assert_eq!(psdu_bytes(WifiPhy::Dsss1m, 800, 0), 100);
    assert_eq!(psdu_bytes(WifiPhy::Dsss2m, 400, 0), 100);
    assert_eq!(psdu_bytes(WifiPhy::Cck5m5, 146, 0), 100);
    assert_eq!(psdu_bytes(WifiPhy::Cck11m, 73, 0x80), 99);
    assert_eq!(psdu_bytes(WifiPhy::Cck11m, 73, 0), 100);
}

fn qpsk(first: bool, second: bool) -> f32 {
    match (first, second) {
        (false, false) => 0.0,
        (false, true) => FRAC_PI_2,
        (true, false) => PI,
        (true, true) => 3.0 * FRAC_PI_2,
    }
}

fn standard_codeword(phi: [f32; 4]) -> [Complex<f32>; 8] {
    let [p1, p2, p3, p4] = phi;
    let chip = |phase: f32, sign: f32| Complex::from_polar(sign, phase);
    [
        chip(p1 + p2 + p3 + p4, 1.0),
        chip(p1 + p3 + p4, 1.0),
        chip(p1 + p2 + p4, 1.0),
        chip(p1 + p4, -1.0),
        chip(p1 + p2 + p3, 1.0),
        chip(p1 + p3, 1.0),
        chip(p1 + p2, -1.0),
        chip(p1, 1.0),
    ]
}

fn assert_close(a: &[Complex<f32>; 8], b: &[Complex<f32>; 8]) {
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).norm() < 1e-5, "{a:?} vs {b:?}");
    }
}

#[test]
fn the_shared_cck_codebook_follows_the_802_11_codeword_table() {
    for label in 0..64u8 {
        let bits: [bool; 6] = std::array::from_fn(|bit| label >> bit & 1 == 1);
        let phi = [
            0.3,
            qpsk(bits[0], bits[1]),
            qpsk(bits[2], bits[3]),
            qpsk(bits[4], bits[5]),
        ];
        assert_close(
            &cck_codeword(WifiPhy::Cck11m, 0.3, &bits),
            &standard_codeword(phi),
        );
    }
    for label in 0..4u8 {
        let bits = [label & 1 == 1, label & 2 == 2];
        let phi = [
            -0.7,
            if bits[0] { PI } else { 0.0 } + FRAC_PI_2,
            0.0,
            if bits[1] { PI } else { 0.0 },
        ];
        assert_close(
            &cck_codeword(WifiPhy::Cck5m5, -0.7, &bits),
            &standard_codeword(phi),
        );
    }
}

#[test]
fn the_header_check_is_ones_complement_ccitt() {
    let header: Vec<bool> = (0..32).map(|bit| bit % 3 == 0).collect();
    let crc = header_crc(&header);
    let mut with_crc = header.clone();
    with_crc.extend((0..16).map(|bit| crc >> (15 - bit) & 1 == 1));
    assert_ne!(crc, 0);
    assert_eq!(header_crc(&with_crc[..32]), crc);
}
