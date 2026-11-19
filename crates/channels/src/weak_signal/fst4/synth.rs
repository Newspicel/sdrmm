use std::f64::consts::{PI, TAU};

use sdrmm_dsp::math::special::erf;

use super::code::{Code, Payload};
use super::{Fst4Setup, SAMPLE_RATE, SYMBOLS, Timing, message, tones};

const BT: f64 = 2.0;

/// The audio of one FST4 or FST4W transmission of `text`, lowest tone at `audio_hz`, unit
/// amplitude from sample 0, as WSJT-X's `gen_fst4wave`.
pub(crate) fn waveform(setup: Fst4Setup, text: &str, audio_hz: f64) -> Option<Vec<f32>> {
    if !setup.is_supported() {
        return None;
    }
    let timing = Timing::of(setup.period_s)?;
    let payload = if setup.wspr {
        message::pack_beacon(text)?
    } else {
        message::scrambled(super::super::ftx::message::pack(text)?)
    };
    let kind = if setup.wspr {
        Payload::Wspr
    } else {
        Payload::Standard
    };
    let codeword = Code::new(kind).encode(&payload);
    Some(modulate(&tones(&codeword), timing.symbol_samples, audio_hz))
}

fn modulate(tones: &[u8; SYMBOLS], samples: usize, audio_hz: f64) -> Vec<f32> {
    let pulse = pulse(samples);
    let peak = TAU / samples as f64;
    let mut deviation = vec![0.0f64; (SYMBOLS + 2) * samples];
    for (symbol, &tone) in tones.iter().enumerate() {
        let start = symbol * samples;
        for (slot, &shape) in deviation[start..start + 3 * samples].iter_mut().zip(&pulse) {
            *slot += peak * shape * f64::from(tone);
        }
    }
    let carrier = TAU * audio_hz / f64::from(SAMPLE_RATE);
    let mut phase = 0.0f64;
    let mut wave: Vec<f32> = deviation[samples..(SYMBOLS + 1) * samples]
        .iter()
        .map(|&step| {
            let value = phase.sin() as f32;
            phase = (phase + step + carrier) % TAU;
            value
        })
        .collect();
    let ramp = samples / 4;
    let length = wave.len();
    for index in 0..ramp {
        let gain = ((1.0 - (TAU * index as f64 / (samples / 2) as f64).cos()) / 2.0) as f32;
        wave[index] *= gain;
        wave[length - 1 - index] *= gain;
    }
    wave
}

fn pulse(samples: usize) -> Vec<f64> {
    let scale = PI * (2.0 / 2f64.ln()).sqrt() * BT;
    (0..3 * samples)
        .map(|index| {
            let t = ((index + 1) as f64 - 1.5 * samples as f64) / samples as f64;
            0.5 * (erf(scale * (t + 0.5)) - erf(scale * (t - 0.5)))
        })
        .collect()
}
