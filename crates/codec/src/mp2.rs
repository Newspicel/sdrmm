//! MPEG-1/2 Layer II audio (MP2), as DAB and DVB carry it, decoded in pure Rust to 48 kHz
//! interleaved stereo.

use num_complex::Complex;
use sdrmm_dsp::FracResampler;
use symphonia_bundle_mp3::MpaDecoder;
use symphonia_core::{
    audio::{Audio, GenericAudioBufferRef},
    codecs::audio::{
        AudioCodecParameters, AudioDecoder, AudioDecoderOptions, well_known::CODEC_ID_MP2,
    },
    packet::PacketRef,
    units::{Duration as MediaDuration, Timestamp},
};

use crate::OUTPUT_RATE;

mod crc;
#[cfg(test)]
mod tests;
mod worker;

pub use worker::LayerTwoAudio;

/// The longest frame a header can announce.
pub const FRAME_BYTES: usize = 1729;
/// The most interleaved output samples one frame decodes to.
pub const PCM_SAMPLES: usize = 6920;
/// Samples per channel in one Layer II frame.
pub const FRAME_SAMPLES: u32 = 1152;

/// One frame header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub length: usize,
    pub rate: u32,
    pub mono: bool,
    pub bitrate: u32,
    pub mpeg1: bool,
}

impl Header {
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let &[a, b, c, d, ..] = bytes else {
            return None;
        };
        let version = (b >> 3) & 3;
        if a != 0xff || b & 0xe0 != 0xe0 || !matches!(version, 2 | 3) || b & 6 != 4 {
            return None;
        }
        let rates = [44100, 48000, 32000];
        let rate = *rates.get(usize::from((c >> 2) & 3))? / if version == 3 { 1 } else { 2 };
        let table = if version == 3 {
            [
                0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 0,
            ]
        } else {
            [
                0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
            ]
        };
        let bitrate = table[usize::from(c >> 4)];
        let length = (144000 * bitrate / rate) as usize + usize::from((c >> 1) & 1);
        (bitrate > 0 && (4..=FRAME_BYTES).contains(&length)).then_some(Self {
            length,
            rate,
            mono: d >> 6 == 3,
            bitrate,
            mpeg1: version == 3,
        })
    }

    /// Interleaved stereo samples at [`OUTPUT_RATE`] this frame decodes to.
    #[must_use]
    pub fn output_samples(self) -> usize {
        2 * FRAME_SAMPLES as usize * OUTPUT_RATE as usize / self.rate as usize
    }

    /// Bytes of scale factor CRC that sit before the two DAB PAD length bytes at the frame end.
    #[must_use]
    pub fn scf_crc_bytes(self) -> usize {
        if self.mpeg1 && self.bitrate < if self.mono { 56 } else { 112 } {
            2
        } else {
            4
        }
    }
}

/// What one byte did to a [`Framer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framed {
    Pending,
    /// The byte before it was dropped while hunting for a header.
    Skipped,
    /// A whole frame is buffered; [`Framer::frame`] holds it until the next byte.
    Frame(Header),
}

/// Splits a byte stream into whole frames without allocating.
pub struct Framer {
    bytes: [u8; FRAME_BYTES],
    used: usize,
    header: Option<Header>,
}

impl Default for Framer {
    fn default() -> Self {
        Self {
            bytes: [0; FRAME_BYTES],
            used: 0,
            header: None,
        }
    }
}

impl Framer {
    pub fn reset(&mut self) {
        self.used = 0;
        self.header = None;
    }

    /// Bytes held towards the next frame.
    #[must_use]
    pub const fn buffered(&self) -> usize {
        self.used
    }

    pub fn push(&mut self, byte: u8) -> Framed {
        self.bytes[self.used] = byte;
        self.used += 1;
        if self.used < 4 {
            return Framed::Pending;
        }
        if self.header.is_none() {
            self.header = Header::read(&self.bytes[..4]);
            if self.header.is_none() {
                self.bytes.copy_within(1..4, 0);
                self.used = 3;
                return Framed::Skipped;
            }
        }
        match self.header {
            Some(header) if self.used == header.length => {
                self.used = 0;
                self.header = None;
                Framed::Frame(header)
            }
            _ => Framed::Pending,
        }
    }

    /// The frame the last [`Framed::Frame`] announced.
    #[must_use]
    pub fn frame(&self, header: Header) -> &[u8] {
        &self.bytes[..header.length]
    }
}

/// Decodes whole frames to interleaved stereo at [`OUTPUT_RATE`].
pub struct Mp2Decoder {
    codec: MpaDecoder,
    spec: Option<(u32, bool)>,
    resampler: Option<FracResampler>,
    input: Vec<Complex<f32>>,
    output: Vec<Complex<f32>>,
}

