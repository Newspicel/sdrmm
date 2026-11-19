//! The 72-bit message JT65 and JT9 carry: two 28-bit callsign fields and a 16-bit grid, report
//! or free-text field, sent as twelve 6-bit words.

pub(crate) const WORDS: usize = 12;
pub(crate) type Words = [u8; WORDS];

const NBASE: u32 = 37 * 36 * 10 * 27 * 27 * 27;
const NGBASE: u32 = 180 * 180;
const TEXT_FLAG: u32 = 32_768;
const CQ_PREFIX: u32 = 262_178_563;
const QRZ_PREFIX: u32 = 264_002_072;
const DE_PREFIX: u32 = 265_825_581;
const CQ_SUFFIX: u32 = 267_649_090;
const QRZ_SUFFIX: u32 = 267_698_375;
const DE_SUFFIX: u32 = 267_747_660;
const DE_ALONE: u32 = 267_796_945;
const A37: &[u8; 37] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ ";
const A42: &[u8; 42] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ +-./?";
const SUFFIXES: [&str; 12] = ["P", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A"];
const PREFIXES: [&str; 339] = [
    "1A", "1S", "3A", "3B6", "3B8", "3B9", "3C", "3C0", "3D2", "3D2C", "3D2R", "3DA", "3V", "3W",
    "3X", "3Y", "3YB", "3YP", "4J", "4L", "4S", "4U1I", "4U1U", "4W", "4X", "5A", "5B", "5H", "5N",
    "5R", "5T", "5U", "5V", "5W", "5X", "5Z", "6W", "6Y", "7O", "7P", "7Q", "7X", "8P", "8Q", "8R",
    "9A", "9G", "9H", "9J", "9K", "9L", "9M2", "9M6", "9N", "9Q", "9U", "9V", "9X", "9Y", "A2",
    "A3", "A4", "A5", "A6", "A7", "A9", "AP", "BS7", "BV", "BV9", "BY", "C2", "C3", "C5", "C6",
    "C9", "CE", "CE0X", "CE0Y", "CE0Z", "CE9", "CM", "CN", "CP", "CT", "CT3", "CU", "CX", "CY0",
    "CY9", "D2", "D4", "D6", "DL", "DU", "E3", "E4", "EA", "EA6", "EA8", "EA9", "EI", "EK", "EL",
    "EP", "ER", "ES", "ET", "EU", "EX", "EY", "EZ", "F", "FG", "FH", "FJ", "FK", "FKC", "FM", "FO",
    "FOA", "FOC", "FOM", "FP", "FR", "FRG", "FRJ", "FRT", "FT5W", "FT5X", "FT5Z", "FW", "FY", "M",
    "MD", "MI", "MJ", "MM", "MU", "MW", "H4", "H40", "HA", "HB", "HB0", "HC", "HC8", "HH", "HI",
    "HK", "HK0", "HK0M", "HL", "HM", "HP", "HR", "HS", "HV", "HZ", "I", "IS", "IS0", "J2", "J3",
    "J5", "J6", "J7", "J8", "JA", "JDM", "JDO", "JT", "JW", "JX", "JY", "K", "KG4", "KH0", "KH1",
    "KH2", "KH3", "KH4", "KH5", "KH5K", "KH6", "KH7", "KH8", "KH9", "KL", "KP1", "KP2", "KP4",
    "KP5", "LA", "LU", "LX", "LY", "LZ", "OA", "OD", "OE", "OH", "OH0", "OJ0", "OK", "OM", "ON",
    "OX", "OY", "OZ", "P2", "P4", "PA", "PJ2", "PJ7", "PY", "PY0F", "PT0S", "PY0T", "PZ", "R1F",
    "R1M", "S0", "S2", "S5", "S7", "S9", "SM", "SP", "ST", "SU", "SV", "SVA", "SV5", "SV9", "T2",
    "T30", "T31", "T32", "T33", "T5", "T7", "T8", "T9", "TA", "TF", "TG", "TI", "TI9", "TJ", "TK",
    "TL", "TN", "TR", "TT", "TU", "TY", "TZ", "UA", "UA2", "UA9", "UK", "UN", "UR", "V2", "V3",
    "V4", "V5", "V6", "V7", "V8", "VE", "VK", "VK0H", "VK0M", "VK9C", "VK9L", "VK9M", "VK9N",
    "VK9W", "VK9X", "VP2E", "VP2M", "VP2V", "VP5", "VP6", "VP6D", "VP8", "VP8G", "VP8H", "VP8O",
    "VP8S", "VP9", "VQ9", "VR", "VU", "VU4", "VU7", "XE", "XF4", "XT", "XU", "XW", "XX9", "XZ",
    "YA", "YB", "YI", "YJ", "YK", "YL", "YN", "YO", "YS", "YU", "YV", "YV0", "Z2", "Z3", "ZA",
    "ZB", "ZC4", "ZD7", "ZD8", "ZD9", "ZF", "ZK1N", "ZK1S", "ZK2", "ZK3", "ZL", "ZL7", "ZL8",
    "ZL9", "ZP", "ZS", "ZS8", "KC4", "E5",
];

struct Fields {
    first: u32,
    second: u32,
    grid: u32,
}

fn fields(words: &Words) -> Fields {
    let w = words.map(u32::from);
    Fields {
        first: w[0] << 22 | w[1] << 16 | w[2] << 10 | w[3] << 4 | (w[4] >> 2 & 15),
        second: (w[4] & 3) << 26
            | w[5] << 20
            | w[6] << 14
            | w[7] << 8
            | w[8] << 2
            | (w[9] >> 4 & 3),
        grid: (w[9] & 15) << 12 | w[10] << 6 | w[11],
    }
}

/// The text of a 72-bit message, `None` when no transmitter could have packed it.
pub(crate) fn unpack(words: &Words) -> Option<String> {
    if words.iter().any(|&word| word > 63) {
        return None;
    }
    let fields = fields(words);
    let text = if fields.grid >= TEXT_FLAG {
        free_text(&fields)
    } else {
        standard(&fields)?
    };
    let text = tidy(text.trim_end().to_owned());
    (!text.is_empty() && !text.starts_with("000AAA ") && !text.starts_with("0L6MWK "))
        .then_some(text)
}

fn free_text(fields: &Fields) -> String {
    let mut first = fields.first;
    let mut second = fields.second;
    let mut third = fields.grid & (TEXT_FLAG - 1);
    if first & 1 == 1 {
        third += 32_768;
    }
    first /= 2;
    if second & 1 == 1 {
        third += 65_536;
    }
    second /= 2;
    let mut text = [b' '; 13];
    for (slots, mut value) in [(0..5, first), (5..10, second), (10..13, third)] {
        for slot in slots.rev() {
            text[slot] = A42[(value % 42) as usize];
            value /= 42;
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}

fn standard(fields: &Fields) -> Option<String> {
    let grid = grid_field(fields.grid)?;
    let second = call(fields.second)?;
    if let Some(text) = with_affix(fields.first, &second, &grid) {
        return Some(text);
    }
    let first = match fields.first {
        n if n == NBASE + 1 => "CQ".to_owned(),
        n if n == NBASE + 2 => "QRZ".to_owned(),
        n if (NBASE + 3..NBASE + 1_003).contains(&n) => format!("CQ {:03}", n - NBASE - 3),
        n => call(n)?,
    };
    let (first, second, grid) = match prefix_slot(&grid) {
        slot @ 1..=450 => (prefixed(&first, slot), second, String::new()),
        slot @ 451..=900 => (first, prefixed(&second, slot - 450), String::new()),
        _ => (first, second, grid),
    };
    Some(format!("{first} {second} {grid}"))
}

fn with_affix(first: u32, second: &str, grid: &str) -> Option<String> {
    let (lead, affix, prefix) = match first {
        n if (CQ_PREFIX..QRZ_PREFIX).contains(&n) => ("CQ", n - CQ_PREFIX, true),
        n if (QRZ_PREFIX..DE_PREFIX).contains(&n) => ("QRZ", n - QRZ_PREFIX, true),
        n if (DE_PREFIX..CQ_SUFFIX).contains(&n) => ("DE", n - DE_PREFIX, true),
        n if (CQ_SUFFIX..QRZ_SUFFIX).contains(&n) => ("CQ", n - CQ_SUFFIX, false),
        n if (QRZ_SUFFIX..DE_SUFFIX).contains(&n) => ("QRZ", n - QRZ_SUFFIX, false),
        n if (DE_SUFFIX..DE_ALONE).contains(&n) => ("DE", n - DE_SUFFIX, false),
        DE_ALONE => return Some(de_alone(second, grid)),
        _ => return None,
    };
    let affix = base37(affix, if prefix { 4 } else { 3 });
    let affix = affix.trim();
    Some(if prefix {
        format!("{lead} {affix}/{second} {grid}")
    } else {
        format!("{lead} {second}/{affix} {grid}")
    })
}

fn de_alone(second: &str, grid: &str) -> String {
    match prefix_slot(grid) {
        slot @ 451..=900 => format!("DE {}", prefixed(second, slot - 450)),
        _ => format!("DE {second} {grid}"),
    }
}

fn base37(mut value: u32, width: usize) -> String {
    let mut out = vec![b' '; width];
    for slot in out.iter_mut().rev() {
        *slot = A37[(value % 37) as usize];
        value /= 37;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn call(value: u32) -> Option<String> {
    if value >= NBASE {
        return None;
    }
    let mut word = [b' '; 6];
    let mut n = value;
    for slot in (3..6).rev() {
        word[slot] = A37[(n % 27 + 10) as usize];
        n /= 27;
    }
    word[2] = A37[(n % 10) as usize];
    n /= 10;
    word[1] = A37[(n % 36) as usize];
    word[0] = *A37.get((n / 36) as usize)?;
    let word = std::str::from_utf8(&word).ok()?.trim().to_owned();
    if word.is_empty() || word.contains(' ') {
        return None;
    }
    Some(if let Some(rest) = word.strip_prefix("3D0") {
        format!("3DA0{rest}")
    } else if word.starts_with('Q') && word.as_bytes().get(1).is_some_and(u8::is_ascii_uppercase) {
        format!("3X{}", &word[1..])
    } else {
        word
    })
}

fn grid_field(value: u32) -> Option<String> {
    if value < NGBASE {
        return Some(locator(value));
    }
    Some(match value - NGBASE - 1 {
        0 => String::new(),
        n @ 1..=30 => format!("-{n:02}"),
        n @ 31..=60 => format!("R-{:02}", n - 30),
        61 => "RO".to_owned(),
        62 => "RRR".to_owned(),
        63 => "73".to_owned(),
        _ => return None,
    })
}

fn locator(value: u32) -> String {
    let latitude = value % 180;
    let west = 2 * (value / 180);
    let long_field = (358 - west) / 20;
    let long_square = (358 - west) % 20 / 2;
    let grid = [
        b'A' + long_field as u8,
        b'A' + (latitude / 10) as u8,
        b'0' + long_square as u8,
        b'0' + (latitude % 10) as u8,
    ];
    let grid = String::from_utf8_lossy(&grid).into_owned();
    let report = |base: &str| -> Option<i32> {
        let digits = grid.strip_prefix(base)?;
        digits.parse::<i32>().ok().map(|n| n - 50)
    };
    if let Some(n) = report("KA") {
        format!("{n:+03}")
    } else if let Some(n) = report("LA") {
        format!("R{n:+03}")
    } else {
        grid
    }
}

/// The add-on prefix or suffix a grid near the north pole stands for: 1 to 450 on the first
/// callsign, 451 to 900 on the second, 0 for an ordinary grid.
fn prefix_slot(grid: &str) -> i32 {
    let bytes = grid.as_bytes();
    if bytes.len() != 4
        || !bytes[..2].iter().all(|b| (b'A'..=b'R').contains(b))
        || !bytes[2..].iter().all(u8::is_ascii_digit)
    {
        return 0;
    }
    let field = |byte: u8| i32::from(byte - b'A');
    let digit = |byte: u8| i32::from(byte - b'0');
    let west = 180 - 20 * field(bytes[0]) - 2 * digit(bytes[2]) - 1;
    let latitude = -90 + 10 * field(bytes[1]) + digit(bytes[3]);
    if latitude < 85 {
        return 0;
    }
    5 * (west + 179) / 2 + latitude - 84
}

fn prefixed(call: &str, slot: i32) -> String {
    match slot {
        1..=339 => format!("{}/{call}", PREFIXES[(slot - 1) as usize]),
        401..=412 => format!("{call}/{}", SUFFIXES[(slot - 401) as usize]),
        _ => call.to_owned(),
    }
}

fn tidy(text: String) -> String {
    if let Some(rest) = text.strip_prefix("CQ9DX ") {
        return format!("CQ DX {rest}");
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 5
        && bytes.starts_with(b"E9")
        && bytes[2].is_ascii_uppercase()
        && bytes[3].is_ascii_uppercase()
        && bytes[4] == b' '
    {
        return format!("CQ {}", &text[2..]);
    }
    if let Some(rest) = text.strip_prefix("CQ 00")
        && rest.starts_with(|c: char| c.is_ascii_digit())
    {
        return format!("CQ {rest}");
    }
    text
}

#[cfg(any(test, feature = "synth"))]
pub(crate) use pack::pack;

#[cfg(any(test, feature = "synth"))]
mod pack {
    use super::{A37, A42, NBASE, NGBASE, TEXT_FLAG, WORDS, Words};

    /// Packs a standard message (`CALL CALL GRID|REPORT|RRR|73`, `CQ CALL GRID`) or up to 13
    /// characters of free text.
    pub(crate) fn pack(text: &str) -> Option<Words> {
        let parts: Vec<&str> = text.split_whitespace().collect();
        let standard = match parts.as_slice() {
            [first, second] => standard(first, second, ""),
            [first, second, third] => standard(first, second, third),
            _ => None,
        };
        let (first, second, grid) = standard.or_else(|| free_text(text))?;
        Some(words(first, second, grid))
    }

    fn standard(first: &str, second: &str, third: &str) -> Option<(u32, u32, u32)> {
        let first = match first {
            "CQ" => NBASE + 1,
            "QRZ" => NBASE + 2,
            call => pack_call(call)?,
        };
        Some((first, pack_call(second)?, pack_grid(third)?))
    }

    fn pack_call(call: &str) -> Option<u32> {
        let bytes = call.as_bytes();
        let aligned = if bytes.len() >= 3 && bytes[2].is_ascii_digit() && bytes.len() <= 6 {
            format!("{call:<6}")
        } else if bytes.len() >= 2 && bytes[1].is_ascii_digit() && bytes.len() <= 5 {
            format!(" {call:<5}")
        } else {
            return None;
        };
        let code = |byte: u8| A37.iter().position(|&c| c == byte).map(|i| i as u32);
        let c = aligned.as_bytes();
        let mut value = code(c[0])?;
        value = value * 36 + code(c[1]).filter(|&i| i < 36)?;
        value = value * 10 + code(c[2]).filter(|&i| i < 10)?;
        for &byte in &c[3..] {
            value = value * 27 + code(byte).filter(|&i| i >= 10)? - 10;
        }
        Some(value)
    }

    fn pack_grid(text: &str) -> Option<u32> {
        let report = |digits: &str| digits.parse::<u32>().ok().filter(|n| (1..=30).contains(n));
        Some(match text {
            "" => NGBASE + 1,
            "RO" => NGBASE + 62,
            "RRR" => NGBASE + 63,
            "73" => NGBASE + 64,
            _ if text.starts_with("R-") => NGBASE + 31 + report(&text[2..])?,
            _ if text.starts_with('-') => NGBASE + 1 + report(&text[1..])?,
            _ => locator(text)?,
        })
    }

    fn locator(grid: &str) -> Option<u32> {
        let b = grid.as_bytes();
        let field = |byte: u8| {
            (b'A'..=b'R')
                .contains(&byte)
                .then(|| u32::from(byte - b'A'))
        };
        let digit = |byte: u8| byte.is_ascii_digit().then(|| u32::from(byte - b'0'));
        if b.len() != 4 {
            return None;
        }
        let west_index = 179 - 10 * field(b[0])? - digit(b[2])?;
        let latitude = 10 * field(b[1])? + digit(b[3])?;
        Some(west_index * 180 + latitude)
    }

    fn free_text(text: &str) -> Option<(u32, u32, u32)> {
        let mut chars = [36u32; 13];
        for (slot, byte) in chars.iter_mut().zip(text.bytes()) {
            *slot = A42.iter().position(|&c| c == byte)? as u32;
        }
        let number =
            |range: std::ops::Range<usize>| chars[range].iter().fold(0, |n, &c| 42 * n + c);
        let (mut first, mut second, mut third) = (number(0..5), number(5..10), number(10..13));
        first = 2 * first + u32::from(third & 32_768 != 0);
        second = 2 * second + u32::from(third & 65_536 != 0);
        third &= 32_767;
        Some((first, second, third + TEXT_FLAG))
    }

    fn words(first: u32, second: u32, grid: u32) -> Words {
        let mut words = [0u8; WORDS];
        let bits = u128::from(first) << 44 | u128::from(second) << 16 | u128::from(grid);
        for (index, word) in words.iter_mut().enumerate() {
            *word = (bits >> (66 - 6 * index) & 63) as u8;
        }
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> Option<String> {
        unpack(&pack(text)?)
    }

    #[test]
    fn standard_messages_survive_a_round_trip() {
        for text in [
            "K1JT KF4RWA 73",
            "CQ GM7GAX IO75",
            "TF3G N7MQ CN84",
            "JA1KAU PD0JAC -23",
            "VE6WQ SQ2NIJ -14",
            "N2SU W0JMW R-14",
            "K1JT N5KDV EM41",
            "WB8QPG IZ0MIT RRR",
            "QRZ W1AW FN31",
            "CQ M0ABA JO01",
        ] {
            assert_eq!(round_trip(text).as_deref(), Some(text));
        }
    }

    #[test]
    fn free_text_keeps_thirteen_characters() {
        assert_eq!(
            round_trip("BG THX JOE 73").as_deref(),
            Some("BG THX JOE 73")
        );
        assert_eq!(round_trip("TNX 73 GL").as_deref(), Some("TNX 73 GL"));
    }

    #[test]
    fn the_bit_layout_matches_the_reference_packing() {
        let words = pack("CQ K1JT FN20").expect("packs");
        assert_eq!(&words[..4], &[62, 32, 32, 49]);
    }

    #[test]
    fn extended_reports_unpack_from_the_polar_grid() {
        let value = |text: &str| {
            let b = text.as_bytes();
            let west = 179 - 10 * u32::from(b[0] - b'A') - u32::from(b[2] - b'0');
            west * 180 + 10 * u32::from(b[1] - b'A') + u32::from(b[3] - b'0')
        };
        assert_eq!(grid_field(value("KA45")).as_deref(), Some("-05"));
        assert_eq!(grid_field(value("LA52")).as_deref(), Some("R+02"));
    }

    #[test]
    fn a_compound_callsign_reads_its_prefix() {
        let affix = (((u32::from(b'D' - b'A') + 10) * 37 + 21) * 37 + 36) * 37 + 36;
        let mut bits = u128::from(CQ_PREFIX + affix) << 44;
        bits |= u128::from(call_value("K1ABC")) << 16 | u128::from(NGBASE + 1);
        let words: Words = std::array::from_fn(|i| (bits >> (66 - 6 * i) & 63) as u8);
        assert_eq!(unpack(&words).as_deref(), Some("CQ DL/K1ABC"));
    }

    fn call_value(call: &str) -> u32 {
        let words = pack(&format!("{call} {call}")).expect("packs");
        fields(&words).first
    }

    #[test]
    fn words_out_of_range_are_refused() {
        let mut words = pack("CQ K1JT FN20").expect("packs");
        words[3] = 64;
        assert_eq!(unpack(&words), None);
    }
}
