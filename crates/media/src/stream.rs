//! Encodes a channel's pictures and sound as an MPEG transport stream other programs play:
//! MPEG-2 video and MPEG audio through `FFmpeg`'s own encoders, multiplexed in pure Rust.

use std::collections::VecDeque;

use ffmpeg_the_third::{
    self as av, ChannelLayout, ChannelLayoutMask, Dictionary, Rational, codec, frame, picture,
    software,
    util::format::{Pixel, Sample, sample::Type},
};
use sdrmm_decoder::{AUDIO_RATE, VideoPicture};
use sdrmm_modem::{
    dvb::PACKET,
    mpegts::{AUDIO_PID, PAT_PID, PMT_PID, TsWriter, VIDEO_PID, pat, pmt_of},
};

#[cfg(test)]
mod tests;

/// The transport stream's clock, in ticks a second.
pub const CLOCK_HZ: u64 = 90_000;
/// How far presentation runs behind the programme clock, so a player has frames buffered.
const DELAY: u64 = CLOCK_HZ * 2 / 5;
const CLOCK_EVERY: u64 = CLOCK_HZ / 25;
const TABLES_EVERY: u64 = CLOCK_HZ / 4;
const FRAME_RATE: u64 = 25;
const FRAME_TICKS: u64 = CLOCK_HZ / FRAME_RATE;
const MPEG2_VIDEO: u8 = 0x02;
const MPEG1_AUDIO: u8 = 0x03;
const VIDEO_STREAM_ID: u8 = 0xE0;
const AUDIO_STREAM_ID: u8 = 0xC0;
const BITS_PER_PIXEL: f64 = 0.3;
/// The largest buffer MPEG-2 main profile allows a decoder, in bits.
const MAX_VBV_BITS: usize = 9_781_248;
const AUDIO_BITRATE: usize = 192_000;
const MP2_FRAME: usize = 1152;
const MP2_FRAME_TICKS: u64 = MP2_FRAME as u64 * CLOCK_HZ / AUDIO_RATE as u64;
/// Audio that drifts this far from the wall clock starts over at the wall clock.
const MAX_DRIFT: u64 = CLOCK_HZ / 2;
const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1152;

/// One stream's encoders and multiplexer. Every call takes `now`, the wall clock in
/// [`CLOCK_HZ`] ticks, which must never go back.
pub struct TsEncoder {
    writer: TsWriter,
    with_audio: bool,
    video: Option<VideoEncoder>,
    audio: Option<AudioEncoder>,
    tabled_at: Option<u64>,
    clocked_at: Option<u64>,
}

impl TsEncoder {
    /// A stream of pictures, and of sound too when `with_audio`.
    #[must_use]
    pub fn new(with_audio: bool) -> Self {
        Self {
            writer: TsWriter::new(),
            with_audio,
            video: None,
            audio: None,
            tabled_at: None,
            clocked_at: None,
        }
    }

    /// Encodes `picture`, shown at `now`. Pictures faster than 25 a second are thinned out.
    pub fn picture(
        &mut self,
        picture: &VideoPicture,
        now: u64,
        out: &mut Vec<[u8; PACKET]>,
    ) -> Result<(), String> {
        self.clock(now, out);
        let size = coded_size(picture)?;
        if self.video.as_ref().is_none_or(|video| video.size != size) {
            self.video = Some(VideoEncoder::open(size)?);
        }
        let Some(video) = &mut self.video else {
            return Err("Video encoder missing".to_owned());
        };
        let mut units = Vec::new();
        video.encode(picture, now + DELAY, &mut units)?;
        for unit in units {
            if unit.key {
                self.tables(now, out);
            }
            self.writer
                .pes(VIDEO_PID, VIDEO_STREAM_ID, unit.pts, &unit.data, out);
        }
        Ok(())
    }

    /// Encodes interleaved stereo at the decoder rate, its first sample heard at `now`.
    pub fn audio(
        &mut self,
        stereo: &[f32],
        now: u64,
        out: &mut Vec<[u8; PACKET]>,
    ) -> Result<(), String> {
        if !self.with_audio {
            return Ok(());
        }
        self.clock(now, out);
        if self.audio.is_none() {
            self.audio = Some(AudioEncoder::open()?);
        }
        let Some(audio) = &mut self.audio else {
            return Err("Audio encoder missing".to_owned());
        };
        let mut units = Vec::new();
        audio.encode(stereo, now + DELAY, &mut units)?;
        for unit in units {
            self.writer
                .pes(AUDIO_PID, AUDIO_STREAM_ID, unit.pts, &unit.data, out);
        }
        Ok(())
    }

    fn clock(&mut self, now: u64, out: &mut Vec<[u8; PACKET]>) {
        if self
            .tabled_at
            .is_none_or(|at| now.saturating_sub(at) >= TABLES_EVERY)
        {
            self.tables(now, out);
        }
        if self
            .clocked_at
            .is_none_or(|at| now.saturating_sub(at) >= CLOCK_EVERY)
        {
            self.writer.pcr(VIDEO_PID, now, out);
            self.clocked_at = Some(now);
        }
    }

