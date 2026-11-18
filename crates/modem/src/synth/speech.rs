use std::f64::consts::{PI, TAU};

const FORMANTS: [(f64, f64); 3] = [(500.0, 80.0), (1_500.0, 110.0), (2_500.0, 170.0)];
const SPEECH_LOW_HZ: f64 = 300.0;
const SPEECH_HIGH_HZ: f64 = 3_000.0;

fn resonance(hz: f64, centre: f64, bandwidth: f64) -> f64 {
    let ratio = hz / centre;
    let real = 1.0 - ratio * ratio;
    let imag = ratio * bandwidth / centre;
    1.0 / real.hypot(imag)
}

fn harmonic_gain(hz: f64) -> f64 {
    if hz <= SPEECH_LOW_HZ * 0.5 || hz >= SPEECH_HIGH_HZ * 1.15 {
        return 0.0;
    }
    let edges = taper(hz, SPEECH_LOW_HZ * 0.5, SPEECH_LOW_HZ)
        * taper(SPEECH_HIGH_HZ * 1.15 - hz, 0.0, SPEECH_HIGH_HZ * 0.15);
    let tilt = 1.0 / (1.0 + hz / 400.0);
    let formants: f64 = FORMANTS
        .iter()
        .map(|&(centre, bandwidth)| resonance(hz, centre, bandwidth))
        .product();
    edges * tilt * formants
}

fn taper(hz: f64, start: f64, end: f64) -> f64 {
    let t = ((hz - start) / (end - start)).clamp(0.0, 1.0);
    0.5 - 0.5 * (PI * t).cos()
}

/// Voiced speech-like audio: a wandering pitch, three formants and syllable swell, 300 to
/// 3000 Hz, peaking at half scale.
#[must_use]
pub fn speech_audio(rate: f64, len: usize) -> Vec<f32> {
    let mut phase = 0.0;
    let mut out = Vec::with_capacity(len);
    for k in 0..len {
        let t = k as f64 / rate;
        let pitch_hz = 120.0 + 15.0 * (TAU * 1.7 * t).sin();
        phase += TAU * pitch_hz / rate;
        let syllable = 0.55 + 0.45 * (TAU * 3.3 * t).sin();
        let mut sample = 0.0;
        let mut harmonic = 1;
        loop {
            let hz = f64::from(harmonic) * pitch_hz;
            if hz >= SPEECH_HIGH_HZ * 1.15 {
                break;
            }
            sample += harmonic_gain(hz) * (f64::from(harmonic) * phase).sin();
            harmonic += 1;
        }
        out.push((syllable * sample) as f32);
    }
    let peak = out.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
    if peak > 0.0 {
        for s in &mut out {
            *s *= 0.5 / peak;
        }
    }
    out
}
