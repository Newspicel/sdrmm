use crate::fusion::FUSION_FRAME_CELLS;
use crate::ws::StreamKind;

pub const PROTOCOL_VERSION: u8 = 1;

pub const HEADER_LEN: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
/// 2, 3 and 4 carried uncompressed IQ and video. They stay retired, so an old client never
/// misreads a new frame.
pub enum FrameKind {
    Spectrum = 0,
    AudioOpus = 1,
    Symbols = 5,
    RangeDoppler = 6,
    SpatialSpectrum = 7,
    Visibility = 8,
    FusionGrid = 9,
    IqI16 = 10,
    VideoJpeg = 11,
}

impl FrameKind {
    #[must_use]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Spectrum),
            1 => Some(Self::AudioOpus),
            5 => Some(Self::Symbols),
            6 => Some(Self::RangeDoppler),
            7 => Some(Self::SpatialSpectrum),
            8 => Some(Self::Visibility),
            9 => Some(Self::FusionGrid),
            10 => Some(Self::IqI16),
            11 => Some(Self::VideoJpeg),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("frame too short")]
    Short,
    #[error("frame protocol {0} is not supported")]
    Version(u8),
    #[error("frame kind {0} was not expected")]
    Kind(u8),
    #[error("frame length field disagrees with the buffer")]
    Length,
    #[error("frame cells do not match its shape")]
    Shape,
    #[error("frame field of {0} bytes does not fit a 16 bit length prefix")]
    Oversize(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub kind: FrameKind,
    pub stream_id: u16,
    pub seq: u32,
    pub timestamp: u64,
}

