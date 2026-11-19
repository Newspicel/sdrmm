//! The `JS8Call` message layer: what the 72 bits and three flag bits of one frame say. Translated
//! from `JS8Call`'s `varicode.cpp` and `decodedtext.cpp`.

const FRAME_ALPHABET: &[u8; 64] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-+";
const CALL_ALPHABET: &[u8; 39] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ /@";
const FRAME_CHARS: usize = 12;
const FLAG_FIRST: u8 = 1;
const FLAG_LAST: u8 = 2;
const FLAG_DATA: u8 = 4;
const BASE_CALLS: u32 = 37 * 36 * 10 * 27 * 27 * 27;
const BASE_GRIDS: u16 = 180 * 180;
const USER_GRIDS: u16 = BASE_GRIDS + 10;
const MAX_GRID: u16 = (1 << 15) - 1;
const SNR_BIAS: i32 = 31;

const GROUPS: [&str; 54] = [
    "<....>",
    "@ALLCALL",
    "@JS8NET",
    "@DX/NA",
    "@DX/SA",
    "@DX/EU",
    "@DX/AS",
    "@DX/AF",
    "@DX/OC",
    "@DX/AN",
    "@REGION/1",
    "@REGION/2",
    "@REGION/3",
    "@GROUP/0",
    "@GROUP/1",
    "@GROUP/2",
    "@GROUP/3",
    "@GROUP/4",
    "@GROUP/5",
    "@GROUP/6",
    "@GROUP/7",
    "@GROUP/8",
    "@GROUP/9",
    "@COMMAND",
    "@CONTROL",
    "@NET",
    "@NTS",
    "@RESERVE/0",
    "@RESERVE/1",
    "@RESERVE/2",
    "@RESERVE/3",
    "@RESERVE/4",
    "@APRSIS",
    "@RAGCHEW",
    "@JS8",
    "@EMCOMM",
    "@ARES",
    "@MARS",
    "@AMRRON",
    "@RACES",
    "@RAYNET",
    "@RADAR",
    "@SKYWARN",
    "@CQ",
    "@HB",
    "@QSO",
    "@QSOPARTY",
    "@CONTEST",
    "@FIELDDAY",
    "@SOTA",
    "@IOTA",
    "@POTA",
    "@QRP",
    "@QRO",
];

const CQS: [&str; 8] = [
    "CQ CQ CQ",
    "CQ DX",
    "CQ QRP",
    "CQ CONTEST",
    "CQ FIELD",
    "CQ FD",
    "CQ CQ",
    "CQ",
];

/// Directed commands by number, as `JS8Call`'s command map reads back (its first key per value).
const COMMANDS: [&str; 32] = [
    " SNR?",
    " DIT DIT",
    " NACK",
    " HEARING?",
    " GRID?",
    ">",
    " STATUS?",
    " STATUS",
    " HEARING",
    " MSG",
    " MSG TO:",
    " QUERY",
    " QUERY MSGS",
    " QUERY CALL",
    " ACK",
    " GRID",
    " INFO?",
    " INFO",
    " FB",
    " HW CPY?",
    " SK",
    " RR",
    " QSL?",
    " QSL",
    " CMD",
    " SNR",
    " NO",
    " YES",
    " 73",
    " HEARTBEAT SNR",
    " AGN?",
    " ",
];
const SNR_COMMANDS: [usize; 2] = [25, 29];

const HUFFMAN: [(char, &str); 44] = [
    (' ', "01"),
    ('E', "100"),
    ('T', "1101"),
    ('A', "0011"),
    ('O', "11111"),
    ('I', "11100"),
    ('N', "10111"),
    ('S', "10100"),
    ('H', "00011"),
    ('R', "00000"),
    ('D', "111011"),
    ('L', "110011"),
    ('C', "110001"),
    ('U', "101101"),
    ('M', "101011"),
    ('W', "001011"),
    ('F', "001001"),
    ('G', "000101"),
    ('Y', "000011"),
    ('P', "1111011"),
    ('B', "1111001"),
    ('.', "1110100"),
    ('V', "1100101"),
    ('K', "1100100"),
    ('-', "1100001"),
    ('+', "1100000"),
    ('?', "1011001"),
    ('!', "1011000"),
    ('"', "1010101"),
    ('X', "1010100"),
    ('0', "0010101"),
    ('J', "0010100"),
    ('1', "0010001"),
    ('Q', "0010000"),
    ('2', "0001001"),
    ('Z', "0001000"),
    ('3', "0000101"),
    ('5', "0000100"),
    ('4', "11110101"),
    ('9', "11110100"),
    ('8', "11110001"),
    ('6', "11110000"),
    ('7', "11101011"),
    ('/', "11101010"),
];

