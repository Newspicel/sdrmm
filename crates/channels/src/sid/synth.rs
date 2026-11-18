use num_complex::Complex;
use sdrmm_modem::synth::{AUDIO_HZ, Render, Signal};

use sdrmm_modem::synth::{add_noise, ook, silence};

#[must_use]
pub fn carriers(
    carriers: &[(f64, f32)],
    centre_hz: f64,
    seconds: f64,
    rate: f64,
    seed: u32,
) -> Vec<Complex<f32>> {
    let len = (seconds * rate) as usize;
    let mut iq = silence(len);
    for &(frequency_hz, amplitude) in carriers {
        let tone = ook(&vec![amplitude; len], frequency_hz - centre_hz, rate);
        iq.iter_mut().zip(tone).for_each(|(s, t)| *s += t);
    }
    add_noise(&mut iq, seed, 0.01);
    iq
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "vlf_stations",
    label: "VLF stations · 30 kHz",
    rate_hz: AUDIO_HZ,
    render: Render::Always(vlf_stations),
}];

const VLF_CENTRE_HZ: f64 = 30_000.0;

fn vlf_stations() -> Vec<Complex<f32>> {
    carriers(
        &[(19_580.0, 0.05), (23_400.0, 0.1), (24_000.0, 0.03)],
        VLF_CENTRE_HZ,
        10.0,
        AUDIO_HZ,
        0x51D,
    )
}
