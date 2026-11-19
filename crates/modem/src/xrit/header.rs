pub const PRIMARY: u8 = 0;
pub const IMAGE_STRUCTURE: u8 = 1;
pub const ANNOTATION: u8 = 4;
pub const TIME_STAMP: u8 = 5;
pub const KEY: u8 = 7;
pub const SEGMENT: u8 = 128;
pub const NOAA: u8 = 129;
pub const RICE: u8 = 131;
pub const PRIMARY_LEN: usize = 16;
const RECORD_HEAD: usize = 3;

/// LRIT/HRIT file types (CGMS 03, NOAA and KMA additions).
pub const IMAGE: u8 = 0;
pub const MESSAGE: u8 = 1;
pub const TEXT: u8 = 2;
pub const KEY_MESSAGE: u8 = 3;
pub const DCS: u8 = 130;
pub const ADDITIONAL: u8 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Primary {
    pub file_type: u8,
    pub header_len: usize,
    pub data_bits: u64,
}

impl Primary {
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let head = bytes.get(..PRIMARY_LEN)?;
        if head[0] != PRIMARY || usize::from(u16::from_be_bytes([head[1], head[2]])) != PRIMARY_LEN
        {
            return None;
        }
        let header_len = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let data_bits = u64::from_be_bytes(head[8..16].try_into().ok()?);
        (header_len >= PRIMARY_LEN).then_some(Self {
            file_type: head[3],
            header_len,
            data_bits,
        })
    }

    #[must_use]
    pub fn data_len(self) -> usize {
        self.data_bits.div_ceil(8) as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageStructure {
    pub bits_per_pixel: u8,
    pub columns: u16,
    pub lines: u16,
    pub compression: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentId {
    pub image: u16,
    pub sequence: u16,
    pub start_column: u16,
    pub start_line: u16,
    pub max_segment: u16,
    pub max_column: u16,
    pub max_line: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Noaa {
    pub agency: [u8; 4],
    pub product: u16,
    pub subproduct: u16,
    pub parameter: u16,
    pub compression: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rice {
    pub flags: u16,
    pub pixels_per_block: u8,
    pub lines_per_packet: u8,
}

/// CCSDS day segmented time: days since 1958-01-01 and milliseconds of the day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CdsTime {
    pub days: u16,
    pub millis: u32,
}

const UNIX_EPOCH_DAYS: i64 = 4_383;

impl CdsTime {
    #[must_use]
    pub fn unix_seconds(self) -> i64 {
        (i64::from(self.days) - UNIX_EPOCH_DAYS) * 86_400 + i64::from(self.millis / 1_000)
    }
}

/// The header records of one xRIT file, read in place.
#[derive(Clone, Copy, Debug)]
pub struct Headers<'a> {
    pub primary: Primary,
    bytes: &'a [u8],
}

impl<'a> Headers<'a> {
    /// The headers of `file`, if it holds all `header_len` bytes the primary header claims.
    #[must_use]
    pub fn parse(file: &'a [u8]) -> Option<Self> {
        let primary = Primary::parse(file)?;
        let bytes = file.get(..primary.header_len)?;
        Some(Self { primary, bytes })
    }

    pub fn records(self) -> impl Iterator<Item = (u8, &'a [u8])> {
        let mut at = 0;
        std::iter::from_fn(move || {
            let head = self.bytes.get(at..at + RECORD_HEAD)?;
            let len = usize::from(u16::from_be_bytes([head[1], head[2]]));
            let body = self
                .bytes
                .get(at + RECORD_HEAD..at + len.max(RECORD_HEAD))?;
            if len < RECORD_HEAD {
                return None;
            }
            at += len;
            Some((head[0], body))
        })
    }

    #[must_use]
    pub fn record(self, kind: u8) -> Option<&'a [u8]> {
        self.records()
            .find(|&(found, _)| found == kind)
            .map(|(_, body)| body)
    }

    #[must_use]
    pub fn annotation(self) -> Option<&'a str> {
        let text = std::str::from_utf8(self.record(ANNOTATION)?).ok()?;
        Some(text.trim_matches(|c: char| c == '\0' || c.is_whitespace()))
    }

    #[must_use]
    pub fn image(self) -> Option<ImageStructure> {
        let body = self.record(IMAGE_STRUCTURE)?.get(..6)?;
        Some(ImageStructure {
            bits_per_pixel: body[0],
            columns: u16::from_be_bytes([body[1], body[2]]),
            lines: u16::from_be_bytes([body[3], body[4]]),
            compression: body[5],
        })
    }

    #[must_use]
    pub fn segment(self) -> Option<SegmentId> {
        let body = self.record(SEGMENT)?.get(..14)?;
        let word = |index: usize| u16::from_be_bytes([body[2 * index], body[2 * index + 1]]);
        Some(SegmentId {
            image: word(0),
            sequence: word(1),
            start_column: word(2),
            start_line: word(3),
            max_segment: word(4),
            max_column: word(5),
            max_line: word(6),
        })
    }

    #[must_use]
    pub fn noaa(self) -> Option<Noaa> {
        let body = self.record(NOAA)?.get(..11)?;
        Some(Noaa {
            agency: [body[0], body[1], body[2], body[3]],
            product: u16::from_be_bytes([body[4], body[5]]),
            subproduct: u16::from_be_bytes([body[6], body[7]]),
            parameter: u16::from_be_bytes([body[8], body[9]]),
            compression: body[10],
        })
    }

    #[must_use]
    pub fn rice(self) -> Option<Rice> {
        let body = self.record(RICE)?.get(..4)?;
        Some(Rice {
            flags: u16::from_be_bytes([body[0], body[1]]),
            pixels_per_block: body[2],
            lines_per_packet: body[3],
        })
    }

    /// The encryption key index; 0 means the data are in the clear.
    #[must_use]
    pub fn key(self) -> Option<u16> {
        let body = self.record(KEY)?;
        let index = body.get(body.len().checked_sub(2)?..)?;
        Some(u16::from_be_bytes([index[0], index[1]]))
    }

    #[must_use]
    pub fn time(self) -> Option<CdsTime> {
        let body = self.record(TIME_STAMP)?.get(..7)?;
        Some(CdsTime {
            days: u16::from_be_bytes([body[1], body[2]]),
            millis: u32::from_be_bytes([body[3], body[4], body[5], body[6]]),
        })
    }
}

