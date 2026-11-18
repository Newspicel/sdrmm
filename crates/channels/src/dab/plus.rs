use super::superframe::AudioFormat;

struct Bits {
    bytes: Vec<u8>,
    used: usize,
}

impl Bits {
    fn push(&mut self, value: usize, width: usize) {
        for bit in (0..width).rev() {
            if self.used.is_multiple_of(8) {
                self.bytes.push(0);
            }
            self.bytes[self.used / 8] |= (((value >> bit) & 1) as u8) << (7 - self.used % 8);
            self.used += 1;
        }
    }
}

pub fn audio_specific_config(format: AudioFormat) -> Result<Vec<u8>, &'static str> {
    if format.surround != 0 || !matches!(format.sample_rate_hz, 32000 | 48000) {
        return Err("Unsupported DAB+ surround or sample rate");
    }
    let mut config = Bits {
        bytes: Vec::with_capacity(4),
        used: 0,
    };
    let extension = format.spectral_band_replication;
    config.push(
        if extension {
            if format.parametric_stereo { 29 } else { 5 }
        } else {
            2
        },
        5,
    );
    let index = |rate| match rate {
        16000 => 8,
        24000 => 6,
        32000 => 5,
        48000 => 3,
        _ => 15,
    };
    config.push(index(format.core_rate_hz()), 4);
    config.push(if format.stereo_core { 2 } else { 1 }, 4);
    if extension {
        config.push(index(format.output_rate_hz()), 4);
        config.push(2, 5);
    }
    config.push(1, 1);
    config.push(0, 2);
    Ok(config.bytes)
}

/// Strips leading PAD data elements and the zero fill after the block: a raw block ends in the
/// nonzero bits of `ID_END`, and FDK rejects bytes past it.
pub fn access_unit(mut unit: &[u8]) -> Result<&[u8], &'static str> {
    if unit.len() > 8000 {
        return Err("Unsupported DAB+ access-unit size");
    }
    while unit.first().is_some_and(|byte| byte >> 5 == 4) {
        let count = usize::from(*unit.get(1).ok_or("Truncated DAB+ data element")?);
        let offset = if count == 255 {
            3 + count + usize::from(*unit.get(2).ok_or("Truncated DAB+ data element")?)
        } else {
            2 + count
        };
        unit = unit.get(offset..).ok_or("Truncated DAB+ data element")?;
    }
    let end = unit
        .iter()
        .rposition(|&byte| byte != 0)
        .map_or(0, |last| last + 1);
    Ok(&unit[..end])
}

#[cfg(test)]
mod tests {
    use sdrmm_decoder::{AUDIO_RATE, ChannelOutputs};
    use sdrmm_media::{Codec, Media, decode_audio, length_prefixed};

    use super::*;

    #[test]
    fn the_config_names_the_core_its_extension_and_the_960_sample_frame() {
        let format = AudioFormat {
            sample_rate_hz: 48_000,
            spectral_band_replication: false,
            parametric_stereo: false,
            stereo_core: false,
            surround: 0,
        };
        assert_eq!(audio_specific_config(format), Ok(vec![0x11, 0x8C]));
        assert!(
            audio_specific_config(AudioFormat {
                surround: 1,
                ..format
            })
            .is_err()
        );
    }

    #[test]
    fn an_access_unit_drops_its_pad_element_and_trailing_fill() {
        let unit = [0x80, 2, 0xAA, 0xBB, 0x21, 0x1B, 0x0E, 0, 0, 0];
        assert_eq!(access_unit(&unit), Ok([0x21, 0x1B, 0x0E].as_slice()));
    }

    /// What the media worker plays of DAB+ `units` in `format`, framed as the channel frames them.
    fn played(units: &[&[u8]], format: AudioFormat) -> Vec<f32> {
        let mut media = Media::new().expect("worker");
        let codec = Codec::fdk(&audio_specific_config(format).expect("config"), None);
        for unit in units {
            media.push(&codec, access_unit(unit).expect("unit"), None);
        }
        let mut out = ChannelOutputs::default();
        for _ in 0..2000 {
            media.drain(&mut out);
            if media.audio_frames >= units.len() as u32 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            media.audio_frames,
            units.len() as u32,
            "{:?}",
            media.audio_error
        );
        out.audio_pcm
    }

