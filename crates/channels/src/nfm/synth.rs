use num_complex::Complex;
use sdrmm_modem::synth::{NARROW_HZ, Render, Signal, fm_modulate, seconds};

pub use sdrmm_modem::synth::speech_audio;

use sdrmm_dsp::golay23_encode;

const DCS_BAUD: f64 = 134.4;
const DCS_WORD_BITS: u32 = 23;
const DCS_SIGNATURE: u16 = 0b100;

#[must_use]
pub fn ctcss_audio(hz: f64, deviation: f32, rate: f64, len: usize) -> Vec<f32> {
    sdrmm_modem::synth::tone_audio(hz, deviation, rate, len)
}

#[must_use]
pub fn dcs_audio(code: u16, deviation: f32, rate: f64, len: usize) -> Vec<f32> {
    let digits = u32::from(code);
    let raw = (digits / 100 % 10) << 6 | (digits / 10 % 10) << 3 | (digits % 10);
    let word = golay23_encode((u32::from(DCS_SIGNATURE) << 9 | raw) as u16);
    let samples_per_bit = rate / DCS_BAUD;
    (0..len)
        .map(|k| {
            let bit = (k as f64 / samples_per_bit) as u32 % DCS_WORD_BITS;
            if word >> bit & 1 == 1 {
                deviation
            } else {
                -deviation
            }
        })
        .collect()
}

#[must_use]
pub fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[
    Signal {
        id: "nfm_ctcss",
        label: "NFM voice · CTCSS 88.5",
        rate_hz: NARROW_HZ,
        render: Render::Always(nfm_ctcss),
    },
    Signal {
        id: "nfm_voice",
        label: "NFM voice",
        rate_hz: NARROW_HZ,
        render: Render::Always(nfm_voice),
    },
    Signal {
        id: "nfm_dcs",
        label: "NFM voice · DCS 023",
        rate_hz: NARROW_HZ,
        render: Render::Always(nfm_dcs),
    },
];

const VOICE_SECONDS: f64 = 3.0;

fn nfm_voice() -> Vec<Complex<f32>> {
    let audio = speech_audio(NARROW_HZ, seconds(NARROW_HZ, VOICE_SECONDS));
    fm_modulate(&audio, 2_500.0, NARROW_HZ)
}

fn nfm_ctcss() -> Vec<Complex<f32>> {
    let len = seconds(NARROW_HZ, VOICE_SECONDS);
    let audio = mix(
        &ctcss_audio(88.5, 0.15, NARROW_HZ, len),
        &speech_audio(NARROW_HZ, len),
    );
    fm_modulate(&audio, 2_500.0, NARROW_HZ)
}

fn nfm_dcs() -> Vec<Complex<f32>> {
    let len = seconds(NARROW_HZ, VOICE_SECONDS);
    let audio = mix(
        &dcs_audio(23, 0.15, NARROW_HZ, len),
        &speech_audio(NARROW_HZ, len),
    );
    fm_modulate(&audio, 2_500.0, NARROW_HZ)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dcs_waveform_is_the_published_word_sent_backwards() {
        let rate = DCS_BAUD;
        let audio = dcs_audio(23, 1.0, rate, DCS_WORD_BITS as usize);
        let sent: String = audio
            .iter()
            .map(|&s| if s > 0.0 { '1' } else { '0' })
            .collect();
        let word: String = "10000001001111101100011".chars().rev().collect();
        assert_eq!(sent, word);
    }

    #[test]
    fn the_word_repeats_for_as_long_as_it_is_asked_to() {
        let rate = DCS_BAUD * 4.0;
        let bits = DCS_WORD_BITS as usize * 3;
        let audio = dcs_audio(754, 0.2, rate, bits * 4);
        assert_eq!(audio.len(), bits * 4);
        for (k, &s) in audio.iter().enumerate() {
            assert_eq!(s.abs(), 0.2, "sample {k} is not a keyed level");
            let period = DCS_WORD_BITS as usize * 4;
            if k >= period {
                assert_eq!(s, audio[k - period], "word did not repeat at {k}");
            }
        }
    }
}
