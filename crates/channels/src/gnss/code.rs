//! GPS L1 C/A Gold codes, IS-GPS-200 table 3-Ia.

pub(super) const CHIPS: usize = 1_023;
pub(super) const PRNS: u8 = 32;

/// G2 phase-selector taps per PRN.
const TAPS: [(usize, usize); PRNS as usize] = [
    (2, 6),
    (3, 7),
    (4, 8),
    (5, 9),
    (1, 9),
    (2, 10),
    (1, 8),
    (2, 9),
    (3, 10),
    (2, 3),
    (3, 4),
    (5, 6),
    (6, 7),
    (7, 8),
    (8, 9),
    (9, 10),
    (1, 4),
    (2, 5),
    (3, 6),
    (4, 7),
    (5, 8),
    (6, 9),
    (1, 3),
    (4, 6),
    (5, 7),
    (6, 8),
    (7, 9),
    (8, 10),
    (1, 6),
    (2, 7),
    (3, 8),
    (4, 9),
];

/// One period of the PRN's code as ±1, chip 0 first.
pub(super) fn ca_code(prn: u8) -> [f32; CHIPS] {
    let (tap1, tap2) = TAPS[usize::from(prn.saturating_sub(1).min(PRNS - 1))];
    let mut g1 = [true; 10];
    let mut g2 = [true; 10];
    let mut out = [0.0; CHIPS];
    for chip in &mut out {
        let bit = g1[9] ^ g2[tap1 - 1] ^ g2[tap2 - 1];
        *chip = if bit { -1.0 } else { 1.0 };
        let g1_feedback = g1[2] ^ g1[9];
        let g2_feedback = g2[1] ^ g2[2] ^ g2[5] ^ g2[7] ^ g2[8] ^ g2[9];
        g1.copy_within(0..9, 1);
        g2.copy_within(0..9, 1);
        g1[0] = g1_feedback;
        g2[0] = g2_feedback;
    }
    out
}

/// The code with one chip of wrap on each side: index `k + 1` holds chip `k`, so an early or
/// late replica half a chip off either end needs no modulo.
pub(super) fn padded_code(prn: u8) -> [f32; CHIPS + 2] {
    let code = ca_code(prn);
    let mut out = [0.0; CHIPS + 2];
    out[0] = code[CHIPS - 1];
    out[1..=CHIPS].copy_from_slice(&code);
    out[CHIPS + 1] = code[0];
    out
}

/// The code held for each of `samples` samples spanning one period.
pub(super) fn sampled_code(prn: u8, samples: usize) -> Vec<f32> {
    let mut out = vec![0.0; samples];
    fill_sampled_code(prn, &mut out);
    out
}

/// Writes one period of the code across `out`, each sample holding the chip it falls in.
pub(super) fn fill_sampled_code(prn: u8, out: &mut [f32]) {
    let chips = ca_code(prn);
    let samples = out.len();
    for (n, slot) in out.iter_mut().enumerate() {
        *slot = chips[n * CHIPS / samples];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPS_SPS_TABLE_2_1_FIRST_TEN_CHIPS_OCTAL: [u16; 32] = [
        0o1440, 0o1620, 0o1710, 0o1744, 0o1133, 0o1455, 0o1131, 0o1454, 0o1626, 0o1504, 0o1642,
        0o1750, 0o1764, 0o1772, 0o1775, 0o1776, 0o1156, 0o1467, 0o1633, 0o1715, 0o1746, 0o1763,
        0o1063, 0o1706, 0o1743, 0o1761, 0o1770, 0o1774, 0o1127, 0o1453, 0o1625, 0o1712,
    ];

    #[test]
    fn every_prn_reproduces_the_published_first_ten_chips_octal() {
        for (index, &octal) in GPS_SPS_TABLE_2_1_FIRST_TEN_CHIPS_OCTAL.iter().enumerate() {
            let prn = index as u8 + 1;
            let chips = ca_code(prn);
            let first_ten = chips[..10]
                .iter()
                .fold(0u16, |value, &chip| (value << 1) | u16::from(chip < 0.0));
            assert_eq!(first_ten, octal, "PRN {prn} starts {first_ten:04o}");
        }
    }

    const GOLD_CORRELATION_VALUES: [i32; 3] = [-65, -1, 63];

    fn periodic_correlation(one: &[f32; CHIPS], two: &[f32; CHIPS], shift: usize) -> i32 {
        one.iter()
            .enumerate()
            .map(|(index, &chip)| chip * two[(index + shift) % CHIPS])
            .sum::<f32>() as i32
    }

    #[test]
    fn code_correlation_only_ever_takes_the_three_published_gold_values() {
        for (a, b) in [(1u8, 2u8), (7, 19), (12, 25), (31, 32)] {
            let one = ca_code(a);
            let two = ca_code(b);
            assert_eq!(periodic_correlation(&one, &one, 0), CHIPS as i32);
            for shift in 0..CHIPS {
                let cross = periodic_correlation(&one, &two, shift);
                assert!(
                    GOLD_CORRELATION_VALUES.contains(&cross),
                    "PRN {a} against {b} at shift {shift} is {cross}"
                );
                if shift > 0 {
                    let sidelobe = periodic_correlation(&one, &one, shift);
                    assert!(
                        GOLD_CORRELATION_VALUES.contains(&sidelobe),
                        "PRN {a} sidelobe at shift {shift} is {sidelobe}"
                    );
                }
            }
        }
    }

    #[test]
    fn padding_wraps_one_chip_each_way() {
        let code = ca_code(9);
        let padded = padded_code(9);
        assert_eq!(padded[0], code[CHIPS - 1]);
        assert_eq!(&padded[1..=CHIPS], &code);
        assert_eq!(padded[CHIPS + 1], code[0]);
    }
}
