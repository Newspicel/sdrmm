//! An LTR repeater keyed from scratch: its status words under a voice tone.

use num_complex::Complex;
use sdrmm_dsp::Biquad;
use sdrmm_modem::synth::{bits, fm_modulate, tone_audio};

use super::BAUD;
use super::word::WORD_BITS;
pub use super::word::Word;

/// The data's share of the peak deviation: subaudible, well under the voice.
const DATA_LEVEL: f32 = 0.25;
const VOICE_LEVEL: f32 = 0.5;
const DATA_CORNER_HZ: f64 = 300.0;

/// The repeater's words, back to back, as NRZ under the voice, band-limited as a radio sends it.
#[must_use]
pub fn data_audio(words: &[Word], rate: f64) -> Vec<f32> {
    let keyed: Vec<bool> = words
        .iter()
        .flat_map(|word| bits(word.encode(), WORD_BITS as usize))
        .collect();
    let per_bit = rate / BAUD;
    let mut audio: Vec<f32> = (0..(keyed.len() as f64 * per_bit) as usize)
        .map(|n| {
            if keyed[((n as f64 / per_bit) as usize).min(keyed.len() - 1)] {
                DATA_LEVEL
            } else {
                -DATA_LEVEL
            }
        })
        .collect();
    for _ in 0..2 {
        Biquad::lowpass(rate, DATA_CORNER_HZ, std::f64::consts::FRAC_1_SQRT_2).process(&mut audio);
    }
    audio
}

/// A repeater carrying `call` for `repeats` words with a voice tone, then its turn-off words.
#[must_use]
pub fn transmission(call: Word, repeats: usize, deviation_hz: f64, rate: f64) -> Vec<Complex<f32>> {
    let turn_off = Word { goto: 31, ..call };
    let mut words = vec![call; repeats];
    words.extend([turn_off; 3]);
    let data = data_audio(&words, rate);
    let voiced = data.len() * repeats / words.len();
    let voice = tone_audio(1_000.0, VOICE_LEVEL, rate, voiced);
    let mixed: Vec<f32> = data
        .iter()
        .enumerate()
        .map(|(n, &d)| d + voice.get(n).copied().unwrap_or(0.0))
        .collect();
    fm_modulate(&mixed, deviation_hz, rate)
}

/// A call on repeater 5 for group 4 homed there, repeater 2 free.
#[must_use]
pub const fn call() -> Word {
    Word {
        area: false,
        goto: 5,
        home: 5,
        group: 4,
        free: 2,
        inbound: false,
    }
}
