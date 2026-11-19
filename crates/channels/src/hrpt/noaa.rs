//! NOAA POES HRPT: minor frames of 11090 ten-bit words, six a second, each carrying one AVHRR
//! scan of five channels at 2048 pixels.

use sdrmm_modem::framesync::{FixedFramer, Framed};

use super::{
    image::{PLANES, Scan},
    meteor::Pushed,
};

/// The six frame sync words 644, 367, 860, 413, 527 and 149.
pub const FRAME_SYNC: u64 = 0x0A11_6FD7_19D8_3C95;
pub const SYNC_BITS: u32 = 60;
pub const WORD_BITS: usize = 10;
pub const FRAME_WORDS: usize = 11_090;
pub const WIDTH: usize = 2_048;
pub const CHANNELS: usize = 5;
pub const LINE_MS: f64 = 1_000.0 / 6.0;
const AVHRR_AT: usize = 750;
/// The channels the picture planes take: 1, 2 and 4.
const KEPT: [usize; PLANES] = [0, 1, 3];
const TIME_AT: usize = 8;
const SEARCH_ERRORS: u32 = 4;
const TRACK_ERRORS: u32 = 12;
const MISSES: u8 = 2;

pub struct Link {
    frames: FixedFramer,
}

impl Link {
    #[must_use]
    pub fn new() -> Self {
        Self {
            frames: FixedFramer::new(
                FRAME_SYNC,
                SYNC_BITS,
                FRAME_WORDS * WORD_BITS,
                SEARCH_ERRORS,
                TRACK_ERRORS,
                MISSES,
            ),
        }
    }

    pub fn reset(&mut self) {
        self.frames.reset();
    }

    /// Minor frames read but never handed on.
    #[must_use]
    pub const fn frames_dropped(&self) -> u32 {
        self.frames.dropped()
    }

    /// The last complete minor frame, packed MSB first, until the next bit.
    #[must_use]
    pub fn frame(&self) -> &[u8] {
        self.frames.frame()
    }

    /// A frame kept on position alone may hold anything, so it draws no line.
    pub fn push(&mut self, bit: bool) -> Pushed {
        let cadu: Option<Framed> = self.frames.push(bit);
        Pushed {
            line: cadu.is_some_and(|framed| !framed.flywheel),
            cadu,
        }
    }
}

/// Word `index` of a packed minor frame.
#[must_use]
pub fn word(frame: &[u8], index: usize) -> u16 {
    let bit = index * WORD_BITS;
    let (byte, shift) = (bit / 8, bit % 8);
    let high = u32::from(frame.get(byte).copied().unwrap_or(0));
    let low = u32::from(frame.get(byte + 1).copied().unwrap_or(0));
    ((high << 8 | low) >> (6 - shift) & 0x3FF) as u16
}

/// Channel `channel` (0 to 4) of the frame's AVHRR scan into `out`.
pub fn unpack(frame: &[u8], channel: usize, out: &mut [u16; WIDTH]) {
    for (pixel, value) in out.iter_mut().enumerate() {
        *value = word(frame, AVHRR_AT + pixel * CHANNELS + channel);
    }
}

/// AVHRR channels 1, 2 and 4 of `frame` into the picture's planes.
pub fn scan(frame: &[u8], out: &mut Scan) {
    for (plane, &channel) in out.planes.iter_mut().zip(&KEPT) {
        if let Ok(row) = <&mut [u16; WIDTH]>::try_from(&mut plane[..]) {
            unpack(frame, channel, row);
        }
    }
    out.time_ms = time_ms(frame);
}

/// Milliseconds of the UTC day from the frame's time code, `None` when it is no time of day.
#[must_use]
pub fn time_ms(frame: &[u8]) -> Option<u32> {
    let [_, a, b, c] = std::array::from_fn(|k| u32::from(word(frame, TIME_AT + k)));
    let ms = (a & 0x7F) << 20 | b << 10 | c;
    (ms < 86_400_000).then_some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed(words: &[u16]) -> Vec<u8> {
        let bits: Vec<bool> = words
            .iter()
            .flat_map(|&word| {
                (0..WORD_BITS)
                    .rev()
                    .map(move |shift| word >> shift & 1 == 1)
            })
            .collect();
        bits.chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0u8, |byte, (k, &bit)| byte | u8::from(bit) << (7 - k))
            })
            .collect()
    }

    #[test]
    fn ten_bit_words_unpack_across_bytes() {
        let frame = packed(&[644, 367, 860, 413, 527, 149, 1023, 0, 1]);
        let words: Vec<u16> = (0..9).map(|k| word(&frame, k)).collect();
        assert_eq!(words, [644, 367, 860, 413, 527, 149, 1023, 0, 1]);
    }

    #[test]
    fn the_time_code_reads_milliseconds_of_the_day() {
        let mut words = vec![0u16; 12];
        words[8..12].copy_from_slice(&[622, 704, 889, 563]);
        assert_eq!(time_ms(&packed(&words)), Some(68_019_763));
    }
}
