use std::f64::consts::TAU;

use num_complex::Complex;

use super::iq::carrier;

#[must_use]
pub fn tone_audio(freq_hz: f64, amplitude: f32, rate: f64, len: usize) -> Vec<f32> {
    (0..len)
        .map(|k| amplitude * (TAU * freq_hz * k as f64 / rate).cos() as f32)
        .collect()
}

#[must_use]
pub fn fm_modulate(audio: &[f32], deviation_hz: f64, rate: f64) -> Vec<Complex<f32>> {
    let mut phase = 0.0f64;
    audio
        .iter()
        .map(|&s| {
            phase += TAU * deviation_hz * f64::from(s) / rate;
            if phase > TAU {
                phase -= TAU;
            } else if phase < -TAU {
                phase += TAU;
            }
            Complex::from_polar(1.0, phase as f32)
        })
        .collect()
}

#[must_use]
pub fn fsk(bits: &[bool], baud: f64, deviation_hz: f64, rate: f64) -> Vec<Complex<f32>> {
    let sps = rate / baud;
    let symbols: Vec<f32> = (0..(bits.len() as f64 * sps) as usize)
        .map(|k| {
            let idx = ((k as f64 / sps) as usize).min(bits.len() - 1);
            if bits[idx] { 1.0 } else { -1.0 }
        })
        .collect();
    fm_modulate(&symbols, deviation_hz, rate)
}

#[must_use]
pub fn ook(key: &[f32], tone_hz: f64, rate: f64) -> Vec<Complex<f32>> {
    key.iter()
        .zip(carrier(tone_hz, rate))
        .map(|(&env, rotor)| rotor * env)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsk_produces_the_requested_duration_and_unit_magnitude() {
        let bits = [true, false, true, true, false, false, true, false];
        let iq = fsk(&bits, 1_200.0, 4_500.0, 48_000.0);
        assert_eq!(iq.len(), 8 * 40);
        for s in &iq {
            assert!((s.norm() - 1.0).abs() < 1e-3, "magnitude {}", s.norm());
        }
    }

    #[test]
    fn a_long_carrier_holds_its_phase_step_to_the_end() {
        let rate = 48_000.0;
        let tone = 3_500.0;
        let iq = ook(&vec![1.0f32; rate as usize * 6], tone, rate);
        let step = (TAU * tone / rate) as f32;
        let worst = iq
            .windows(2)
            .map(|pair| {
                let advance = (pair[1] * pair[0].conj()).arg();
                (advance - step)
                    .rem_euclid(TAU as f32)
                    .min((step - advance).rem_euclid(TAU as f32))
            })
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "phase step drifted by {worst} rad");
    }

    #[test]
    fn fm_modulation_tracks_the_audio_deviation() {
        let rate = 48_000.0;
        let audio = tone_audio(1_000.0, 1.0, rate, 4_800);
        let iq = fm_modulate(&audio, 2_500.0, rate);
        let peak = iq
            .windows(2)
            .map(|pair| f64::from((pair[1] * pair[0].conj()).arg()) * rate / TAU)
            .fold(0.0f64, |worst, hz| worst.max(hz.abs()));
        assert!((peak / 2_500.0 - 1.0).abs() < 0.05, "{peak}");
    }
}