    fn tables(&mut self, now: u64, out: &mut Vec<[u8; PACKET]>) {
        let video: (u8, u16, &[u8]) = (MPEG2_VIDEO, VIDEO_PID, &[]);
        let audio: (u8, u16, &[u8]) = (MPEG1_AUDIO, AUDIO_PID, &[]);
        let map = if self.with_audio {
            pmt_of(VIDEO_PID, &[video, audio])
        } else {
            pmt_of(VIDEO_PID, &[video])
        };
        self.writer.section(PAT_PID, &pat(), out);
        self.writer.section(PMT_PID, &map, out);
        self.tabled_at = Some(now);
    }
}

struct Unit {
    pts: u64,
    key: bool,
    data: Vec<u8>,
}

/// The size a picture is coded at: even, as 4:2:0 needs, and no larger than MPEG-2 high level.
fn coded_size(picture: &VideoPicture) -> Result<(u32, u32), String> {
    let (width, height) = (u32::from(picture.width), u32::from(picture.height));
    let scale = (f64::from(MAX_WIDTH) / f64::from(width.max(1)))
        .min(f64::from(MAX_HEIGHT) / f64::from(height.max(1)))
        .min(1.0);
    let even = |side: u32| ((f64::from(side) * scale) as u32) & !1;
    let size = (even(width), even(height));
    if size.0 < 16 || size.1 < 16 {
        return Err(format!("Picture too small to stream: {width}x{height}"));
    }
    Ok(size)
}

struct VideoEncoder {
    size: (u32, u32),
    encoder: codec::encoder::video::Encoder,
    scaler: Option<(Pixel, u32, u32, software::scaling::Context)>,
    last_frame: Option<i64>,
}

impl VideoEncoder {
    fn open(size: (u32, u32)) -> Result<Self, String> {
        let found = av::encoder::find_by_name("mpeg2video")
            .ok_or_else(|| "FFmpeg has no MPEG-2 video encoder".to_owned())?;
        let mut context = codec::Context::new_with_codec(found);
        context.set_flags(codec::Flags::LOW_DELAY);
        let mut video = context.encoder().video().map_err(|e| e.to_string())?;
        let bitrate = (f64::from(size.0 * size.1) * FRAME_RATE as f64 * BITS_PER_PIXEL) as usize;
        video.set_width(size.0);
        video.set_height(size.1);
        video.set_format(Pixel::YUV420P);
        video.set_time_base(Rational::new(1, FRAME_RATE as i32));
        video.set_frame_rate(Some(Rational::new(FRAME_RATE as i32, 1)));
        video.set_gop(FRAME_RATE as u32);
        video.set_max_b_frames(0);
        video.set_bit_rate(bitrate);
        video.set_max_bit_rate(bitrate * 2);
        let mut options = Dictionary::new();
        options.set("bufsize", bitrate.min(MAX_VBV_BITS).to_string());
        let encoder = video
            .open_as_with(found, options)
            .map_err(|e| format!("MPEG-2 encoder: {e}"))?;
        Ok(Self {
            size,
            encoder,
            scaler: None,
            last_frame: None,
        })
    }

    fn encode(
        &mut self,
        picture: &VideoPicture,
        pts: u64,
        units: &mut Vec<Unit>,
    ) -> Result<(), String> {
        let index = (pts / FRAME_TICKS) as i64;
        if self.last_frame.is_some_and(|last| index <= last) {
            return Ok(());
        }
        self.last_frame = Some(index);
        let source = source_frame(picture)?;
        let mut coded = frame::Video::empty();
        self.scaler(&source)?
            .run(&source, &mut coded)
            .map_err(|e| e.to_string())?;
        coded.set_pts(Some(index));
        coded.set_kind(picture::Type::None);
        self.encoder.send_frame(&coded).map_err(|e| e.to_string())?;
        let mut packet = av::Packet::empty();
        while received(self.encoder.receive_packet(&mut packet))? {
            let Some(data) = packet.data() else { continue };
            let frame = packet.pts().unwrap_or(index).max(0) as u64;
            units.push(Unit {
                pts: frame * FRAME_TICKS,
                key: packet.is_key(),
                data: data.to_vec(),
            });
        }
        Ok(())
    }

    fn scaler(&mut self, source: &frame::Video) -> Result<&mut software::scaling::Context, String> {
        let spec = (source.format(), source.width(), source.height());
        if self
            .scaler
            .as_ref()
            .is_none_or(|(format, width, height, _)| (*format, *width, *height) != spec)
        {
            let context = software::scaling::Context::get(
                spec.0,
                spec.1,
                spec.2,
                Pixel::YUV420P,
                self.size.0,
                self.size.1,
                software::scaling::Flags::BILINEAR,
            )
            .map_err(|e| e.to_string())?;
            self.scaler = Some((spec.0, spec.1, spec.2, context));
        }
        match &mut self.scaler {
            Some((_, _, _, context)) => Ok(context),
            None => Err("Video scaler missing".to_owned()),
        }
    }
}