fn codec() -> Result<MpaDecoder, String> {
    let mut params = AudioCodecParameters::new();
    params.codec = CODEC_ID_MP2;
    MpaDecoder::try_new(&params, &AudioDecoderOptions::default())
        .map_err(|error| format!("MPEG Layer II decoder: {error}"))
}

impl Mp2Decoder {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            codec: codec()?,
            spec: None,
            resampler: None,
            input: Vec::with_capacity(FRAME_SAMPLES as usize),
            output: Vec::with_capacity(3 * FRAME_SAMPLES as usize),
        })
    }

    pub fn reset(&mut self) {
        self.spec = None;
    }

    /// Decodes `frame` into `pcm`, returning how many samples it wrote.
    pub fn decode(
        &mut self,
        frame: &[u8],
        header: Header,
        pcm: &mut [f32],
    ) -> Result<usize, &'static str> {
        if !crc::valid(frame, header) {
            return Err("MPEG Layer II audio CRC mismatch");
        }
        self.configure(header)?;
        let packet = PacketRef::new(0, Timestamp::ZERO, MediaDuration::ZERO, frame);
        let GenericAudioBufferRef::F32(audio) = self
            .codec
            .decode_ref(&packet)
            .map_err(|_| "Invalid MPEG Layer II audio frame")?
        else {
            return Err("Unexpected MPEG Layer II sample format");
        };
        let left = audio
            .plane(0)
            .ok_or("MPEG Layer II frame has no audio channels")?;
        let right = audio.plane(1).unwrap_or(left);
        self.input.clear();
        self.input
            .extend(left.iter().zip(right).map(|(&l, &r)| Complex::new(l, r)));
        let samples = if let Some(resampler) = &mut self.resampler {
            resampler.process(&self.input, &mut self.output);
            &self.output
        } else {
            &self.input
        };
        interleave(samples, pcm)
    }

    fn configure(&mut self, header: Header) -> Result<(), &'static str> {
        let spec = (header.rate, header.mono);
        if self.spec != Some(spec) {
            self.codec = codec().map_err(|_| "MPEG Layer II decoder initialization failed")?;
            self.resampler = (spec.0 != OUTPUT_RATE)
                .then(|| FracResampler::new(f64::from(OUTPUT_RATE) / f64::from(spec.0)));
            self.spec = Some(spec);
        }
        Ok(())
    }
}

fn interleave(samples: &[Complex<f32>], pcm: &mut [f32]) -> Result<usize, &'static str> {
    let length = samples.len() * 2;
    if length > pcm.len() {
        return Err("MPEG Layer II audio output exceeds frame capacity");
    }
    if samples
        .iter()
        .any(|s| !s.re.is_finite() || !s.im.is_finite())
    {
        return Err("MPEG Layer II decoder produced non-finite audio");
    }
    for (pair, sample) in pcm[..length].as_chunks_mut::<2>().0.iter_mut().zip(samples) {
        pair[0] = sample.re.clamp(-1.0, 1.0);
        pair[1] = sample.im.clamp(-1.0, 1.0);
    }
    Ok(length)
}

/// A byte stream in, decoded frames out, on the caller's thread.
pub struct Mp2Stream {
    framer: Framer,
    decoder: Mp2Decoder,
    pcm: Box<[f32]>,
    synced: bool,
}

impl Mp2Stream {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            framer: Framer::default(),
            decoder: Mp2Decoder::new()?,
            pcm: vec![0.0; PCM_SAMPLES].into_boxed_slice(),
            synced: false,
        })
    }

    pub fn reset(&mut self) {
        self.framer.reset();
        self.decoder.reset();
        self.synced = false;
    }

    /// Bytes held towards the next frame.
    #[must_use]
    pub const fn buffered(&self) -> usize {
        self.framer.buffered()
    }

    /// Feeds `bytes`, handing `emit` each frame's audio or why it was lost. Losing sync after a
    /// frame counts as a loss; hunting for the first header does not.
    pub fn push(
        &mut self,
        bytes: &[u8],
        mut emit: impl FnMut(Result<(Header, &[f32]), &'static str>),
    ) {
        for &byte in bytes {
            match self.framer.push(byte) {
                Framed::Pending => {}
                Framed::Skipped => {
                    if self.synced {
                        self.synced = false;
                        self.decoder.reset();
                        emit(Err("Lost MPEG Layer II frame synchronization"));
                    }
                }
                Framed::Frame(header) => {
                    self.synced = true;
                    let frame = self.framer.frame(header);
                    match self.decoder.decode(frame, header, &mut self.pcm[..]) {
                        Ok(length) => emit(Ok((header, &self.pcm[..length]))),
                        Err(error) => {
                            self.decoder.reset();
                            emit(Err(error));
                        }
                    }
                }
            }
        }
    }
}
