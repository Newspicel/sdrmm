use std::ops::Range;

use anyhow::{Context, anyhow, bail};

const FORMAT_PCM: u16 = 1;
const FORMAT_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xfffe;
const CHUNK_HEAD: usize = 8;
const FMT_LEN: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    Pcm16,
    Float32,
}

impl Encoding {
    const fn width(self) -> usize {
        match self {
            Self::Pcm16 => 2,
            Self::Float32 => 4,
        }
    }
}

#[derive(Debug)]
pub struct Wav {
    bytes: Vec<u8>,
    data: Range<usize>,
    encoding: Encoding,
    channels: usize,
    sample_rate: u32,
}

fn u16_at(bytes: &[u8], at: usize) -> anyhow::Result<u16> {
    bytes
        .get(at..at + 2)
        .and_then(|raw| raw.try_into().ok())
        .map(u16::from_le_bytes)
        .context("the WAV header is cut short")
}

fn u32_at(bytes: &[u8], at: usize) -> anyhow::Result<u32> {
    bytes
        .get(at..at + 4)
        .and_then(|raw| raw.try_into().ok())
        .map(u32::from_le_bytes)
        .context("the WAV header is cut short")
}

fn encoding_of(format: u16, bits: u16, bytes: &[u8], at: usize) -> anyhow::Result<Encoding> {
    let format = if format == FORMAT_EXTENSIBLE {
        u16_at(bytes, at + 24)?
    } else {
        format
    };
    match (format, bits) {
        (FORMAT_PCM, 16) => Ok(Encoding::Pcm16),
        (FORMAT_FLOAT, 32) => Ok(Encoding::Float32),
        _ => Err(anyhow!("cannot play {bits}-bit WAV format {format}")),
    }
}

struct Fmt {
    encoding: Encoding,
    channels: usize,
    sample_rate: u32,
}

fn read_fmt(bytes: &[u8], at: usize) -> anyhow::Result<Fmt> {
    let channels = usize::from(u16_at(bytes, at)?.max(1));
    let sample_rate = u32_at(bytes, at + 2)?;
    let bits = u16_at(bytes, at + 12)?;
    let encoding = encoding_of(u16_at(bytes, at - 2)?, bits, bytes, at - 2)?;
    if sample_rate == 0 {
        bail!("the WAV says it has no sample rate");
    }
    Ok(Fmt {
        encoding,
        channels,
        sample_rate,
    })
}

impl Wav {
    pub fn parse(bytes: Vec<u8>) -> anyhow::Result<Self> {
        if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
            bail!("not a WAV file");
        }
        let mut at = 12;
        let mut fmt = None;
        while at + CHUNK_HEAD <= bytes.len() {
            let id = bytes.get(at..at + 4).unwrap_or_default();
            let len = u32_at(&bytes, at + 4)? as usize;
            let body = at + CHUNK_HEAD;
            if id == b"fmt " {
                if len < FMT_LEN {
                    bail!("the WAV format chunk is too short");
                }
                fmt = Some(read_fmt(&bytes, body + 2)?);
            } else if id == b"data" {
                let fmt = fmt.context("the WAV has samples before its format")?;
                let end = body.saturating_add(len).min(bytes.len());
                return Ok(Self {
                    data: body..end,
                    encoding: fmt.encoding,
                    channels: fmt.channels,
                    sample_rate: fmt.sample_rate,
                    bytes,
                });
            }
            at = body.saturating_add(len).saturating_add(len % 2);
        }
        bail!("the WAV has no samples")
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub fn frames(&self) -> usize {
        self.data.len() / (self.channels * self.encoding.width())
    }

    #[must_use]
    pub fn duration_s(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate)
    }

    fn sample(&self, frame: usize, channel: usize) -> f32 {
        let width = self.encoding.width();
        let at = self.data.start + (frame * self.channels + channel) * width;
        let Some(raw) = self.bytes.get(at..at + width) else {
            return 0.0;
        };
        match (self.encoding, raw) {
            (Encoding::Pcm16, &[lo, hi]) => f32::from(i16::from_le_bytes([lo, hi])) / 32_768.0,
            (Encoding::Float32, &[a, b, c, d]) => f32::from_le_bytes([a, b, c, d]),
            _ => 0.0,
        }
    }

    fn stereo_at(&self, frame: usize) -> (f32, f32) {
        let left = self.sample(frame, 0);
        let right = if self.channels > 1 {
            self.sample(frame, 1)
        } else {
            left
        };
        (left, right)
    }

    pub fn render(&self, cursor: &mut f64, out_rate: u32, frames: usize, into: &mut Vec<f32>) {
        let step = f64::from(self.sample_rate) / f64::from(out_rate.max(1));
        let last = self.frames();
        for _ in 0..frames {
            if *cursor >= last as f64 {
                return;
            }
            let whole = cursor.floor() as usize;
            let frac = (*cursor - cursor.floor()) as f32;
            let (l0, r0) = self.stereo_at(whole);
            let (l1, r1) = if whole + 1 < last {
                self.stereo_at(whole + 1)
            } else {
                (l0, r0)
            };
            into.push(l0 + (l1 - l0) * frac);
            into.push(r0 + (r1 - r0) * frac);
            *cursor += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, format: u16, bits: u16, samples: &[u8]) -> Vec<u8> {
        let block = channels * bits / 8;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&format.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        out.extend_from_slice(samples);
        out
    }

    fn pcm(values: &[i16]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    #[test]
    fn reads_the_mono_pcm_the_recorder_writes() {
        let parsed =
            Wav::parse(wav(1, 48_000, FORMAT_PCM, 16, &pcm(&[0, 16_384, -32_768]))).expect("a wav");
        assert_eq!(parsed.frames(), 3);
        assert_eq!(parsed.sample_rate(), 48_000);
        let mut cursor = 0.0;
        let mut out = Vec::new();
        parsed.render(&mut cursor, 48_000, 8, &mut out);
        assert_eq!(out, [0.0, 0.0, 0.5, 0.5, -1.0, -1.0]);
        assert!((cursor - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn keeps_stereo_sides_apart_and_reads_float() {
        let samples: Vec<u8> = [0.25f32, -0.5]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let parsed = Wav::parse(wav(2, 48_000, FORMAT_FLOAT, 32, &samples)).expect("a wav");
        let mut out = Vec::new();
        parsed.render(&mut 0.0, 48_000, 1, &mut out);
        assert_eq!(out, [0.25, -0.5]);
    }

    #[test]
    fn resamples_by_stepping_between_frames() {
        let parsed = Wav::parse(wav(1, 24_000, FORMAT_PCM, 16, &pcm(&[0, 16_384]))).expect("a wav");
        assert!((parsed.duration_s() - 2.0 / 24_000.0).abs() < 1e-12);
        let mut out = Vec::new();
        parsed.render(&mut 0.0, 48_000, 4, &mut out);
        assert_eq!(out, [0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn refuses_what_it_cannot_play() {
        assert!(Wav::parse(b"nope".to_vec()).is_err());
        assert!(Wav::parse(wav(1, 48_000, FORMAT_PCM, 24, &[0; 6])).is_err());
        assert!(Wav::parse(wav(1, 0, FORMAT_PCM, 16, &[0; 2])).is_err());
        let mut headless = wav(1, 48_000, FORMAT_PCM, 16, &[0; 2]);
        headless.truncate(40);
        assert!(Wav::parse(headless).is_err());
    }
}