/// A picture as an `FFmpeg` frame: its colour when it has some, else its grey.
fn source_frame(picture: &VideoPicture) -> Result<frame::Video, String> {
    let (width, height) = (usize::from(picture.width), usize::from(picture.height));
    let (format, bytes, plane) = if picture.rgb.is_empty() {
        (Pixel::GRAY8, 1, &picture.luma)
    } else {
        (Pixel::RGB24, 3, &picture.rgb)
    };
    let row = width * bytes;
    if plane.len() != row * height {
        return Err(format!(
            "Picture of {width}x{height} carries {} bytes",
            plane.len()
        ));
    }
    let mut source = frame::Video::new(format, width as u32, height as u32);
    let stride = source.stride(0);
    for (target, line) in source.data_mut(0).chunks_mut(stride).zip(plane.chunks(row)) {
        target[..row].copy_from_slice(line);
    }
    Ok(source)
}

struct AudioEncoder {
    encoder: codec::encoder::audio::Encoder,
    pending: Vec<f32>,
    pending_at: Option<u64>,
    sent: VecDeque<u64>,
    samples: i64,
}

impl AudioEncoder {
    fn open() -> Result<Self, String> {
        let found = av::encoder::find_by_name("mp2")
            .ok_or_else(|| "FFmpeg has no MPEG audio encoder".to_owned())?;
        let mut audio = codec::Context::new_with_codec(found)
            .encoder()
            .audio()
            .map_err(|e| e.to_string())?;
        audio.set_rate(AUDIO_RATE as i32);
        audio.set_format(Sample::I16(Type::Packed));
        audio.set_ch_layout(ChannelLayout::STEREO);
        audio.set_bit_rate(AUDIO_BITRATE);
        audio.set_time_base(Rational::new(1, AUDIO_RATE as i32));
        let encoder = audio
            .open_as(found)
            .map_err(|e| format!("MPEG audio encoder: {e}"))?;
        Ok(Self {
            encoder,
            pending: Vec::with_capacity(MP2_FRAME * 4),
            pending_at: None,
            sent: VecDeque::new(),
            samples: 0,
        })
    }

    fn encode(&mut self, stereo: &[f32], pts: u64, units: &mut Vec<Unit>) -> Result<(), String> {
        let frames = (self.pending.len() / 2) as u64;
        let expected = self
            .pending_at
            .map(|at| at + frames * CLOCK_HZ / u64::from(AUDIO_RATE));
        if expected.is_none_or(|expected| expected.abs_diff(pts) > MAX_DRIFT) {
            self.pending.clear();
            self.pending_at = Some(pts);
        }
        self.pending.extend_from_slice(stereo);
        while self.pending.len() >= MP2_FRAME * 2 {
            self.send()?;
            self.receive(units)?;
        }
        Ok(())
    }

    fn send(&mut self) -> Result<(), String> {
        let mut block = frame::Audio::new(
            Sample::I16(Type::Packed),
            MP2_FRAME,
            ChannelLayoutMask::STEREO,
        );
        block.set_rate(AUDIO_RATE);
        for (target, pair) in block
            .plane_mut::<(i16, i16)>(0)
            .iter_mut()
            .zip(self.pending.as_chunks::<2>().0)
        {
            *target = (to_i16(pair[0]), to_i16(pair[1]));
        }
        block.set_pts(Some(self.samples));
        self.samples += MP2_FRAME as i64;
        self.pending.drain(..MP2_FRAME * 2);
        let at = self.pending_at.unwrap_or_default();
        self.sent.push_back(at);
        self.pending_at = Some(at + MP2_FRAME_TICKS);
        self.encoder.send_frame(&block).map_err(|e| e.to_string())
    }

    fn receive(&mut self, units: &mut Vec<Unit>) -> Result<(), String> {
        let mut packet = av::Packet::empty();
        while received(self.encoder.receive_packet(&mut packet))? {
            let Some(data) = packet.data() else { continue };
            let pts = self.sent.pop_front().unwrap_or_default();
            units.push(Unit {
                pts,
                key: true,
                data: data.to_vec(),
            });
        }
        Ok(())
    }
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

/// Whether the encoder handed a packet back; `false` once it wants more input.
fn received(result: Result<(), av::Error>) -> Result<bool, String> {
    match result {
        Ok(()) => Ok(true),
        Err(av::Error::Eof) => Ok(false),
        Err(av::Error::Other { errno })
            if std::io::Error::from_raw_os_error(errno).kind()
                == std::io::ErrorKind::WouldBlock =>
        {
            Ok(false)
        }
        Err(error) => Err(error.to_string()),
    }
}