/// What a frame is, by the type bits at its head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameType {
    Heartbeat,
    Compound,
    CompoundDirected,
    Directed,
    Data,
}

impl FrameType {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Heartbeat => "Heartbeat",
            Self::Compound => "Compound",
            Self::CompoundDirected => "Compound directed",
            Self::Directed => "Directed",
            Self::Data => "Data",
        }
    }
}

/// One decoded frame: its 72 bits as the 12 characters `JS8Call` shows, its flags, and what they
/// say.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Frame {
    pub(crate) raw: String,
    pub(crate) first: bool,
    pub(crate) last: bool,
    pub(crate) kind: Option<FrameType>,
    /// The readable text, when the frame can be read without `JS8Call`'s word dictionary.
    pub(crate) text: Option<String>,
    pub(crate) callsign: Option<String>,
    pub(crate) to: Option<String>,
    pub(crate) grid: Option<String>,
}

impl Frame {
    /// The frame a 75-bit payload carries: 72 message bits, then the three flag bits.
    pub(crate) fn read(payload: u128) -> Self {
        let value = (payload >> 3) & ((1 << 72) - 1);
        let flags = (payload & 7) as u8;
        let raw = (0..FRAME_CHARS)
            .map(|index| {
                let word = (value >> (6 * (FRAME_CHARS - 1 - index))) as usize & 63;
                char::from(FRAME_ALPHABET[word])
            })
            .collect();
        let mut frame = Self {
            raw,
            first: flags & FLAG_FIRST != 0,
            last: flags & FLAG_LAST != 0,
            kind: None,
            text: None,
            callsign: None,
            to: None,
            grid: None,
        };
        let bits = Bits(value);
        if flags & FLAG_DATA != 0 {
            frame.kind = Some(FrameType::Data);
        } else if !(frame.data(bits) || frame.heartbeat(bits) || frame.compound(bits)) {
            frame.directed(bits);
        }
        frame
    }

    fn data(&mut self, bits: Bits) -> bool {
        if bits.field(0, 1) == 0 {
            return false;
        }
        if bits.field(1, 1) == 1 {
            self.kind = Some(FrameType::Data);
            return true;
        }
        let Some(pad) = (2..72).rev().find(|&index| bits.field(index, 1) == 0) else {
            return false;
        };
        let text = huffman(bits, 2, pad);
        if text.is_empty() {
            return false;
        }
        self.kind = Some(FrameType::Data);
        self.text = Some(text);
        true
    }

    fn heartbeat(&mut self, bits: Bits) -> bool {
        let Some((0, callsign, extra, bits3)) = compound_frame(bits) else {
            return false;
        };
        let grid = grid(extra & MAX_GRID);
        let what = if extra & (1 << 15) != 0 {
            format!("@ALLCALL {}", CQS[usize::from(bits3)])
        } else {
            "@HB HEARTBEAT".to_owned()
        };
        let text = [
            format!("{callsign}: {what}"),
            grid.clone().unwrap_or_default(),
        ]
        .join(" ")
        .trim_end()
        .to_owned();
        self.kind = Some(FrameType::Heartbeat);
        self.text = Some(text);
        self.callsign = Some(callsign);
        self.grid = grid;
        true
    }

