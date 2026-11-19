//! Station Information Service: the PIDS frames' station ID, names, slogan, message, location
//! and service list (NRSC-5 1020s).

use super::layout::PIDS_BITS;

const CRC_BITS: usize = 12;
const PIDS_DATA: usize = PIDS_BITS - CRC_BITS;
const CRC_POLY: u16 = 0xD010;
const CRC_XOR: u16 = 0x955;
const SIS_BITS: usize = PIDS_DATA - 1;
const CHARS5: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ ?-*$ ";
/// Payload bits of each message ID; `None` for the reserved ones.
const PAYLOAD: [Option<usize>; 16] = [
    Some(32),
    Some(22),
    Some(58),
    Some(32),
    Some(27),
    Some(58),
    Some(27),
    Some(22),
    Some(58),
    Some(58),
    Some(27),
    None,
    None,
    None,
    None,
    None,
];
const LONG_NAME_FRAMES: usize = 8;
const LONG_NAME_CHARS: usize = 7;
const SHORT_NAME_FRAMES: usize = 2;
const SLOGAN_FRAMES: usize = 16;
const MESSAGE_FRAMES: usize = 32;
const PROGRAMS: usize = 8;
const UCS2: u8 = 4;

/// One audio program the station lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Program {
    pub restricted: bool,
    pub kind: u8,
}

/// A text sent over several SIS frames, kept until every frame of one sequence is in.
#[derive(Clone, Debug)]
struct Pieces<const FRAMES: usize, const BYTES: usize> {
    bytes: [u8; BYTES],
    have: [bool; FRAMES],
    sequence: Option<u8>,
    length: usize,
    encoding: u8,
    done: bool,
}

impl<const FRAMES: usize, const BYTES: usize> Default for Pieces<FRAMES, BYTES> {
    fn default() -> Self {
        Self {
            bytes: [0; BYTES],
            have: [false; FRAMES],
            sequence: None,
            length: 0,
            encoding: 0,
            done: false,
        }
    }
}

impl<const FRAMES: usize, const BYTES: usize> Pieces<FRAMES, BYTES> {
    fn restart(&mut self, sequence: Option<u8>) {
        if self.sequence != sequence || sequence.is_none() {
            self.bytes = [0; BYTES];
            self.have = [false; FRAMES];
            self.done = false;
        }
        self.sequence = sequence;
    }

    fn put(&mut self, frame: usize, at: usize, bytes: impl IntoIterator<Item = u8>) {
        if frame >= FRAMES {
            return;
        }
        for (slot, byte) in self.bytes.iter_mut().skip(at).zip(bytes) {
            *slot = byte;
        }
        self.have[frame] = true;
    }

    /// True once, when the first `frames` frames are all in.
    fn complete(&mut self, frames: usize) -> bool {
        let complete =
            frames > 0 && frames <= FRAMES && self.have[..frames].iter().all(|&have| have);
        let fresh = complete && !self.done;
        self.done |= complete;
        fresh
    }

    fn text(&self, length: usize) -> String {
        decode_text(self.encoding, &self.bytes[..length.min(BYTES)])
    }
}

/// What the station has said about itself so far.
#[derive(Clone, Debug, Default)]
pub struct Station {
    pub country: Option<String>,
    pub facility_id: Option<u32>,
    pub call_sign: Option<String>,
    pub name: Option<String>,
    pub long_name: Option<String>,
    pub slogan: Option<String>,
    pub message: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude_m: u32,
    pub programs: [Option<Program>; PROGRAMS],
    pub frames_ok: u32,
    pub frames_bad: u32,
    long: Pieces<LONG_NAME_FRAMES, { LONG_NAME_FRAMES * LONG_NAME_CHARS }>,
    short: Pieces<SHORT_NAME_FRAMES, 12>,
    short_append: bool,
    slogan_pieces: Pieces<SLOGAN_FRAMES, 96>,
    message_pieces: Pieces<MESSAGE_FRAMES, 192>,
    message_checksum: u8,
}

/// Reads `bits` most significant first.
struct Bits<'a> {
    bits: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn take(&mut self, count: usize) -> u32 {
        let value = self.bits[self.at..self.at + count]
            .iter()
            .fold(0u32, |acc, &bit| acc << 1 | u32::from(bit));
        self.at += count;
        value
    }

    fn flag(&mut self) -> bool {
        self.take(1) == 1
    }

    fn byte(&mut self) -> u8 {
        self.take(8) as u8
    }

    fn signed(&mut self, count: usize) -> i32 {
        let value = self.take(count) as i32;
        if value & (1 << (count - 1)) != 0 {
            value - (1 << count)
        } else {
            value
        }
    }

    fn char5(&mut self) -> char {
        char::from(CHARS5[self.take(5) as usize])
    }
}

