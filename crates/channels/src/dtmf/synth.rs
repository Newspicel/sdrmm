use num_complex::Complex;
use sdrmm_dsp::DtmfKey;
use sdrmm_modem::synth::{AUDIO_HZ, Need, Render, Rendered, Signal, fm_modulate, silence};

const DEVIATION_HZ: f64 = 2_500.0;
const TONE_AMPLITUDE: f64 = 0.35;

/// `digits` keyed as DTMF for `tone_ms` each with `gap_ms` between, as audio at `rate`.
pub fn audio(digits: &str, tone_ms: u32, gap_ms: u32, rate: f64) -> Result<Vec<f32>, &'static str> {
    let samples = |ms: u32| (rate * f64::from(ms) / 1_000.0).round() as usize;
    let mut audio = vec![0.0; samples(100)];
    for symbol in digits.chars() {
        let (low, high) = DtmfKey::from_symbol(symbol)
            .ok_or("digits hold a symbol off the DTMF keypad")?
            .tones_hz();
        audio.extend((0..samples(tone_ms)).map(|n| {
            let t = std::f64::consts::TAU * n as f64 / rate;
            (TONE_AMPLITUDE * ((low * t).sin() + (high * t).sin())) as f32
        }));
        audio.resize(audio.len() + samples(gap_ms), 0.0);
    }
    Ok(audio)
}

/// `digits` as DTMF over narrow FM at `rate`, then silence.
pub fn transmission(
    digits: &str,
    tone_ms: u32,
    gap_ms: u32,
    rate: f64,
) -> Result<Vec<Complex<f32>>, &'static str> {
    let mut iq = fm_modulate(&audio(digits, tone_ms, gap_ms, rate)?, DEVIATION_HZ, rate);
    iq.extend(silence((rate * 0.2) as usize));
    Ok(iq)
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "dtmf",
    label: "DTMF",
    rate_hz: AUDIO_HZ,
    render: Render::Fallible(dtmf),
}];

fn dtmf() -> Rendered {
    transmission("0123456789*#ABCD", 70, 50, AUDIO_HZ).need("dtmf")
}