    fn compound(&mut self, bits: Bits) -> bool {
        let Some((kind @ (1 | 2), callsign, extra, _)) = compound_frame(bits) else {
            return false;
        };
        let mut words = Vec::new();
        if extra <= BASE_GRIDS {
            self.grid = grid(extra);
            words.extend(self.grid.as_ref().map(|square| format!(" {square}")));
        } else if (USER_GRIDS..MAX_GRID).contains(&extra) {
            let (command, number) = command(extra - USER_GRIDS);
            words.push(COMMANDS[command].to_owned());
            if SNR_COMMANDS.contains(&command) {
                words.push(snr(i32::from(number) - SNR_BIAS));
            }
        }
        let text = if kind == 1 {
            self.kind = Some(FrameType::Compound);
            format!("{callsign}:{}", words.concat())
        } else {
            self.kind = Some(FrameType::CompoundDirected);
            format!("{callsign}{}", words.join(" "))
        };
        self.text = Some(text.trim_end().to_owned());
        self.callsign = Some(callsign);
        true
    }

    fn directed(&mut self, bits: Bits) {
        if bits.field(0, 3) != 3 {
            return;
        }
        let extra = bits.field(64, 8) as u8;
        let from = callsign(bits.field(3, 28) as u32, extra >> 7 & 1 == 1);
        let to = callsign(bits.field(31, 28) as u32, extra >> 6 & 1 == 1);
        let (Some(from), Some(to)) = (from, to) else {
            return;
        };
        let command = bits.field(59, 5) as usize;
        let number = i32::from(extra % 64);
        let mut text = format!("{from}: {to}{}", COMMANDS[command]);
        if number != 0 {
            let value = number - SNR_BIAS;
            let shown = if SNR_COMMANDS.contains(&command) {
                snr(value)
            } else {
                value.to_string()
            };
            text = format!("{text} {shown}");
        }
        self.kind = Some(FrameType::Directed);
        self.text = Some(text.trim_end().to_owned());
        self.callsign = Some(from);
        self.to = Some(to);
    }
}

/// The 72 message bits, read from the first sent.
#[derive(Clone, Copy)]
struct Bits(u128);

impl Bits {
    fn field(self, start: usize, width: usize) -> u64 {
        (self.0 >> (72 - start - width)) as u64 & ((1u64 << width) - 1)
    }
}

/// A compound frame's type, callsign, 16-bit extra and three spare bits.
fn compound_frame(bits: Bits) -> Option<(u64, String, u16, u8)> {
    let kind = bits.field(0, 3);
    if kind >= 3 {
        return None;
    }
    let callsign = alphanumeric50(bits.field(3, 50))?;
    let extra = ((bits.field(53, 11) << 5) | bits.field(64, 5)) as u16;
    Some((kind, callsign, extra, bits.field(69, 3) as u8))
}

fn huffman(bits: Bits, start: usize, end: usize) -> String {
    let mut text = String::new();
    let mut at = start;
    while at < end {
        let next = HUFFMAN.iter().find(|(_, code)| {
            at + code.len() <= end
                && code
                    .bytes()
                    .enumerate()
                    .all(|(offset, digit)| bits.field(at + offset, 1) == u64::from(digit - b'0'))
        });
        let Some((symbol, code)) = next else {
            break;
        };
        text.push(*symbol);
        at += code.len();
    }
    text
}

fn alphanumeric50(mut packed: u64) -> Option<String> {
    let mut word = [b' '; 11];
    for (index, radix) in [
        (10, 38),
        (9, 38),
        (8, 38),
        (7, 2),
        (6, 38),
        (5, 38),
        (4, 38),
        (3, 2),
        (2, 38),
        (1, 38),
        (0, 39),
    ] {
        let digit = (packed % radix) as usize;
        packed /= radix;
        word[index] = if radix == 2 {
            if digit == 1 { b'/' } else { b' ' }
        } else {
            CALL_ALPHABET[digit]
        };
    }
    let text: String = word
        .iter()
        .filter(|&&byte| byte != b' ')
        .map(|&byte| char::from(byte))
        .collect();
    (!text.is_empty()).then_some(text)
}

fn callsign(mut value: u32, portable: bool) -> Option<String> {
    if let Some(group) = value
        .checked_sub(BASE_CALLS + 1)
        .and_then(|index| GROUPS.get(index as usize))
    {
        return Some((*group).to_owned());
    }
    let mut word = [b' '; 6];
    for (index, radix, offset) in [
        (5, 27, 10),
        (4, 27, 10),
        (3, 27, 10),
        (2, 10, 0),
        (1, 36, 0),
    ] {
        word[index] = CALL_ALPHABET[(value % radix + offset) as usize];
        value /= radix;
    }
    word[0] = *CALL_ALPHABET.get(value as usize)?;
    let mut call: String = String::from_utf8_lossy(&word).trim().to_owned();
    if let Some(rest) = call.strip_prefix("3D0") {
        call = format!("3DA0{rest}");
    }
    if call.starts_with('Q') && call[1..].starts_with(|c: char| c.is_ascii_uppercase()) {
        call = format!("3X{}", &call[1..]);
    }
    if portable {
        call.push_str("/P");
    }
    (!call.is_empty()).then_some(call)
}

