//! What one `sdrmm_process` call leaves in guest memory: an [`OutputHeader`] pointing at audio,
//! events, status, pictures and symbols, all in buffers the guest keeps until its next call.

use serde::{Deserialize, Serialize};

use crate::frame::SymbolPlane;

/// Little-endian `u32` words at the pointer `sdrmm_process` returns, in field order. Floats
/// travel as their bits, the symbol rate as two words, low first.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OutputHeader {
    pub audio_ptr: u32,
    /// Interleaved `f32` samples.
    pub audio_len: u32,
    pub audio_rate: u32,
    /// A JSON array of `DecoderEvent`s, empty when none.
    pub events_ptr: u32,
    pub events_len: u32,
    /// A JSON `ChannelStatus`, empty when unchanged.
    pub status_ptr: u32,
    pub status_len: u32,
    pub rejected: u32,
    pub skipped: u32,
    /// Records [`Picture::write`] made, empty when none.
    pub pictures_ptr: u32,
    pub pictures_len: u32,
    /// `f32` symbols, interleaved I/Q on the complex plane.
    pub symbols_ptr: u32,
    pub symbols_len: u32,
    /// `f32` ideal points the symbols are measured against.
    pub reference_ptr: u32,
    pub reference_len: u32,
    /// [`plane_code`] of the symbols' plane.
    pub plane: u32,
    pub symbol_rate: f64,
    pub evm: f32,
    pub mer_db: f32,
    pub margin: f32,
    pub freq_error_hz: f32,
}

impl OutputHeader {
    pub const WORDS: usize = 22;
    pub const BYTES: usize = Self::WORDS * 4;

    #[must_use]
    pub fn to_words(self) -> [u32; Self::WORDS] {
        let rate = self.symbol_rate.to_bits();
        [
            self.audio_ptr,
            self.audio_len,
            self.audio_rate,
            self.events_ptr,
            self.events_len,
            self.status_ptr,
            self.status_len,
            self.rejected,
            self.skipped,
            self.pictures_ptr,
            self.pictures_len,
            self.symbols_ptr,
            self.symbols_len,
            self.reference_ptr,
            self.reference_len,
            self.plane,
            rate as u32,
            (rate >> 32) as u32,
            self.evm.to_bits(),
            self.mer_db.to_bits(),
            self.margin.to_bits(),
            self.freq_error_hz.to_bits(),
        ]
    }

    #[must_use]
    pub fn from_words(words: [u32; Self::WORDS]) -> Self {
        let [
            audio_ptr,
            audio_len,
            audio_rate,
            events_ptr,
            events_len,
            status_ptr,
            status_len,
            rejected,
            skipped,
            pictures_ptr,
            pictures_len,
            symbols_ptr,
            symbols_len,
            reference_ptr,
            reference_len,
            plane,
            rate_low,
            rate_high,
            evm,
            mer_db,
            margin,
            freq_error_hz,
        ] = words;
        Self {
            audio_ptr,
            audio_len,
            audio_rate,
            events_ptr,
            events_len,
            status_ptr,
            status_len,
            rejected,
            skipped,
            pictures_ptr,
            pictures_len,
            symbols_ptr,
            symbols_len,
            reference_ptr,
            reference_len,
            plane,
            symbol_rate: f64::from_bits(u64::from(rate_high) << 32 | u64::from(rate_low)),
            evm: f32::from_bits(evm),
            mer_db: f32::from_bits(mer_db),
            margin: f32::from_bits(margin),
            freq_error_hz: f32::from_bits(freq_error_hz),
        }
    }

    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::BYTES] {
        let mut bytes = [0u8; Self::BYTES];
        for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(self.to_words()) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    #[must_use]
    pub fn from_bytes(bytes: [u8; Self::BYTES]) -> Self {
        let mut words = [0u32; Self::WORDS];
        for (word, chunk) in words.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*chunk);
        }
        Self::from_words(words)
    }
}

/// The header's `plane` word: zero for no symbols.
#[must_use]
pub fn plane_code(plane: Option<SymbolPlane>) -> u32 {
    plane.map_or(0, |plane| u32::from(plane as u8) + 1)
}

#[must_use]
pub fn plane_of(code: u32) -> Option<SymbolPlane> {
    let code = u8::try_from(code.checked_sub(1)?).ok()?;
    SymbolPlane::from_u8(code)
}

/// What makes a picture a finished image rather than a video frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictureImage {
    /// The decoder that drew it.
    pub source: String,
    pub mode: String,
    pub complete: bool,
    pub lines: u16,
}

/// One record of the pictures buffer: five little-endian `u32` words (width, height, luma,
/// RGB and image lengths), the luma and RGB bytes, then the [`PictureImage`] as JSON for an
/// image or nothing for a video frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture<'a> {
    pub width: u16,
    pub height: u16,
    pub luma: &'a [u8],
    pub rgb: &'a [u8],
    pub image: Option<PictureImage>,
}