pub fn peek_header(buf: &[u8]) -> Result<FrameHeader, FrameError> {
    let mut reader = Reader::new(buf)?;
    let kind = FrameKind::from_u8(reader.kind).ok_or(FrameError::Kind(reader.kind))?;
    Ok(FrameHeader {
        kind,
        stream_id: reader.u16()?,
        seq: reader.u32()?,
        timestamp: reader.u64()?,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SymbolPlane {
    Complex = 0,
    Level = 1,
}

impl SymbolPlane {
    #[must_use]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Complex),
            1 => Some(Self::Level),
            _ => None,
        }
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
    kind: u8,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Result<Self, FrameError> {
        if buf.len() < HEADER_LEN {
            return Err(FrameError::Short);
        }
        if buf[0] != PROTOCOL_VERSION {
            return Err(FrameError::Version(buf[0]));
        }
        Ok(Self {
            buf,
            at: 2,
            kind: buf[1],
        })
    }

    fn accepting(buf: &'a [u8], accepts: FrameKind) -> Result<Self, FrameError> {
        let reader = Self::new(buf)?;
        if accepts as u8 == reader.kind {
            Ok(reader)
        } else {
            Err(FrameError::Kind(reader.kind))
        }
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let bytes = self.buf.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], FrameError> {
        self.take(N)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(FrameError::Short)
    }

    fn u8(&mut self) -> Result<u8, FrameError> {
        self.array::<1>().map(u8::from_le_bytes)
    }

    fn u16(&mut self) -> Result<u16, FrameError> {
        self.array().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Result<u32, FrameError> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Result<u64, FrameError> {
        self.array().map(u64::from_le_bytes)
    }

    fn f32(&mut self) -> Result<f32, FrameError> {
        self.array().map(f32::from_le_bytes)
    }

    fn f64(&mut self) -> Result<f64, FrameError> {
        self.array().map(f64::from_le_bytes)
    }

    fn rest(&mut self) -> &'a [u8] {
        let bytes = self.buf.get(self.at..).unwrap_or_default();
        self.at = self.buf.len();
        bytes
    }

    fn prefixed(&mut self) -> Result<&'a [u8], FrameError> {
        let len = self.u16()?;
        self.take(usize::from(len)).ok_or(FrameError::Length)
    }

    fn finish(&self) -> Result<(), FrameError> {
        if self.at == self.buf.len() {
            Ok(())
        } else {
            Err(FrameError::Length)
        }
    }
}

trait Shaped {
    fn shape_holds(&self) -> bool;
}

fn cells_fit(cells: usize, rows: u16, cols: u16) -> bool {
    rows > 0 && cols > 0 && cells == usize::from(rows) * usize::from(cols)
}

fn prefix_len(len: usize) -> Result<u16, FrameError> {
    u16::try_from(len).map_err(|_| FrameError::Oversize(len))
}

mod schema;
pub use schema::typescript_frames;

macro_rules! field_type {
    ($life:lifetime, bytes) => {
        &$life[u8]
    };
    ($life:lifetime, bytes16) => {
        &$life[u8]
    };
    ($life:lifetime, floats) => {
        &$life[f32]
    };
    ($life:lifetime, floats16) => {
        &$life[f32]
    };
    ($life:lifetime, shorts) => {
        &$life[i16]
    };
    ($life:lifetime, plane) => {
        SymbolPlane
    };
    ($life:lifetime, $scalar:ty) => {
        $scalar
    };
}

macro_rules! owned_type {
    (bytes) => { Vec<u8> };
    (bytes16) => { Vec<u8> };
    ($scalar:ty) => { $scalar };
}

macro_rules! borrow_field {
    ($value:expr, bytes) => {
        $value.as_slice()
    };
    ($value:expr, bytes16) => {
        $value.as_slice()
    };
    ($value:expr, $scalar:ty) => {
        $value
    };
}

macro_rules! write_field {
    ($buf:ident, $value:expr, bytes) => {
        $buf.extend_from_slice($value)
    };
    ($buf:ident, $value:expr, bytes16) => {{
        $buf.extend_from_slice(&prefix_len($value.len())?.to_le_bytes());
        $buf.extend_from_slice($value);
    }};
    ($buf:ident, $value:expr, floats) => {
        for value in $value {
            $buf.extend_from_slice(&value.to_le_bytes());
        }
    };
    ($buf:ident, $value:expr, floats16) => {{
        $buf.extend_from_slice(&prefix_len($value.len())?.to_le_bytes());
        for value in $value {
            $buf.extend_from_slice(&value.to_le_bytes());
        }
    }};
    ($buf:ident, $value:expr, shorts) => {
        for value in $value {
            $buf.extend_from_slice(&value.to_le_bytes());
        }
    };
    ($buf:ident, $value:expr, plane) => {
        $buf.push($value as u8)
    };
    ($buf:ident, $value:expr, $scalar:ty) => {
        $buf.extend_from_slice(&$value.to_le_bytes())
    };
}

macro_rules! read_field {
    ($reader:ident, bytes) => {
        $reader.rest()
    };
    ($reader:ident, bytes16) => {
        $reader.prefixed()?
    };
    ($reader:ident, $scalar:ident) => {
        $reader.$scalar()?
    };
}

macro_rules! field_len {
    ($value:expr, bytes) => {
        $value.len()
    };
    ($value:expr, bytes16) => {
        2 + $value.len()
    };
    ($value:expr, floats) => {
        $value.len() * 4
    };
    ($value:expr, floats16) => {
        2 + $value.len() * 4
    };
    ($value:expr, shorts) => {
        $value.len() * 2
    };
    ($value:expr, plane) => {
        1
    };
    ($value:expr, $scalar:ty) => {
        std::mem::size_of::<$scalar>()
    };
}

macro_rules! define_frame {
    ($name:ident, $kind:ident, {$($field:ident: $ty:ident),* $(,)?}) => {
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name<'a> {
            pub stream_id: u16,
            pub seq: u32,
            pub timestamp: u64,
            $(pub $field: field_type!('a, $ty),)*
        }
        impl $name<'_> {
            #[must_use]
            pub fn encoded_len(&self) -> usize {
                HEADER_LEN $(+ field_len!(self.$field, $ty))*
            }
            pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
                if !Shaped::shape_holds(self) {
                    return Err(FrameError::Shape);
                }
                let mut buf = Vec::with_capacity(self.encoded_len());
                buf.push(PROTOCOL_VERSION);
                buf.push(FrameKind::$kind as u8);
                buf.extend_from_slice(&self.stream_id.to_le_bytes());
                buf.extend_from_slice(&self.seq.to_le_bytes());
                buf.extend_from_slice(&self.timestamp.to_le_bytes());
                $(write_field!(buf, self.$field, $ty);)*
                Ok(buf)
            }
            fn fields() -> &'static [(&'static str, &'static str)] {
                &[$((stringify!($field), stringify!($ty))),*]
            }
        }
    };
    ($name:ident, $kind:ident, decode, {$($field:ident: $ty:ident),* $(,)?}) => {
        define_frame!($name, $kind, {$($field: $ty),*});
        impl<'a> $name<'a> {
            pub fn decode(buf: &'a [u8]) -> Result<Self, FrameError> {
                let mut reader = Reader::accepting(buf, FrameKind::$kind)?;
                let stream_id = reader.u16()?;
                let seq = reader.u32()?;
                let timestamp = reader.u64()?;
                $(let $field = read_field!(reader, $ty);)*
                reader.finish()?;
                let frame = Self { stream_id, seq, timestamp, $($field),* };
                if Shaped::shape_holds(&frame) {
                    Ok(frame)
                } else {
                    Err(FrameError::Shape)
                }
            }
        }
    };
    (
        $name:ident, $kind:ident, decode, owned($owned:ident),
        {$($field:ident: $ty:ident),* $(,)?}
    ) => {
        define_frame!($name, $kind, decode, {$($field: $ty),*});
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct $owned {
            pub stream_id: u16,
            pub seq: u32,
            pub timestamp: u64,
            $(pub $field: owned_type!($ty),)*
        }
        impl $owned {
            #[must_use]
            pub fn frame(&self) -> $name<'_> {
                $name {
                    stream_id: self.stream_id,
                    seq: self.seq,
                    timestamp: self.timestamp,
                    $($field: borrow_field!(self.$field, $ty),)*
                }
            }
        }
    };
}