fn command(value: u16) -> (usize, u8) {
    if value & (1 << 7) != 0 {
        let number = (value & 63) as u8;
        let command = if value & (1 << 6) != 0 { 29 } else { 25 };
        (command, number)
    } else {
        (usize::from(value & 31), 0)
    }
}

/// A Maidenhead square from `JS8Call`'s 15-bit grid number.
fn grid(value: u16) -> Option<String> {
    if value > BASE_GRIDS {
        return None;
    }
    let latitude = f32::from(value % 180) - 90.0;
    let mut longitude = f32::from(value / 180) * 2.0 - 180.0 + 2.0;
    if longitude < -180.0 {
        longitude += 360.0;
    }
    if longitude > 180.0 {
        longitude -= 360.0;
    }
    let east = (60.0 * (180.0 - longitude) / 5.0) as u32;
    let north = (60.0 * (latitude + 90.0) / 2.5) as u32;
    let letter = |value: u32| char::from(b'A' + (value / 240) as u8);
    let digit = |value: u32| char::from(b'0' + ((value % 240) / 24) as u8);
    Some(
        [letter(east), letter(north), digit(east), digit(north)]
            .iter()
            .collect(),
    )
}

fn snr(value: i32) -> String {
    if value >= 0 {
        format!("+{value:02}")
    } else {
        format!("-{:02}", -value)
    }
}

#[cfg(test)]
pub(crate) mod pack {
    //! Builds frames the way `JS8Call`'s packers do, for tests and the synthetic slot.

    use super::{BASE_GRIDS, CALL_ALPHABET};

    fn index(byte: u8) -> u64 {
        CALL_ALPHABET.iter().position(|&c| c == byte).unwrap_or(36) as u64
    }

    pub(crate) fn alphanumeric50(call: &str) -> u64 {
        let mut word: Vec<u8> = call.bytes().collect();
        if word.len() > 3 && word[3] != b'/' {
            word.insert(3, b' ');
        }
        if word.len() > 7 && word[7] != b'/' {
            word.insert(7, b' ');
        }
        word.resize(11, b' ');
        word.iter().enumerate().fold(0, |acc, (position, &byte)| {
            if position == 3 || position == 7 {
                acc * 2 + u64::from(byte == b'/')
            } else if position == 0 {
                index(byte)
            } else {
                acc * 38 + index(byte)
            }
        })
    }

    pub(crate) fn grid(square: &str) -> u16 {
        let g = square.as_bytes();
        let longitude = 180.0
            - 20.0 * f32::from(g[0] - b'A')
            - 2.0 * f32::from(g[2] - b'0')
            - 5.0 * 12.5 / 60.0;
        let latitude =
            -90.0 + 10.0 * f32::from(g[1] - b'A') + f32::from(g[3] - b'0') + 2.5 * 12.5 / 60.0;
        let value = i32::midpoint(longitude as i32, 180) * 180 + (latitude + 90.0) as i32;
        u16::try_from(value).unwrap_or(BASE_GRIDS + 1)
    }

    /// A heartbeat (or, with `cq`, a CQ) from `call` in `square`, as a 75-bit payload.
    pub(crate) fn heartbeat(call: &str, square: &str, cq: bool, flags: u8) -> u128 {
        let extra = grid(square) | if cq { 1 << 15 } else { 0 };
        let value = (u128::from(alphanumeric50(call)) << 19)
            | (u128::from(extra >> 5) << 8)
            | (u128::from(extra & 31) << 3)
            | if cq { 7 } else { 0 };
        (value << 3) | u128::from(flags)
    }

