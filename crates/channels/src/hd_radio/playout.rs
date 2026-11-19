//! Hands HDC packets to the media worker's FDK decoder as standard AAC.

use sdrmm_codec::aac::hdc;
use sdrmm_media::{Codec, Media};

/// The name HDC errors carry in the status.
const HDC: &str = "HDC";
/// Output frames one HDC frame fills at the audio rate.
pub const PACKET_FRAMES: usize =
    sdrmm_decoder::AUDIO_RATE as usize * hdc::FRAME_SAMPLES / hdc::OUTPUT_RATE_HZ as usize;

#[derive(Default)]
pub struct Playout {
    codec: Option<(bool, Codec)>,
    raw: Vec<u8>,
}

impl Playout {
    pub fn reset(&mut self) {
        self.codec = None;
    }

    fn codec(&mut self, stereo: bool) -> Codec {
        match &self.codec {
            Some((layout, codec)) if *layout == stereo => codec.clone(),
            _ => {
                let codec = Codec::fdk(&hdc::audio_specific_config(stereo), Some(HDC));
                self.codec = Some((stereo, codec.clone()));
                codec
            }
        }
    }

    /// Queues one packet, or counts it lost.
    pub fn packet(&mut self, packet: &[u8], media: &mut Media) {
        let stereo = match hdc::stereo(packet) {
            Ok(stereo) => stereo,
            Err(reason) => return media.audio_gap(PACKET_FRAMES, 1, reason),
        };
        let codec = self.codec(stereo);
        match hdc::raw_frame(packet, &mut self.raw) {
            Ok(()) => media.push(&codec, &self.raw, None),
            Err(reason) => media.audio_gap(PACKET_FRAMES, 1, reason),
        }
    }
}