    #[test]
    fn dab_lc_he_aac_and_parametric_stereo_decode_960_sample_access_units() {
        for (rate, sbr, ps, stereo, source, reference) in [
            (
                48000,
                false,
                false,
                false,
                include_bytes!("../../../../fixtures/broadcast_audio/dab_lc_mono_48k.aus")
                    .as_slice(),
                include_bytes!("../../../../fixtures/broadcast_audio/dab_lc_mono_48k.pcm")
                    .as_slice(),
            ),
            (
                48000,
                true,
                false,
                true,
                include_bytes!("../../../../fixtures/broadcast_audio/dab_he_stereo_48k.aus")
                    .as_slice(),
                include_bytes!("../../../../fixtures/broadcast_audio/dab_he_stereo_48k.pcm")
                    .as_slice(),
            ),
            (
                32000,
                true,
                true,
                false,
                include_bytes!("../../../../fixtures/broadcast_audio/dab_he_ps_32k.aus").as_slice(),
                include_bytes!("../../../../fixtures/broadcast_audio/dab_he_ps_32k_48k.pcm")
                    .as_slice(),
            ),
        ] {
            let format = AudioFormat {
                sample_rate_hz: rate,
                spectral_band_replication: sbr,
                parametric_stereo: ps,
                stereo_core: stereo,
                surround: 0,
            };
            let codec = Codec::fdk(&audio_specific_config(format).expect("config"), None);
            let decoded = decode_audio(
                &codec,
                length_prefixed(source)
                    .into_iter()
                    .map(|unit| access_unit(unit).expect("unit")),
            )
            .expect("decode");
            assert!((30..=31).contains(&decoded.len()), "{format:?}");
            let pcm = decoded.concat();
            let expected: Vec<f32> = reference
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            assert!(
                pcm.len().abs_diff(expected.len()) < 100,
                "{format:?}: {} != {}",
                pcm.len(),
                expected.len()
            );
            let best = (-128_i32..=128)
                .flat_map(|lag| [-1.0_f32, 1.0].map(move |sign| (lag, sign)))
                .map(|(lag, sign)| {
                    let mut err = 0.0;
                    let mut pow = 0.0;
                    let gain = if stereo || ps {
                        1.0
                    } else {
                        std::f32::consts::FRAC_1_SQRT_2
                    };
                    for i in 12000..pcm.len().min(expected.len()) - 4096 {
                        let actual = pcm[(i as i32 + 2 * lag) as usize];
                        let target = expected[i] * gain * sign;
                        err += f64::from(actual - target).powi(2);
                        pow += f64::from(target).powi(2);
                    }
                    (err / pow, lag, sign)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .expect("alignment");

            assert!(
                best.0 < if sbr { 0.08 } else { 0.0001 },
                "{format:?}: relative error {best:?}"
            );
        }
    }

    fn tone_residual_db(pcm: &[f32], channel: usize, frequency: f64) -> f64 {
        let samples: Vec<f64> = pcm
            .iter()
            .skip(channel)
            .step_by(2)
            .map(|&v| v.into())
            .collect();
        let (mut residual, mut power) = (0.0, 0.0);
        for window in samples[9600..samples.len() - 4800].as_chunks::<960>().0 {
            let basis: Vec<(f64, f64)> = (0..window.len())
                .map(|i| {
                    (std::f64::consts::TAU * frequency * i as f64 / f64::from(AUDIO_RATE)).sin_cos()
                })
                .collect();
            let (mut ss, mut sc, mut cc, mut xs, mut xc) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for (&x, &(s, c)) in window.iter().zip(&basis) {
                (ss, sc, cc, xs, xc) = (ss + s * s, sc + s * c, cc + c * c, xs + x * s, xc + x * c);
            }
            let det = ss * cc - sc * sc;
            let (a, b) = ((xs * cc - xc * sc) / det, (xc * ss - xs * sc) / det);
            for (&x, &(s, c)) in window.iter().zip(&basis) {
                let fit = a * s + b * c;
                residual += (x - fit).powi(2);
                power += fit.powi(2);
            }
        }
        10.0 * (residual / power).log10()
    }

    #[test]
    fn parametric_stereo_on_960_sample_frames_keeps_both_tones_clean() {
        let format = AudioFormat {
            sample_rate_hz: 32_000,
            spectral_band_replication: true,
            parametric_stereo: true,
            stereo_core: false,
            surround: 0,
        };
        let units = length_prefixed(include_bytes!(
            "../../../../fixtures/broadcast_audio/dab_he_ps_32k.aus"
        ));
        let pcm = played(&units, format);
        for (channel, frequency) in [(0, 700.0), (1, 1300.0)] {
            let residual = tone_residual_db(&pcm, channel, frequency);
            assert!(residual < -40.0, "channel {channel}: {residual:.1} dB");
        }
    }
}