pub const SPECTRUM_SIGNAL_MARGIN_DB: f32 = 10.0;
/// What an IQ sample of `i16::MAX` stands for is the frame's `scale`.
pub const IQ_FULL_SCALE: i16 = i16::MAX;
pub const VIDEO_GRAY: u8 = 1;
pub const VIDEO_RGB: u8 = 3;

define_frame!(SpectrumFrame, Spectrum, decode, {
    center_hz: f64, span_hz: f32, db_min: f32, db_max: f32, floor_db: f32, bins: bytes16,
});
define_frame!(AudioFrame, AudioOpus, decode, { ch_layout: u8, opus: bytes });
define_frame!(IqFrame, IqI16, {
    center_hz: f64, sample_rate: f32, scale: f32, samples: shorts,
});
define_frame!(SymbolFrame, Symbols, {
    plane: plane, symbol_rate: f32, evm: f32, mer_db: f32, margin: f32,
    freq_error_hz: f32, reference: floats16, symbols: floats,
});
define_frame!(RangeDopplerFrame, RangeDoppler, decode, owned(RangeDopplerOwned), {
    ranges: u16, dopplers: u16,
    range_first_m: f32, range_step_m: f32,
    doppler_first_hz: f32, doppler_step_hz: f32,
    carrier_hz: f64,
    db_min: f32, db_max: f32,
    cells: bytes,
});
define_frame!(VideoFrame, VideoJpeg, decode, {
    width: u16, height: u16, channels: u8, jpeg: bytes,
});
define_frame!(SpatialSpectrumFrame, SpatialSpectrum, decode, owned(SpatialSpectrumOwned), {
    center_hz: f64, span_hz: f32, bearings: u16, bins: u16, db_min: f32, db_max: f32, cells: bytes,
});
define_frame!(VisibilityFrame, Visibility, decode, owned(VisibilityOwned), {
    center_hz: f64, span_hz: f32, baselines: u16, bins: u16, db_min: f32, db_max: f32,
    amplitude: bytes16, phase: bytes,
});
define_frame!(FusionGridFrame, FusionGrid, decode, owned(FusionGridOwned), {
    south: f64, west: f64, north: f64, east: f64, cols: u16, rows: u16, cells: bytes,
});

impl Shaped for SpectrumFrame<'_> {
    fn shape_holds(&self) -> bool {
        true
    }
}

impl Shaped for AudioFrame<'_> {
    fn shape_holds(&self) -> bool {
        true
    }
}

impl Shaped for IqFrame<'_> {
    fn shape_holds(&self) -> bool {
        true
    }
}

impl Shaped for SymbolFrame<'_> {
    fn shape_holds(&self) -> bool {
        true
    }
}

impl Shaped for VideoFrame<'_> {
    fn shape_holds(&self) -> bool {
        self.width > 0
            && self.height > 0
            && matches!(self.channels, VIDEO_GRAY | VIDEO_RGB)
            && !self.jpeg.is_empty()
    }
}

impl Shaped for RangeDopplerFrame<'_> {
    fn shape_holds(&self) -> bool {
        cells_fit(self.cells.len(), self.dopplers, self.ranges)
    }
}

impl Shaped for SpatialSpectrumFrame<'_> {
    fn shape_holds(&self) -> bool {
        cells_fit(self.cells.len(), self.bearings, self.bins)
    }
}

impl Shaped for VisibilityFrame<'_> {
    fn shape_holds(&self) -> bool {
        cells_fit(self.amplitude.len(), self.baselines, self.bins)
            && self.phase.len() == self.amplitude.len()
    }
}

impl Shaped for FusionGridFrame<'_> {
    fn shape_holds(&self) -> bool {
        cells_fit(self.cells.len(), self.rows, self.cols)
            && self.cols <= FUSION_FRAME_CELLS
            && self.rows <= FUSION_FRAME_CELLS
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceFrame {
    RangeDoppler(RangeDopplerOwned),
    SpatialSpectrum(SpatialSpectrumOwned),
    Visibility(VisibilityOwned),
    FusionGrid(FusionGridOwned),
}

impl SurfaceFrame {
    #[must_use]
    pub const fn kind(&self) -> StreamKind {
        match self {
            Self::RangeDoppler(_) => StreamKind::RangeDoppler,
            Self::SpatialSpectrum(_) => StreamKind::SpatialSpectrum,
            Self::Visibility(_) => StreamKind::Visibility,
            Self::FusionGrid(_) => StreamKind::FusionGrid,
        }
    }

    pub fn encode(&self, stream_id: u16) -> Result<Vec<u8>, FrameError> {
        match self {
            Self::RangeDoppler(owned) => RangeDopplerFrame {
                stream_id,
                ..owned.frame()
            }
            .encode(),
            Self::SpatialSpectrum(owned) => SpatialSpectrumFrame {
                stream_id,
                ..owned.frame()
            }
            .encode(),
            Self::Visibility(owned) => VisibilityFrame {
                stream_id,
                ..owned.frame()
            }
            .encode(),
            Self::FusionGrid(owned) => FusionGridFrame {
                stream_id,
                ..owned.frame()
            }
            .encode(),
        }
    }
}

#[cfg(test)]
mod tests;
