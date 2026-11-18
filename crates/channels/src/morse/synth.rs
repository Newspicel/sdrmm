use num_complex::Complex;
use sdrmm_modem::synth::{AUDIO_HZ, Render, Signal, morse::transmission};

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "morse",
    label: "Morse",
    rate_hz: AUDIO_HZ,
    render: Render::Always(morse),
}];

fn morse() -> Vec<Complex<f32>> {
    transmission("CQ CQ DE SDRmm K", 20.0, 800.0, AUDIO_HZ)
}

/// The reference captures the fixtures are written from.
pub(crate) static RECIPES: &[sdrmm_modem::synth::Recipe] = &[sdrmm_modem::synth::Recipe {
    stem: "morse_20wpm_48k",
    rate: sdrmm_modem::synth::AUDIO_HZ,
    note: "morse channel at -5 kHz -> \"CQ DE DL1ABC K\" at 20 wpm",
    iq: || {
        Ok(sdrmm_modem::synth::shifted(
            transmission("CQ DE DL1ABC K", 20.0, 0.0, sdrmm_modem::synth::AUDIO_HZ),
            -5_000.0,
            sdrmm_modem::synth::AUDIO_HZ,
        ))
    },
}];
