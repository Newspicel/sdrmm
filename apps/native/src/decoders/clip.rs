use anyhow::{Context, anyhow, bail};

const PCM: u16 = 1;
const FLOAT: u16 = 3;
const EXTENSIBLE: u16 = 0xFFFE;

#[derive(Clone, Debug, PartialEq)]
pub struct Wav {
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

impl Wav {
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate.max(1))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Format {
    tag: u16,
    channels: u16,
    rate: u32,
    bits: u16,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn format_of(chunk: &[u8]) -> anyhow::Result<Format> {
    let short = || anyhow!("the WAV format chunk is cut short");
    let tag = u16_at(chunk, 0).ok_or_else(short)?;
    let tag = if tag == EXTENSIBLE {
        u16_at(chunk, 24).ok_or_else(short)?
    } else {
        tag
    };
    Ok(Format {
        tag,
        channels: u16_at(chunk, 2).ok_or_else(short)?,
        rate: u32_at(chunk, 4).ok_or_else(short)?,
        bits: u16_at(chunk, 14).ok_or_else(short)?,
    })
}

fn chunks(bytes: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut at = 12;
    std::iter::from_fn(move || {
        let id = bytes.get(at..at + 4)?;
        let size = u32_at(bytes, at + 4)? as usize;
        let start = at + 8;
        let end = start.saturating_add(size).min(bytes.len());
        at = end + (size & 1);
        Some((id, bytes.get(start..end)?))
    })
}

fn sample(format: Format, raw: &[u8]) -> f32 {
    match (format.tag, raw) {
        (FLOAT, [a, b, c, d]) => f32::from_le_bytes([*a, *b, *c, *d]),
        (_, [a]) => (f32::from(*a) - 128.0) / 128.0,
        (_, [a, b]) => f32::from(i16::from_le_bytes([*a, *b])) / 32_768.0,
        (_, [a, b, c]) => (i32::from_le_bytes([0, *a, *b, *c]) >> 8) as f32 / 8_388_608.0,
        (_, [a, b, c, d]) => i32::from_le_bytes([*a, *b, *c, *d]) as f32 / 2_147_483_648.0,
        _ => 0.0,
    }
}

fn check(format: Format) -> anyhow::Result<usize> {
    let width = usize::from(format.bits / 8);
    let integer = format.tag == PCM && matches!(format.bits, 8 | 16 | 24 | 32);
    let float = format.tag == FLOAT && format.bits == 32;
    if !(integer || float) {
        bail!(
            "WAV format {} with {} bits is not supported",
            format.tag,
            format.bits
        );
    }
    if format.channels == 0 || format.rate == 0 {
        bail!("the WAV header names no channels or no rate");
    }
    Ok(width)
}

pub fn decode_wav(bytes: &[u8]) -> anyhow::Result<Wav> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        bail!("not a WAV file");
    }
    let mut format = None;
    for (id, body) in chunks(bytes) {
        match id {
            b"fmt " => format = Some(format_of(body)?),
            b"data" => {
                let format = format.context("the WAV data comes before its format")?;
                let width = check(format)?;
                return Ok(Wav {
                    rate: format.rate,
                    channels: usize::from(format.channels),
                    samples: body
                        .chunks_exact(width)
                        .map(|raw| sample(format, raw))
                        .collect(),
                });
            }
            _ => {}
        }
    }
    bail!("the WAV file holds no audio")
}

#[must_use]
pub fn to_stereo(wav: &Wav, rate: u32) -> Vec<f32> {
    let channels = wav.channels.max(1);
    let frames = wav.frames();
    if frames == 0 || rate == 0 {
        return Vec::new();
    }
    let at = |frame: usize, side: usize| wav.samples[frame * channels + side.min(channels - 1)];
    let step = f64::from(wav.rate) / f64::from(rate);
    let out_frames = ((frames as f64) / step).floor() as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    for index in 0..out_frames {
        let position = index as f64 * step;
        let left = position.floor() as usize;
        let right = (left + 1).min(frames - 1);
        let blend = (position - left as f64) as f32;
        for side in 0..2 {
            let a = at(left, side);
            let b = at(right, side);
            out.push(a + (b - a) * blend);
        }
    }
    out
}

#[must_use]
pub fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data = (samples.len() * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2 * u32::from(channels)).to_le_bytes());
        out.extend_from_slice(&(2 * channels).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"abc\0");
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    #[test]
    fn a_mono_call_decodes_past_other_chunks() {
        let decoded = decode_wav(&wav(8_000, 1, &[0, 16_384, -32_768])).unwrap();
        assert_eq!(decoded.rate, 8_000);
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.samples, vec![0.0, 0.5, -1.0]);
        assert!((decoded.seconds() - 3.0 / 8_000.0).abs() < 1e-9);
    }

    #[test]
    fn broken_files_say_why() {
        assert!(decode_wav(b"not audio").is_err());
        let mut cut = wav(8_000, 1, &[1, 2]);
        cut.truncate(20);
        assert!(decode_wav(&cut).is_err());
    }

    #[test]
    fn mono_spreads_to_both_sides_at_the_output_rate() {
        let decoded = decode_wav(&wav(24_000, 1, &[0, 16_384, 0, -16_384])).unwrap();
        let out = to_stereo(&decoded, 48_000);
        assert_eq!(out.len(), 16);
        assert_eq!(&out[0..6], &[0.0, 0.0, 0.25, 0.25, 0.5, 0.5]);
    }

    #[test]
    fn a_clock_reads_minutes_and_seconds() {
        assert_eq!(clock(0.4), "0:00");
        assert_eq!(clock(75.9), "1:15");
    }
}