/// Builds header records, the sender's side of [`Headers`].
#[derive(Clone, Debug, Default)]
pub struct HeaderWriter {
    records: Vec<u8>,
}

impl HeaderWriter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn record(mut self, kind: u8, body: &[u8]) -> Self {
        let len = (RECORD_HEAD + body.len()) as u16;
        self.records.push(kind);
        self.records.extend_from_slice(&len.to_be_bytes());
        self.records.extend_from_slice(body);
        self
    }

    #[must_use]
    pub fn image(self, image: ImageStructure) -> Self {
        let mut body = vec![image.bits_per_pixel];
        body.extend_from_slice(&image.columns.to_be_bytes());
        body.extend_from_slice(&image.lines.to_be_bytes());
        body.push(image.compression);
        self.record(IMAGE_STRUCTURE, &body)
    }

    #[must_use]
    pub fn annotation(self, name: &str) -> Self {
        self.record(ANNOTATION, name.as_bytes())
    }

    #[must_use]
    pub fn time(self, time: CdsTime) -> Self {
        let mut body = vec![0x40];
        body.extend_from_slice(&time.days.to_be_bytes());
        body.extend_from_slice(&time.millis.to_be_bytes());
        self.record(TIME_STAMP, &body)
    }

    #[must_use]
    pub fn key(self, index: u16) -> Self {
        let mut body = vec![0, 0];
        body.extend_from_slice(&index.to_be_bytes());
        self.record(KEY, &body)
    }

    #[must_use]
    pub fn segment(self, segment: SegmentId) -> Self {
        let body: Vec<u8> = [
            segment.image,
            segment.sequence,
            segment.start_column,
            segment.start_line,
            segment.max_segment,
            segment.max_column,
            segment.max_line,
        ]
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect();
        self.record(SEGMENT, &body)
    }

    #[must_use]
    pub fn noaa(self, noaa: Noaa) -> Self {
        let mut body = noaa.agency.to_vec();
        for word in [noaa.product, noaa.subproduct, noaa.parameter] {
            body.extend_from_slice(&word.to_be_bytes());
        }
        body.push(noaa.compression);
        self.record(NOAA, &body)
    }

    #[must_use]
    pub fn rice(self, rice: Rice) -> Self {
        let mut body = rice.flags.to_be_bytes().to_vec();
        body.push(rice.pixels_per_block);
        body.push(rice.lines_per_packet);
        self.record(RICE, &body)
    }

    /// The whole file: primary header, these records, then `data`.
    #[must_use]
    pub fn file(&self, file_type: u8, data: &[u8]) -> Vec<u8> {
        let header_len = (PRIMARY_LEN + self.records.len()) as u32;
        let mut out = vec![PRIMARY, 0, PRIMARY_LEN as u8, file_type];
        out.extend_from_slice(&header_len.to_be_bytes());
        out.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
        out.extend_from_slice(&self.records);
        out.extend_from_slice(data);
        out
    }
}
