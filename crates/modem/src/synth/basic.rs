use num_complex::Complex;

use super::{NARROW_HZ, Render, Signal, add_noise, seconds, shift, silence};

/// Test signals that belong to no decoder.
pub static BASIC: &[Signal] = &[
    Signal {
        id: "tone",
        label: "Carrier tone",
        rate_hz: NARROW_HZ,
        render: Render::Always(tone),
    },
    Signal {
        id: "two_tone",
        label: "Two tone",
        rate_hz: NARROW_HZ,
        render: Render::Always(two_tone),
    },
    Signal {
        id: "noise",
        label: "Noise",
        rate_hz: NARROW_HZ,
        render: Render::Always(noise),
    },
    Signal {
        id: "sweep",
        label: "Sweep",
        rate_hz: NARROW_HZ,
        render: Render::Always(sweep),
    },
];

fn tone() -> Vec<Complex<f32>> {
    vec![Complex::new(1.0, 0.0); seconds(NARROW_HZ, 1.0)]
}

fn two_tone() -> Vec<Complex<f32>> {
    let len = seconds(NARROW_HZ, 1.0);
    let mut low = vec![Complex::new(0.5, 0.0); len];
    let mut high = low.clone();
    shift(&mut low, -10_000.0, NARROW_HZ);
    shift(&mut high, 10_000.0, NARROW_HZ);
    low.iter().zip(high).map(|(a, b)| a + b).collect()
}

fn noise() -> Vec<Complex<f32>> {
    let mut iq = silence(seconds(NARROW_HZ, 1.0));
    add_noise(&mut iq, 0x5DEE_CE66, 0.5);
    iq
}

fn sweep() -> Vec<Complex<f32>> {
    let len = seconds(NARROW_HZ, 1.0);
    let span = NARROW_HZ * 0.8;
    let mut phase = 0.0f64;
    (0..len)
        .map(|k| {
            let hz = -span / 2.0 + span * k as f64 / len as f64;
            phase += std::f64::consts::TAU * hz / NARROW_HZ;
            Complex::from_polar(1.0, phase as f32)
        })
        .collect()
}
