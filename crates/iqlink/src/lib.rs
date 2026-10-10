pub const CONTROL_PORT: u16 = 30_432;
pub const HEADER_BYTES: usize = 24;
pub const DATAGRAM_BYTES: usize = 1_472;

const MAGIC: [u8; 4] = *b"IQLK";
const VERSION: u8 = 1;
const START: &str = "START";
const OK: &str = "OK";
const ERR: &str = "ERR";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("datagram of {0} bytes is shorter than its header")]
    Short(usize),
    #[error("datagram does not start with the iqlink magic")]
    Magic,
    #[error("iqlink version {0} is not supported")]
    Version(u8),
    #[error("malformed iqlink line: {0}")]
    Line(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub frame_bytes: u16,
    pub index: u64,
    pub board_lost: u64,
}

impl Header {
    #[must_use]
    pub fn encode(&self) -> [u8; HEADER_BYTES] {
        let mut out = [0u8; HEADER_BYTES];
        out[..4].copy_from_slice(&MAGIC);
        out[4] = VERSION;
        out[6..8].copy_from_slice(&self.frame_bytes.to_le_bytes());
        out[8..16].copy_from_slice(&self.index.to_le_bytes());
        out[16..24].copy_from_slice(&self.board_lost.to_le_bytes());
        out
    }

    pub fn decode(datagram: &[u8]) -> Result<Self, Error> {
        let Some(head) = datagram.get(..HEADER_BYTES) else {
            return Err(Error::Short(datagram.len()));
        };
        if head[..4] != MAGIC {
            return Err(Error::Magic);
        }
        if head[4] != VERSION {
            return Err(Error::Version(head[4]));
        }
        Ok(Self {
            frame_bytes: u16::from_le_bytes([head[6], head[7]]),
            index: u64::from_le_bytes(word(&head[8..16])),
            board_lost: u64::from_le_bytes(word(&head[16..24])),
        })
    }
}

fn word(bytes: &[u8]) -> [u8; 8] {
    let mut out = [0u8; 8];
    out.copy_from_slice(bytes);
    out
}

#[must_use]
pub const fn frames_per_datagram(frame_bytes: usize) -> usize {
    if frame_bytes == 0 {
        return 0;
    }
    (DATAGRAM_BYTES - HEADER_BYTES) / frame_bytes
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    pub device: String,
    pub elements: Vec<u32>,
    pub buffer_frames: usize,
    pub udp_port: u16,
}

impl Start {
    #[must_use]
    pub fn line(&self) -> String {
        let elements: Vec<String> = self.elements.iter().map(u32::to_string).collect();
        format!(
            "{START} {} {} {} {}\n",
            self.device,
            elements.join(","),
            self.buffer_frames,
            self.udp_port
        )
    }

    pub fn parse(line: &str) -> Result<Self, Error> {
        let bad = || Error::Line(line.trim().to_string());
        let mut words = line.split_whitespace();
        if words.next() != Some(START) {
            return Err(bad());
        }
        let device = words.next().ok_or_else(bad)?.to_string();
        let elements = words
            .next()
            .ok_or_else(bad)?
            .split(',')
            .map(str::parse)
            .collect::<Result<Vec<u32>, _>>()
            .map_err(|_| bad())?;
        let buffer_frames = words.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
        let udp_port = words.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
        if words.next().is_some() || elements.is_empty() || buffer_frames == 0 {
            return Err(bad());
        }
        Ok(Self {
            device,
            elements,
            buffer_frames,
            udp_port,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Streaming { frame_bytes: usize },
    Refused(String),
}

impl Reply {
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            Self::Streaming { frame_bytes } => format!("{OK} {frame_bytes}\n"),
            Self::Refused(reason) => format!("{ERR} {}\n", reason.replace('\n', " ")),
        }
    }

    pub fn parse(line: &str) -> Result<Self, Error> {
        let line = line.trim();
        let bad = || Error::Line(line.to_string());
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            OK => Ok(Self::Streaming {
                frame_bytes: rest.trim().parse().map_err(|_| bad())?,
            }),
            ERR => Ok(Self::Refused(rest.trim().to_string())),
            _ => Err(bad()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_survives_the_wire() {
        let header = Header {
            frame_bytes: 8,
            index: 0x0123_4567_89ab_cdef,
            board_lost: 42,
        };
        assert_eq!(Header::decode(&header.encode()), Ok(header));
    }

    #[test]
    fn a_foreign_or_short_datagram_is_refused() {
        assert_eq!(Header::decode(&[0; 10]), Err(Error::Short(10)));
        assert_eq!(Header::decode(&[0; HEADER_BYTES]), Err(Error::Magic));
        let mut newer = Header::default().encode();
        newer[4] = 9;
        assert_eq!(Header::decode(&newer), Err(Error::Version(9)));
    }

    #[test]
    fn a_datagram_carries_whole_frames_within_the_mtu() {
        assert_eq!(frames_per_datagram(4), 362);
        assert_eq!(frames_per_datagram(8), 181);
        assert!(HEADER_BYTES + frames_per_datagram(4) * 4 <= DATAGRAM_BYTES);
        assert_eq!(frames_per_datagram(0), 0);
    }

    #[test]
    fn a_start_request_survives_the_wire() {
        let start = Start {
            device: "cf-ad9361-lpc".to_string(),
            elements: vec![0, 1, 2, 3],
            buffer_frames: 614_400,
            udp_port: 50_123,
        };
        assert_eq!(start.line(), "START cf-ad9361-lpc 0,1,2,3 614400 50123\n");
        assert_eq!(Start::parse(&start.line()), Ok(start));
    }

    #[test]
    fn a_start_request_missing_parts_is_refused() {
        for line in [
            "",
            "STOP x 0 1 2",
            "START cf 0,1 100",
            "START cf a,b 100 5",
            "START cf 0,1 0 5",
            "START cf 0,1 100 70000",
            "START cf 0,1 100 5 extra",
        ] {
            assert!(Start::parse(line).is_err(), "{line}");
        }
    }

    #[test]
    fn replies_survive_the_wire() {
        for reply in [
            Reply::Streaming { frame_bytes: 4 },
            Reply::Refused("the buffer is busy".to_string()),
        ] {
            assert_eq!(Reply::parse(&reply.line()), Ok(reply));
        }
        assert_eq!(
            Reply::Refused("two\nlines".to_string()).line(),
            "ERR two lines\n"
        );
        assert!(Reply::parse("HELLO").is_err());
    }
}
