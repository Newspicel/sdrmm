use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::FracResampler;

pub struct Signal {
    pub id: &'static str,
    pub label: &'static str,
    pub rate_hz: f64,
    pub render: fn() -> Vec<Complex<f32>>,
}

pub struct Catalog {
    pub signals: &'static [Signal],
    pub default_id: &'static str,
    pub center_hz: f64,
}

pub static EMPTY_CATALOG: Catalog = Catalog {
    signals: &[],
    default_id: "",
    center_hz: 0.0,
};

impl Catalog {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.signals.is_empty()
    }

    #[must_use]
    pub fn find(&self, id: &str) -> Option<&'static Signal> {
        self.signals.iter().find(|signal| signal.id == id)
    }

    #[must_use]
    pub fn default_signal(&self) -> Option<&'static Signal> {
        self.find(self.default_id).or_else(|| self.signals.first())
    }

    #[must_use]
    pub fn rates(&self) -> Vec<f64> {
        let mut rates: Vec<f64> = self.signals.iter().map(|signal| signal.rate_hz).collect();
        rates.sort_by(f64::total_cmp);
        rates.dedup();
        rates
    }
}

pub fn shift(iq: &mut [Complex<f32>], freq_hz: f64, rate: f64) {
    let step = TAU * freq_hz / rate;
    for (k, s) in iq.iter_mut().enumerate() {
        let phase = (step * k as f64).rem_euclid(TAU) as f32;
        *s *= Complex::from_polar(1.0, phase);
    }
}

pub fn add_noise(iq: &mut [Complex<f32>], seed: u32, amp: f32) {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
    };
    for s in iq {
        *s += Complex::new(next(), next());
    }
}

pub fn scale(iq: &mut [Complex<f32>], gain: f32) {
    for s in iq {
        *s *= gain;
    }
}

#[must_use]
pub fn silence(len: usize) -> Vec<Complex<f32>> {
    vec![Complex::new(0.0, 0.0); len]
}

#[must_use]
pub fn resample(iq: &[Complex<f32>], from: f64, to: f64) -> Vec<Complex<f32>> {
    let ratio = to / from;
    let mut resampler = FracResampler::new(ratio);
    let mut out = Vec::with_capacity((iq.len() as f64 * ratio) as usize + 1);
    resampler.process(iq, &mut out);
    out
}

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
    let step = TAU * tone_hz / rate;
    key.iter()
        .enumerate()
        .map(|(k, &env)| Complex::from_polar(env, (step * k as f64).rem_euclid(TAU) as f32))
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
    fn shift_moves_a_dc_carrier_to_the_requested_offset() {
        let mut iq = vec![Complex::new(1.0f32, 0.0); 4_800];
        shift(&mut iq, 1_000.0, 48_000.0);
        let turn = iq[48] / iq[0];
        assert!((turn.re - 1.0).abs() < 1e-3, "{turn}");
        assert!(turn.im.abs() < 1e-3, "{turn}");
    }

    #[test]
    fn resample_stretches_the_block_by_the_rate_ratio() {
        let iq = ook(&vec![1.0f32; 4_800], 1_000.0, 48_000.0);
        let out = resample(&iq, 48_000.0, 96_000.0);
        assert!(out.len().abs_diff(2 * iq.len()) < 64, "{}", out.len());
    }

    #[test]
    fn noise_and_scale_move_the_power_the_way_they_are_named() {
        let power = |iq: &[Complex<f32>]| {
            iq.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>() / iq.len() as f64
        };
        let mut quiet = silence(4_096);
        add_noise(&mut quiet, 0x5DEE_CE66, 0.1);
        let mut loud = silence(4_096);
        add_noise(&mut loud, 0x5DEE_CE66, 1.0);
        assert!(power(&loud) > 50.0 * power(&quiet));
        let mut halved = loud.clone();
        scale(&mut halved, 0.5);
        assert!((power(&halved) / power(&loud) - 0.25).abs() < 1e-6);
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