    pub(crate) fn callsign(call: &str) -> u32 {
        let shifted = if call.as_bytes().get(2).is_some_and(u8::is_ascii_digit) {
            call.to_owned()
        } else {
            format!(" {call}")
        };
        let padded = format!("{shifted:<6}");
        let b = padded.as_bytes();
        let at = |i: usize| {
            super::CALL_ALPHABET
                .iter()
                .position(|&c| c == b[i])
                .unwrap_or(36) as u32
        };
        let mut packed = at(0);
        packed = 36 * packed + at(1);
        packed = 10 * packed + at(2);
        for i in 3..6 {
            packed = 27 * packed + at(i) - 10;
        }
        packed
    }

    /// A directed frame from `from` to `to` carrying command `command` and `number`.
    pub(crate) fn directed(from: &str, to: &str, command: u8, number: u8, flags: u8) -> u128 {
        let value = (3u128 << 69)
            | (u128::from(callsign(from)) << 41)
            | (u128::from(callsign(to)) << 13)
            | (u128::from(command) << 8)
            | u128::from(number);
        (value << 3) | u128::from(flags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heartbeat_names_its_station_and_square() {
        let frame = Frame::read(pack::heartbeat(
            "KN4CRD",
            "EM73",
            false,
            FLAG_FIRST | FLAG_LAST,
        ));
        assert_eq!(frame.kind, Some(FrameType::Heartbeat));
        assert_eq!(frame.text.as_deref(), Some("KN4CRD: @HB HEARTBEAT EM73"));
        assert_eq!(frame.callsign.as_deref(), Some("KN4CRD"));
        assert_eq!(frame.grid.as_deref(), Some("EM73"));
        assert!(frame.first && frame.last);
        assert_eq!(frame.raw.len(), 12);
    }

    #[test]
    fn a_cq_reads_as_an_allcall() {
        let frame = Frame::read(pack::heartbeat("W1AW/P", "FN31", true, 0));
        assert_eq!(frame.text.as_deref(), Some("W1AW/P: @ALLCALL CQ FN31"));
    }

    #[test]
    fn a_directed_snr_report_reads_like_js8call() {
        let frame = Frame::read(pack::directed("KN4CRD", "K1ABC", 25, 31 - 12, 0));
        assert_eq!(frame.kind, Some(FrameType::Directed));
        assert_eq!(frame.text.as_deref(), Some("KN4CRD: K1ABC SNR -12"));
        assert_eq!(frame.to.as_deref(), Some("K1ABC"));
        let query = Frame::read(pack::directed("K1ABC", "KN4CRD", 4, 0, 0));
        assert_eq!(query.text.as_deref(), Some("K1ABC: KN4CRD GRID?"));
    }

    #[test]
    fn groups_unpack_by_name() {
        assert_eq!(callsign(BASE_CALLS + 2, false).as_deref(), Some("@ALLCALL"));
        assert_eq!(callsign(BASE_CALLS + 54, false).as_deref(), Some("@QRO"));
    }

    #[test]
    fn huffman_text_frames_read_back() {
        let mut stream = String::from("10");
        for character in "HELLO WORLD".chars() {
            let code = HUFFMAN
                .iter()
                .find(|(symbol, _)| *symbol == character)
                .unwrap()
                .1;
            stream.push_str(code);
        }
        stream.push('0');
        while stream.len() < 72 {
            stream.push('1');
        }
        let value = u128::from_str_radix(&stream, 2).unwrap();
        let frame = Frame::read(value << 3);
        assert_eq!(frame.kind, Some(FrameType::Data));
        assert_eq!(frame.text.as_deref(), Some("HELLO WORLD"));
    }

    #[test]
    fn data_flagged_frames_keep_their_raw_characters() {
        let frame = Frame::read((0x0012_3456_789a_bcde_f012_u128 << 3) | u128::from(FLAG_DATA));
        assert_eq!(frame.kind, Some(FrameType::Data));
        assert_eq!(frame.text, None);
        assert!(frame.raw.bytes().all(|b| FRAME_ALPHABET.contains(&b)));
    }

    #[test]
    fn snr_reads_with_a_sign_and_two_digits() {
        assert_eq!(snr(-5), "-05");
        assert_eq!(snr(7), "+07");
        assert_eq!(snr(-21), "-21");
    }
}
