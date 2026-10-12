use super::bits::Reader;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Payload {
    Text(String),
    Position {
        lat: f64,
        lon: f64,
        error_m: Option<u32>,
    },
    Status(u16),
    Data {
        protocol: Option<u8>,
        hex: String,
    },
}

const SIMPLE_TEXT: u8 = 0x02;
const SIMPLE_IMMEDIATE_TEXT: u8 = 0x09;
const LIP: u8 = 0x0A;
const TEXT: u8 = 0x82;
const IMMEDIATE_TEXT: u8 = 0x89;
const SDS_TL_FROM: u8 = 0x80;
const SDS_TRANSFER: u32 = 0;

const MAX_HEX_BYTES: usize = 64;

pub(crate) fn parse(r: &mut Reader<'_>) -> Option<Payload> {
    match r.take(2)? {
        0 => Some(Payload::Status(r.take_u16(16)?)),
        1 => user_data(r, 32, None),
        2 => user_data(r, 64, None),
        _ => {
            let length = usize::try_from(r.take(11)?).ok()?;
            if r.remaining() < length || length < 8 {
                return None;
            }
            let mut body = Reader::new(&r.rest()[..length]);
            let protocol = body.take_u8(8)?;
            protocol_payload(protocol, &mut body)
        }
    }
}

fn user_data(r: &mut Reader<'_>, bits: usize, protocol: Option<u8>) -> Option<Payload> {
    let mut hex = String::new();
    for _ in 0..bits / 8 {
        hex.push_str(&format!("{:02X}", r.take(8)?));
    }
    Some(Payload::Data { protocol, hex })
}

fn protocol_payload(protocol: u8, r: &mut Reader<'_>) -> Option<Payload> {
    match protocol {
        SIMPLE_TEXT | SIMPLE_IMMEDIATE_TEXT => text(r, false),
        TEXT | IMMEDIATE_TEXT => {
            if !transfer_header(r)? {
                return data(protocol, r);
            }
            text(r, true)
        }
        LIP => location(r).or_else(|| data(protocol, r)),
        _ if protocol >= SDS_TL_FROM => {
            transfer_header(r)?;
            data(protocol, r)
        }
        _ => data(protocol, r),
    }
}

fn data(protocol: u8, r: &mut Reader<'_>) -> Option<Payload> {
    let bytes = (r.remaining() / 8).min(MAX_HEX_BYTES);
    user_data(r, bytes * 8, Some(protocol))
}

fn transfer_header(r: &mut Reader<'_>) -> Option<bool> {
    if r.take(4)? != SDS_TRANSFER {
        return Some(false);
    }
    r.skip(2 + 1)?;
    let stored = r.flag()?;
    r.skip(8)?;
    if stored {
        r.skip(5)?;
        match r.take(3)? {
            0 => r.skip(8)?,
            1 => r.skip(24)?,
            2 => r.skip(48)?,
            3 => {
                let digits = usize::try_from(r.take(8)?).ok()?;
                r.skip(4 * (digits + digits % 2))?;
            }
            _ => {}
        }
    }
    Some(true)
}

fn text(r: &mut Reader<'_>, timestamped: bool) -> Option<Payload> {
    let stamp = r.flag()?;
    let scheme = r.take_u8(7)?;
    if timestamped && stamp {
        r.skip(24)?;
    }
    let mut bytes = [0u8; 256];
    let mut len = 0;
    while r.remaining() >= 8 && len < bytes.len() {
        bytes[len] = r.take_u8(8)?;
        len += 1;
    }
    Some(Payload::Text(decode_text(scheme, &bytes[..len])))
}

pub(crate) fn decode_text(scheme: u8, bytes: &[u8]) -> String {
    match scheme {
        0 => unpack_gsm(bytes),
        26 => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .filter_map(|pair| char::from_u32(u32::from(pair[0]) << 8 | u32::from(pair[1])))
            .collect(),
        1 | 13 => bytes.iter().map(|&byte| char::from(byte)).collect(),
        _ => bytes
            .iter()
            .map(|&byte| {
                if byte.is_ascii() {
                    char::from(byte)
                } else {
                    '?'
                }
            })
            .collect(),
    }
    .trim_end_matches('\0')
    .to_owned()
}

const GSM_BASIC: [char; 128] = [
    '@', '£', '$', '¥', 'è', 'é', 'ù', 'ì', 'ò', 'Ç', '\n', 'Ø', 'ø', '\r', 'Å', 'å', 'Δ', '_',
    'Φ', 'Γ', 'Λ', 'Ω', 'Π', 'Ψ', 'Σ', 'Θ', 'Ξ', ' ', 'Æ', 'æ', 'ß', 'É', ' ', '!', '"', '#', '¤',
    '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', '0', '1', '2', '3', '4', '5', '6', '7',
    '8', '9', ':', ';', '<', '=', '>', '?', '¡', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J',
    'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'Ä', 'Ö', 'Ñ',
    'Ü', '§', '¿', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'ä', 'ö', 'ñ', 'ü', 'à',
];

