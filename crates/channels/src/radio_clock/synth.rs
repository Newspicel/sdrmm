use num_complex::Complex;
use sdrmm_modem::synth::{Render, Signal};

pub const RATE: f64 = 2_000.0;

#[must_use]
pub fn dcf77() -> Vec<Complex<f32>> {
    let mut bits = [false; 59];
    bits[18] = true;
    bits[20] = true;
    bcd(
        &mut bits,
        &[
            (21, 1),
            (22, 2),
            (23, 4),
            (24, 8),
            (25, 10),
            (26, 20),
            (27, 40),
        ],
        34,
    );
    bcd(
        &mut bits,
        &[(29, 1), (30, 2), (31, 4), (32, 8), (33, 10), (34, 20)],
        12,
    );
    bcd(
        &mut bits,
        &[(36, 1), (37, 2), (38, 4), (39, 8), (40, 10), (41, 20)],
        15,
    );
    bcd(&mut bits, &[(42, 1), (43, 2), (44, 4)], 6);
    bcd(
        &mut bits,
        &[(45, 1), (46, 2), (47, 4), (48, 8), (49, 10)],
        8,
    );
    bcd(
        &mut bits,
        &[
            (50, 1),
            (51, 2),
            (52, 4),
            (53, 8),
            (54, 10),
            (55, 20),
            (56, 40),
            (57, 80),
        ],
        26,
    );
    even_parity(&mut bits, 21..=27, 28);
    even_parity(&mut bits, 29..=34, 35);
    even_parity(&mut bits, 36..=57, 58);

    let mut out = Vec::with_capacity(240_000);
    out.extend((0..2_000).map(|_| Complex::new(1.0, 0.0)));
    for _ in 0..2 {
        for &bit in &bits {
            let low_samples = if bit { 400 } else { 200 };
            out.extend(
                (0..2_000).map(|n| Complex::new(if n < low_samples { 0.1 } else { 1.0 }, 0.0)),
            );
        }
        out.extend((0..2_000).map(|_| Complex::new(1.0, 0.0)));
    }
    out
}

fn bcd(bits: &mut [bool], fields: &[(usize, u16)], value: u16) {
    let digits = [value % 10, value / 10 % 10, value / 100];
    for &(index, weight) in fields {
        let place = if weight >= 100 {
            2
        } else {
            usize::from(weight >= 10)
        };
        let binary_weight = weight / 10_u16.pow(place as u32);
        bits[index] = digits[place] & binary_weight != 0;
    }
}

fn even_parity(bits: &mut [bool], range: std::ops::RangeInclusive<usize>, at: usize) {
    bits[at] = range.filter(|&index| bits[index]).count() % 2 == 1;
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "dcf77",
    label: "DCF77 time code",
    rate_hz: RATE,
    render: Render::Always(dcf77_signal),
}];

fn dcf77_signal() -> Vec<Complex<f32>> {
    dcf77()
}

/// The reference captures the fixtures are written from.
pub(crate) static RECIPES: &[sdrmm_modem::synth::Recipe] = &[sdrmm_modem::synth::Recipe {
    stem: "dcf77_2026_2k",
    rate: RATE,
    note: "radio_clock (DCF77) -> 2026-08-15 12:34 CET with valid parity",
    iq: || Ok(dcf77()),
}];
