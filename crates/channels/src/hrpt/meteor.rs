//! Meteor-M HRPT: 1024-byte CADUs behind the CCSDS sync marker, no coding and no
//! randomisation, with the MSU-MR imager's scan lines spread over fixed spans of each one.

use sdrmm_modem::{
    ccsds::ASM,
    framesync::{FixedFramer, Framed},
};

use super::image::{PLANES, Scan};

pub const CADU_BYTES: usize = 1_024;
pub const LINE_SYNC: u64 = 0x0218_A7A3_92DD_9ABF;
pub const LINE_BYTES: usize = 11_850;
/// The byte spans of each CADU that carry the MSU-MR stream, in order.
pub const LINE_SPANS: [(usize, usize); 4] = [(22, 238), (278, 238), (534, 238), (790, 234)];
pub const WIDTH: usize = 1_572;
#[cfg(test)]
pub const CHANNELS: usize = 6;
pub const TIME_AT: usize = 8;
const PIXELS_AT: usize = 50;
const GROUP_STRIDE: usize = 30;
const GROUP_BYTES: usize = 5;
const CADU_SEARCH_ERRORS: u32 = 0;
const CADU_TRACK_ERRORS: u32 = 6;
const CADU_MISSES: u8 = 2;
const LINE_SEARCH_ERRORS: u32 = 6;
const LINE_TRACK_ERRORS: u32 = 12;
const LINE_MISSES: u8 = 1;
/// MSU-MR stamps its lines in Moscow time, to 1/255 s.
const MOSCOW_HOURS: u8 = 3;
const TICKS_PER_SECOND: u32 = 255;
/// One line every 153 ms.
pub const LINE_MS: f64 = 153.0;
/// The channels the picture planes take: 1, 2 and 5.
const KEPT: [usize; PLANES] = [0, 1, 4];

/// What one bit completed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pushed {
    pub cadu: Option<Framed>,
    pub line: bool,
}

pub struct Link {
    cadus: FixedFramer,
    lines: FixedFramer,
    line: Box<[u8; LINE_BYTES]>,
    lost_lines: u32,
}

impl Link {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cadus: FixedFramer::new(
                u64::from(ASM),
                32,
                CADU_BYTES * 8,
                CADU_SEARCH_ERRORS,
                CADU_TRACK_ERRORS,
                CADU_MISSES,
            ),
            lines: FixedFramer::new(
                LINE_SYNC,
                64,
                LINE_BYTES * 8,
                LINE_SEARCH_ERRORS,
                LINE_TRACK_ERRORS,
                LINE_MISSES,
            ),
            line: Box::new([0; LINE_BYTES]),
            lost_lines: 0,
        }
    }

    /// CADUs and lines read but never handed on.
    #[must_use]
    pub fn dropped(&self) -> u32 {
        self.cadus
            .dropped()
            .saturating_add(self.lines.dropped())
            .saturating_add(self.lost_lines)
    }

    pub fn reset(&mut self) {
        self.cadus.reset();
        self.lines.reset();
    }

    #[cfg(test)]
    #[must_use]
    pub fn cadu(&self) -> &[u8] {
        self.cadus.frame()
    }

    /// The last complete MSU-MR line, sync word first.
    #[must_use]
    pub fn line(&self) -> &[u8] {
        self.line.as_slice()
    }

    /// A CADU kept on position alone may hold anything, so the line it would continue is
    /// dropped and the next is searched for afresh.
    pub fn push(&mut self, bit: bool) -> Pushed {
        let Some(framed) = self.cadus.push(bit) else {
            return Pushed::default();
        };
        if framed.flywheel {
            self.lines.reset();
            return Pushed {
                cadu: Some(framed),
                line: false,
            };
        }
        let mut line = false;
        let cadu = self.cadus.frame();
        for &(start, len) in &LINE_SPANS {
            for &byte in &cadu[start..start + len] {
                for shift in (0..8).rev() {
                    let Some(done) = self.lines.push(byte >> shift & 1 == 1) else {
                        continue;
                    };
                    if done.flywheel {
                        self.lost_lines = self.lost_lines.saturating_add(1);
                    } else {
                        self.line.copy_from_slice(self.lines.frame());
                        line = true;
                    }
                }
            }
        }
        Pushed {
            cadu: Some(framed),
            line,
        }
    }
}

/// Milliseconds of the UTC day the line was scanned, `None` when its stamp is no time of day.
#[must_use]
pub fn time_ms(line: &[u8]) -> Option<u32> {
    let [hour, minute, second, fraction] = *line.get(TIME_AT..TIME_AT + 4)? else {
        return None;
    };
    if hour >= 24 || minute >= 60 || second >= 60 {
        return None;
    }
    let hour = u32::from((hour + 24 - MOSCOW_HOURS) % 24);
    let seconds = (hour * 60 + u32::from(minute)) * 60 + u32::from(second);
    Some(seconds * 1_000 + u32::from(fraction) * 1_000 / TICKS_PER_SECOND)
}

/// Channels 1, 2 and 5 of `line` into the picture's planes.
pub fn scan(line: &[u8], out: &mut Scan) {
    for (plane, &channel) in out.planes.iter_mut().zip(&KEPT) {
        let (row, _) = plane.split_at_mut(WIDTH);
        if let Ok(row) = <&mut [u16; WIDTH]>::try_from(row) {
            unpack(line, channel, row);
        }
    }
    out.time_ms = time_ms(line);
}

/// Channel `channel` (0 to 5) of `line` at full 10-bit depth into `out`.
pub fn unpack(line: &[u8], channel: usize, out: &mut [u16; WIDTH]) {
    for (group, pixels) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let at = PIXELS_AT + group * GROUP_STRIDE + channel * GROUP_BYTES;
        let Some([a, b, c, d, e]) = line
            .get(at..at + GROUP_BYTES)
            .and_then(|span| <[u8; GROUP_BYTES]>::try_from(span).ok())
        else {
            pixels.fill(0);
            continue;
        };
        let [a, b, c, d, e] = [a, b, c, d, e].map(u16::from);
        *pixels = [
            a << 2 | b >> 6,
            (b & 0x3F) << 4 | c >> 4,
            (c & 0x0F) << 6 | d >> 2,
            (d & 0x03) << 8 | e,
        ];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_bytes_hold_four_ten_bit_pixels() {
        let mut line = vec![0u8; LINE_BYTES];
        let at = PIXELS_AT + GROUP_STRIDE + 2 * GROUP_BYTES;
        line[at..at + 5].copy_from_slice(&[
            0b1111_1111,
            0b1100_0000,
            0b0001_0000,
            0b0000_0011,
            0xFF,
        ]);
        let mut out = [0; WIDTH];
        unpack(&line, 2, &mut out);
        assert_eq!(out[4..8], [1023, 1, 0, 1023]);
        assert_eq!(out[..4], [0; 4]);
    }

    #[test]
    fn the_line_clock_reads_moscow_time_as_utc() {
        let mut line = vec![0u8; 16];
        line[TIME_AT..TIME_AT + 4].copy_from_slice(&[8, 22, 25, 51]);
        assert_eq!(
            time_ms(&line),
            Some((5 * 3600 + 22 * 60 + 25) * 1_000 + 200)
        );
        line[TIME_AT] = 24;
        assert_eq!(time_ms(&line), None);
    }
}