/// The PIDS frame's bits in transmission order: each byte arrives least significant first.
pub fn reorder(decoded: &[u8; PIDS_BITS]) -> [u8; PIDS_BITS] {
    std::array::from_fn(|i| decoded[(i & !7) + 7 - (i & 7)])
}

/// The frame's CRC-12 over its first 68 bits, as NRSC-5 1017s defines it.
pub fn crc_ok(bits: &[u8; PIDS_BITS]) -> bool {
    let mut register = 0u16;
    let mut shift = |input: u16| {
        let low = register & 1;
        register = register >> 1 ^ input << 15;
        if low == 1 {
            register ^= CRC_POLY;
        }
    };
    for &bit in bits[..PIDS_DATA].iter().rev() {
        shift(u16::from(bit));
    }
    for _ in 0..16 {
        shift(0);
    }
    let expected = bits[PIDS_DATA..]
        .iter()
        .fold(0u16, |acc, &bit| acc << 1 | u16::from(bit));
    (register ^ CRC_XOR) & 0xfff == expected
}

impl Station {
    /// Reads one decoded PIDS frame; returns whether it passed its CRC.
    pub fn push(&mut self, decoded: &[u8; PIDS_BITS]) -> bool {
        let bits = reorder(decoded);
        if !crc_ok(&bits) {
            self.frames_bad = self.frames_bad.saturating_add(1);
            return false;
        }
        self.frames_ok = self.frames_ok.saturating_add(1);
        if bits[0] == 0 {
            self.read_sis(&bits[1..=SIS_BITS]);
        }
        true
    }

    fn read_sis(&mut self, bits: &[u8]) {
        let mut reader = Bits { bits, at: 0 };
        let payloads = usize::from(reader.flag()) + 1;
        for _ in 0..payloads {
            if reader.at + 4 > bits.len() {
                break;
            }
            let id = reader.take(4) as usize;
            let Some(size) = PAYLOAD[id] else { break };
            if reader.at + size > bits.len() {
                break;
            }
            let payload = &bits[reader.at..reader.at + size];
            reader.at += size;
            self.read_message(id, payload);
        }
    }

    fn read_message(&mut self, id: usize, payload: &[u8]) {
        let mut bits = Bits {
            bits: payload,
            at: 0,
        };
        match id {
            0 => self.read_station_id(&mut bits),
            1 => self.read_short_name(&mut bits),
            2 => self.read_long_name(&mut bits),
            4 => self.read_location(&mut bits),
            5 => self.read_message_text(&mut bits),
            6 | 10 => self.read_service(&mut bits),
            8 => self.read_universal_name(&mut bits),
            _ => {}
        }
    }

    fn read_station_id(&mut self, bits: &mut Bits<'_>) {
        let country = [bits.char5(), bits.char5()];
        bits.take(3);
        replace(&mut self.country, &country, "");
        self.facility_id = Some(bits.take(19));
    }

    fn read_short_name(&mut self, bits: &mut Bits<'_>) {
        let letters: [char; 4] = std::array::from_fn(|_| bits.char5());
        let length = letters
            .iter()
            .rposition(|&c| c != ' ')
            .map_or(0, |at| at + 1);
        let suffix = if bits.take(2) == 1 { "-FM" } else { "" };
        replace(&mut self.call_sign, &letters[..length], suffix);
    }

    fn read_long_name(&mut self, bits: &mut Bits<'_>) {
        let last = bits.take(3) as usize;
        let frame = bits.take(3) as usize;
        let sequence = payload_sequence(bits.bits);
        if frame == 0 {
            self.long.restart(Some(sequence));
        }
        let chars: [u8; LONG_NAME_CHARS] = std::array::from_fn(|_| bits.take(7) as u8);
        self.long.put(frame, frame * LONG_NAME_CHARS, chars);
        if self.long.sequence.is_some() && self.long.complete(last + 1) {
            let length = self
                .long
                .bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(self.long.bytes.len());
            self.long_name = Some(self.long.text(length));
        }
    }

    fn read_location(&mut self, bits: &mut Bits<'_>) {
        let latitude = bits.flag();
        let degrees = f64::from(bits.signed(22)) / 8192.0;
        let altitude = bits.take(4);
        if latitude {
            self.latitude = Some(degrees);
            self.altitude_m = (self.altitude_m & 0x0f0) | altitude << 8;
        } else {
            self.longitude = Some(degrees);
            self.altitude_m = (self.altitude_m & 0xf00) | altitude << 4;
        }
    }

