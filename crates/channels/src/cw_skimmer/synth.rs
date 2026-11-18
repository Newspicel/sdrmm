use num_complex::Complex;
use sdrmm_modem::synth::{AUDIO_HZ, Recipe, morse::transmission};

/// The reference captures the fixtures are written from.
pub(crate) static RECIPES: &[Recipe] = &[Recipe {
    stem: "cw_skimmer_dual_48k",
    rate: AUDIO_HZ,
    note: "cw_skimmer channel -> simultaneous DL1AAA at -3.5 kHz/18 wpm and G4BBB at +4.2 kHz/27 wpm",
    iq: || Ok(fixture_pair()),
}];

fn fixture_pair() -> Vec<Complex<f32>> {
    let first = transmission("VVV CQ DE DL1AAA K", 18.0, -3_500.0, AUDIO_HZ);
    let second = transmission("VVV CQ DE G4BBB K", 27.0, 4_200.0, AUDIO_HZ);
    let mut iq =
        vec![Complex::new(0.0, 0.0); first.len().max(second.len()) + AUDIO_HZ as usize * 4];
    for (destination, source) in iq.iter_mut().zip(first) {
        *destination += source * 0.55;
    }
    for (destination, source) in iq.iter_mut().zip(second) {
        *destination += source * 0.35;
    }
    iq
}