fn unpack_gsm(bytes: &[u8]) -> String {
    let septets = bytes.len() * 8 / 7;
    (0..septets)
        .map(|i| {
            let bit = i * 7;
            let (byte, shift) = (bit / 8, bit % 8);
            let low = u16::from(bytes[byte]) >> shift;
            let high = bytes
                .get(byte + 1)
                .map_or(0, |&b| u16::from(b) << (8 - shift));
            GSM_BASIC[usize::from((low | high) & 0x7F)]
        })
        .collect()
}

const POSITION_ERROR_M: [Option<u32>; 8] = [
    Some(2),
    Some(20),
    Some(200),
    Some(2_000),
    Some(20_000),
    Some(200_000),
    None,
    None,
];

const LONG_LOCATION_REPORT: u32 = 3;
const POINT_WITH_ERROR: u32 = 10;

fn location(r: &mut Reader<'_>) -> Option<Payload> {
    match r.take(2)? {
        0 => {
            r.skip(2)?;
            let (lat, lon) = coordinates(r)?;
            let error_m = POSITION_ERROR_M[usize::try_from(r.take(3)?).ok()?];
            Some(Payload::Position { lat, lon, error_m })
        }
        1 => {
            if r.take(4)? != LONG_LOCATION_REPORT {
                return None;
            }
            match r.take(2)? {
                1 => r.skip(2)?,
                2 => r.skip(22)?,
                _ => {}
            }
            let shape = r.take(4)?;
            if !(1..=POINT_WITH_ERROR).contains(&shape) {
                return None;
            }
            let (lat, lon) = coordinates(r)?;
            let error_m = if shape == POINT_WITH_ERROR {
                POSITION_ERROR_M[usize::try_from(r.take(3)?).ok()?]
            } else {
                None
            };
            Some(Payload::Position { lat, lon, error_m })
        }
        _ => None,
    }
}

fn coordinates(r: &mut Reader<'_>) -> Option<(f64, f64)> {
    let lon = signed(r.take(25)?, 25) * 180.0 / f64::from(1u32 << 24);
    let lat = signed(r.take(24)?, 24) * 90.0 / f64::from(1u32 << 23);
    Some((lat, lon))
}

fn signed(value: u32, bits: u32) -> f64 {
    let sign = 1u32 << (bits - 1);
    if value & sign == 0 {
        f64::from(value)
    } else {
        f64::from(value) - f64::from(sign) * 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(out: &mut Vec<bool>, value: u64, len: usize) {
        out.extend((0..len).rev().map(|i| value >> i & 1 == 1));
    }

    fn type4(protocol: u8, body: &[bool]) -> Vec<bool> {
        let mut out = Vec::new();
        push(&mut out, 3, 2);
        push(&mut out, (body.len() + 8) as u64, 11);
        push(&mut out, u64::from(protocol), 8);
        out.extend(body);
        out
    }

    #[test]
    fn sds_tl_text_reads_latin_1() {
        let mut body = Vec::new();
        push(&mut body, 0, 4);
        push(&mut body, 0, 4);
        push(&mut body, 7, 8);
        push(&mut body, 0, 1);
        push(&mut body, 1, 7);
        for byte in b"Hello ".iter().chain(&[0xDC]) {
            push(&mut body, u64::from(*byte), 8);
        }
        let bits = type4(TEXT, &body);
        assert_eq!(
            parse(&mut Reader::new(&bits)),
            Some(Payload::Text("Hello \u{DC}".to_owned()))
        );
    }

    #[test]
    fn gsm_septets_unpack_least_significant_first() {
        assert_eq!(unpack_gsm(&[0xE8, 0x32, 0x9B, 0xFD, 0x06]), "hello");
    }

    #[test]
    fn a_short_location_report_lands_on_the_map() {
        let mut body = Vec::new();
        push(&mut body, 0, 2);
        push(&mut body, 0, 2);
        let lon = (13.4 / 180.0 * f64::from(1u32 << 24)).round() as u64;
        let lat = ((-52.5 / 90.0 * f64::from(1u32 << 23)).round() as i64 & 0xFF_FFFF) as u64;
        push(&mut body, lon, 25);
        push(&mut body, lat, 24);
        push(&mut body, 1, 3);
        push(&mut body, 0, 12);
        let bits = type4(LIP, &body);
        let Some(Payload::Position { lat, lon, error_m }) = parse(&mut Reader::new(&bits)) else {
            panic!("no position");
        };
        assert!(
            (lat + 52.5).abs() < 1e-5 && (lon - 13.4).abs() < 1e-5,
            "{lat} {lon}"
        );
        assert_eq!(error_m, Some(20));
    }

    #[test]
    fn short_user_data_is_shown_as_hex() {
        let mut bits = Vec::new();
        push(&mut bits, 1, 2);
        push(&mut bits, 0xDEAD_BEEF, 32);
        assert_eq!(
            parse(&mut Reader::new(&bits)),
            Some(Payload::Data {
                protocol: None,
                hex: "DEADBEEF".to_owned()
            })
        );
    }
}
