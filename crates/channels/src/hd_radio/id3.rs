//! The ID3v2.3 tags program service data carries: what is playing.

const HEADER: usize = 10;

/// The title and artist a PSD frame names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
}

impl Tags {
    /// "Artist - Title", or whichever of the two is there.
    pub fn now_playing(&self) -> Option<String> {
        match (self.artist.as_deref(), self.title.as_deref()) {
            (Some(artist), Some(title)) if !artist.is_empty() && !title.is_empty() => {
                Some(format!("{artist} - {title}"))
            }
            (_, Some(title)) if !title.is_empty() => Some(title.to_owned()),
            (Some(artist), _) if !artist.is_empty() => Some(artist.to_owned()),
            _ => None,
        }
    }
}

pub fn parse(bytes: &[u8]) -> Option<Tags> {
    if bytes.len() < HEADER || &bytes[..5] != b"ID3\x03\x00" || bytes[5] != 0 {
        return None;
    }
    let size = bytes[6..10]
        .iter()
        .fold(0usize, |acc, &b| acc << 7 | usize::from(b & 0x7f))
        + HEADER;
    let end = size.min(bytes.len());
    let mut tags = Tags::default();
    let mut at = HEADER;
    while at + HEADER <= end {
        let id = &bytes[at..at + 4];
        let len = bytes[at + 4..at + 8]
            .iter()
            .fold(0usize, |acc, &b| acc << 8 | usize::from(b));
        let body_start = at + HEADER;
        if body_start + len > end {
            break;
        }
        let body = &bytes[body_start..body_start + len];
        match id {
            b"TIT2" => tags.title = Some(text(body)),
            b"TPE1" => tags.artist = Some(text(body)),
            _ => {}
        }
        at = body_start + len;
    }
    Some(tags)
}

fn text(body: &[u8]) -> String {
    let Some((&encoding, rest)) = body.split_first() else {
        return String::new();
    };
    let decoded = if encoding == 1 {
        ucs2(rest)
    } else {
        rest.iter().map(|&b| char::from(b)).collect()
    };
    decoded.trim_end_matches('\0').trim().to_owned()
}

/// UCS-2 text, big endian when its byte order mark says so and little endian otherwise.
pub fn ucs2(bytes: &[u8]) -> String {
    let (big_endian, body) = match bytes {
        [0xfe, 0xff, rest @ ..] => (true, rest),
        [0xff, 0xfe, rest @ ..] => (false, rest),
        _ => (false, bytes),
    };
    let units = body.as_chunks::<2>().0.iter().map(|&pair| {
        if big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        }
    });
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(id: [u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((body.len() as u32).to_be_bytes());
        out.extend([0, 0]);
        out.extend(body);
        out
    }

    #[test]
    fn title_and_artist_are_read_from_their_frames() {
        let mut frames = frame(*b"TIT2", b"\0Song");
        frames.extend(frame(*b"TPE1", b"\x01\xff\xfeB\0a\0n\0d\0"));
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        tag.extend([0, 0, 0, frames.len() as u8]);
        tag.extend(frames);
        let tags = parse(&tag).expect("an ID3 tag");
        assert_eq!(tags.now_playing().as_deref(), Some("Band - Song"));
    }

    #[test]
    fn other_data_is_not_a_tag() {
        assert_eq!(parse(b"not an id3 tag"), None);
    }
}
