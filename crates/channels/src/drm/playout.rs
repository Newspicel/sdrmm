//! Frames DRM audio the way the media worker's FDK decoder reads it.

use sdrmm_codec::aac::{AudioConfig, Coding, aac_specific_config, raw_frame, usac_config};
use sdrmm_media::{Codec, Media};

/// The name xHE-AAC errors carry, so the status tells them from AAC ones.
const XHE: &str = "xHE-AAC";

/// The codec the current audio config decodes under, kept while the config holds.
#[derive(Default)]
pub struct Playout {
    codec: Option<(AudioConfig, Result<Codec, String>)>,
}

impl Playout {
    fn codec(&mut self, config: &AudioConfig) -> Result<Codec, String> {
        let (_, codec) = match &mut self.codec {
            Some(entry) if entry.0 == *config => entry,
            slot => slot.insert((*config, codec(config))),
        };
        codec.clone()
    }

    /// Hands one audio frame to `media`, or counts it lost there.
    pub fn frame(&mut self, frame: &[u8], config: &AudioConfig, media: &mut Media) {
        let codec = match self.codec(config) {
            Ok(codec) => codec,
            Err(reason) => return media.audio_gap(0, 1, &reason),
        };
        match config.coding {
            Coding::Aac => match raw_frame(frame, config) {
                Ok(raw) => media.push(&codec, &raw, None),
                Err(reason) => media.audio_gap(0, 1, &reason),
            },
            Coding::Xhe => media.push(&codec, frame, None),
        }
    }
}

fn codec(config: &AudioConfig) -> Result<Codec, String> {
    match config.coding {
        Coding::Aac => aac_specific_config(config)
            .map(|asc| Codec::fdk(&asc, None))
            .map_err(str::to_owned),
        Coding::Xhe => usac_config(config)
            .map(|asc| Codec::fdk(&asc, Some(XHE)))
            .map_err(|reason| format!("{XHE}: {reason}")),
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_codec::aac::{AudioMode, MAX_CONFIG};
    use sdrmm_decoder::ChannelOutputs;
    use sdrmm_media::length_prefixed;

    use super::*;

    fn config(coding: Coding, rate_hz: u32, rate_code: u8) -> AudioConfig {
        AudioConfig {
            coding,
            sbr: false,
            mode: AudioMode::Mono,
            rate_hz,
            rate_code,
            text: false,
            surround: 0,
            config: [0; MAX_CONFIG],
            config_length: 0,
        }
    }

    fn settle(media: &mut Media, done: impl Fn(&Media) -> bool) {
        let mut out = ChannelOutputs::default();
        for _ in 0..2000 {
            media.drain(&mut out);
            if done(media) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn a_frame_failing_its_crc_is_counted_lost_and_the_rest_play() {
        let mut media = Media::new().expect("worker");
        let mut playout = Playout::default();
        let frames = length_prefixed(include_bytes!(
            "../../../../fixtures/drm/drm_lc_mono_48k.drm"
        ));
        let config = config(Coding::Aac, 48_000, 0);
        let mut damaged = frames[5].to_vec();
        damaged[2] ^= 0x01;
        for frame in &frames[..5] {
            playout.frame(frame, &config, &mut media);
        }
        playout.frame(&damaged, &config, &mut media);
        settle(&mut media, |media| {
            media.audio_errors > 0 && media.audio_frames >= 4
        });
        assert!(media.audio_frames >= 4);
        assert_eq!(media.audio_errors, 1);
        assert_eq!(
            media.audio_error.as_deref(),
            Some("DRM AAC frame CRC failure")
        );
    }

    #[test]
    fn xhe_aac_reports_its_codec_on_failure() {
        let mut media = Media::new().expect("worker");
        Playout::default().frame(
            &[0x80, 0x11, 0x22, 0x33],
            &config(Coding::Xhe, 24_000, 4),
            &mut media,
        );
        settle(&mut media, |media| media.audio_errors > 0);
        let error = media.audio_error.expect("an error");
        assert!(error.starts_with("xHE-AAC: "), "{error}");
    }

    #[test]
    fn xhe_aac_frames_at_an_odd_rate_decode_to_audio() {
        let mut media = Media::new().expect("worker");
        let mut playout = Playout::default();
        let config = AudioConfig {
            config_length: 1,
            ..config(Coding::Xhe, 38_400, 6)
        };
        for _ in 0..8 {
            playout.frame(&[0x8C, 0x80, 0x00], &config, &mut media);
        }
        settle(&mut media, |media| {
            media.audio_frames >= 4 || media.audio_errors > 0
        });
        assert_eq!(media.audio_error, None);
        assert!(media.audio_frames >= 4, "{}", media.audio_frames);
    }
}