const RECORD_WORDS: usize = 5;

impl Picture<'_> {
    /// Appends the record to `out`.
    pub fn write(&self, out: &mut Vec<u8>) -> Result<(), String> {
        let meta = match &self.image {
            Some(image) => serde_json::to_vec(image).map_err(|error| error.to_string())?,
            None => Vec::new(),
        };
        let len = |bytes: &[u8]| u32::try_from(bytes.len()).map_err(|_| "picture too large");
        for word in [
            u32::from(self.width),
            u32::from(self.height),
            len(self.luma)?,
            len(self.rgb)?,
            len(&meta)?,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(self.luma);
        out.extend_from_slice(self.rgb);
        out.extend_from_slice(&meta);
        Ok(())
    }
}

/// Every record in a pictures buffer, refusing one that runs past its end.
pub fn read_pictures(mut bytes: &[u8]) -> impl Iterator<Item = Result<Picture<'_>, String>> {
    std::iter::from_fn(move || {
        if bytes.is_empty() {
            return None;
        }
        let read = read_picture(bytes);
        match read {
            Ok((picture, rest)) => {
                bytes = rest;
                Some(Ok(picture))
            }
            Err(error) => {
                bytes = &[];
                Some(Err(error))
            }
        }
    })
}

fn read_picture(bytes: &[u8]) -> Result<(Picture<'_>, &[u8]), String> {
    let (head, rest) = bytes
        .split_first_chunk::<{ RECORD_WORDS * 4 }>()
        .ok_or("picture record cut short")?;
    let mut words = [0u32; RECORD_WORDS];
    for (word, chunk) in words.iter_mut().zip(head.as_chunks::<4>().0) {
        *word = u32::from_le_bytes(*chunk);
    }
    let [width, height, luma, rgb, meta] = words;
    let side = |value: u32| u16::try_from(value).map_err(|_| format!("picture side {value}"));
    let (luma, rest) = split(rest, luma)?;
    let (rgb, rest) = split(rest, rgb)?;
    let (meta, rest) = split(rest, meta)?;
    let image = if meta.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(meta).map_err(|error| format!("picture: {error}"))?)
    };
    let picture = Picture {
        width: side(width)?,
        height: side(height)?,
        luma,
        rgb,
        image,
    };
    Ok((picture, rest))
}

fn split(bytes: &[u8], len: u32) -> Result<(&[u8], &[u8]), String> {
    bytes
        .split_at_checked(len as usize)
        .ok_or_else(|| "picture record runs past its buffer".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_survives_its_byte_form() {
        let header = OutputHeader {
            audio_ptr: 1,
            audio_len: 2,
            audio_rate: 48_000,
            events_ptr: 4,
            events_len: 5,
            status_ptr: 6,
            status_len: 7,
            rejected: 8,
            skipped: u32::MAX,
            pictures_ptr: 9,
            pictures_len: 10,
            symbols_ptr: 11,
            symbols_len: 12,
            reference_ptr: 13,
            reference_len: 14,
            plane: plane_code(Some(SymbolPlane::Level)),
            symbol_rate: 27_500_000.25,
            evm: 0.125,
            mer_db: 21.5,
            margin: -3.0,
            freq_error_hz: 12.5,
        };
        assert_eq!(OutputHeader::from_bytes(header.to_bytes()), header);
    }

    #[test]
    fn a_plane_reads_back_from_its_code() {
        for plane in [None, Some(SymbolPlane::Complex), Some(SymbolPlane::Level)] {
            assert_eq!(plane_of(plane_code(plane)), plane);
        }
        assert_eq!(plane_of(9), None);
    }

    #[test]
    fn frames_and_images_read_back_in_order() {
        let frame = Picture {
            width: 2,
            height: 1,
            luma: &[1, 2],
            rgb: &[],
            image: None,
        };
        let image = Picture {
            width: 1,
            height: 1,
            luma: &[9],
            rgb: &[1, 2, 3],
            image: Some(PictureImage {
                source: "probe".to_owned(),
                mode: "Test".to_owned(),
                complete: true,
                lines: 1,
            }),
        };
        let mut buffer = Vec::new();
        frame.write(&mut buffer).expect("frame");
        image.write(&mut buffer).expect("image");
        let read: Vec<_> = read_pictures(&buffer)
            .collect::<Result<_, _>>()
            .expect("reads");
        assert_eq!(read, [frame, image]);
    }

    #[test]
    fn a_record_past_its_buffer_is_refused_once() {
        let mut buffer = Vec::new();
        Picture {
            width: 4,
            height: 4,
            luma: &[0; 16],
            rgb: &[],
            image: None,
        }
        .write(&mut buffer)
        .expect("frame");
        buffer.truncate(buffer.len() - 1);
        let read: Vec<_> = read_pictures(&buffer).collect();
        assert_eq!(read.len(), 1);
        assert!(read[0].is_err());
    }
}