    fn read_message_text(&mut self, bits: &mut Bits<'_>) {
        let frame = bits.take(5) as usize;
        let sequence = bits.take(2) as u8;
        if frame == 0 {
            self.message_pieces.restart(Some(sequence));
            bits.take(1);
            self.message_pieces.encoding = bits.take(3) as u8;
            self.message_pieces.length = bits.take(8) as usize;
            self.message_checksum = bits.take(7) as u8;
            let bytes: [u8; 4] = std::array::from_fn(|_| bits.byte());
            self.message_pieces.put(0, 0, bytes);
        } else {
            bits.take(3);
            let bytes: [u8; 6] = std::array::from_fn(|_| bits.byte());
            self.message_pieces.put(frame, frame * 6 - 2, bytes);
        }
        let pieces = &mut self.message_pieces;
        let frames = (pieces.length + 7) / 6;
        if pieces.sequence.is_some() && pieces.complete(frames) {
            let sum: u32 = pieces.bytes[..pieces.length.min(pieces.bytes.len())]
                .iter()
                .map(|&b| u32::from(b))
                .sum();
            let checksum = (((sum >> 8) & 0x7f) + (sum & 0xff)) & 0x7f;
            if checksum == u32::from(self.message_checksum) {
                self.message = Some(pieces.text(pieces.length));
            } else {
                pieces.done = false;
            }
        }
    }

    fn read_service(&mut self, bits: &mut Bits<'_>) {
        if bits.take(2) != 0 {
            return;
        }
        let restricted = bits.flag();
        let program = bits.take(6) as usize;
        let kind = bits.take(8) as u8;
        if let Some(slot) = self.programs.get_mut(program) {
            *slot = Some(Program { restricted, kind });
        }
    }

    fn read_universal_name(&mut self, bits: &mut Bits<'_>) {
        let frame = bits.take(4) as usize;
        if bits.flag() {
            self.read_slogan(bits, frame);
            return;
        }
        if frame == 0 {
            self.short.encoding = bits.take(3) as u8;
            self.short_append = bits.flag();
            self.short.length = usize::from(bits.flag()) + 1;
            let bytes: [u8; 6] = std::array::from_fn(|_| bits.byte());
            self.short.put(0, 0, bytes);
        } else {
            bits.take(5);
            let bytes: [u8; 6] = std::array::from_fn(|_| bits.byte());
            self.short.put(frame, frame * 6, bytes);
        }
        if self.short.length > 0 && self.short.complete(self.short.length) {
            let length = self.short.bytes.iter().position(|&b| b == 0).unwrap_or(12);
            let mut name = self.short.text(length);
            if self.short_append {
                name.push_str("-FM");
            }
            self.name = Some(name);
        }
    }

    fn read_slogan(&mut self, bits: &mut Bits<'_>, frame: usize) {
        let pieces = &mut self.slogan_pieces;
        if frame == 0 {
            pieces.encoding = bits.take(3) as u8;
            bits.take(3);
            pieces.length = bits.take(7) as usize;
            let bytes: [u8; 5] = std::array::from_fn(|_| bits.byte());
            pieces.put(0, 0, bytes);
        } else {
            bits.take(5);
            let bytes: [u8; 6] = std::array::from_fn(|_| bits.byte());
            pieces.put(frame, frame * 6 - 1, bytes);
        }
        if pieces.length > 0 && pieces.complete((pieces.length + 6) / 6) {
            self.slogan = Some(pieces.text(pieces.length));
        }
    }

    /// The name to show: the universal name, the call sign, then the long name.
    pub fn display_name(&self) -> Option<&str> {
        self.name
            .as_deref()
            .or(self.call_sign.as_deref())
            .or(self.long_name.as_deref())
    }
}

/// Sets `slot` to `chars` and `suffix`, allocating only when that differs from what it holds.
fn replace(slot: &mut Option<String>, chars: &[char], suffix: &str) {
    let same = slot
        .as_deref()
        .is_some_and(|held| held.chars().eq(chars.iter().copied().chain(suffix.chars())));
    if !same {
        *slot = Some(chars.iter().copied().chain(suffix.chars()).collect());
    }
}

/// The long name's sequence number, three bits at the payload's end.
fn payload_sequence(payload: &[u8]) -> u8 {
    payload[55..58].iter().fold(0u8, |acc, &bit| acc << 1 | bit)
}

fn decode_text(encoding: u8, bytes: &[u8]) -> String {
    let text: String = if encoding == UCS2 {
        super::id3::ucs2(bytes)
    } else {
        bytes.iter().map(|&b| char::from(b)).collect()
    };
    text.trim_end_matches(['\0', ' ']).to_owned()
}

#[cfg(test)]
mod tests;
