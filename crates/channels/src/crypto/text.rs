pub(crate) fn base64(text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim();
    let unpadded = trimmed.trim_end_matches('=');
    let padding = trimmed.len() - unpadded.len();
    if padding > 2 || (padding > 0 && !trimmed.len().is_multiple_of(4)) || unpadded.len() % 4 == 1 {
        return None;
    }
    let mut bytes = Vec::with_capacity(unpadded.len() * 3 / 4);
    let mut accumulator = 0u32;
    let mut bits = 0u32;
    for character in unpadded.bytes() {
        accumulator = (accumulator << 6) | u32::from(base64_value(character)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((accumulator >> bits) as u8);
        }
    }
    Some(bytes)
}

fn base64_value(character: u8) -> Option<u8> {
    match character {
        b'A'..=b'Z' => Some(character - b'A'),
        b'a'..=b'z' => Some(character - b'a' + 26),
        b'0'..=b'9' => Some(character - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

pub(crate) fn hex(text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim();
    let digits_text = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    let digits = digits_text
        .chars()
        .filter(|character| !matches!(character, ' ' | ':' | '-'))
        .map(|character| character.to_digit(16).map(|digit| digit as u8))
        .collect::<Option<Vec<u8>>>()?;
    if digits.len() % 2 != 0 {
        return None;
    }
    Some(
        digits
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[high, low]| (high << 4) | low)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::{base64, hex};

    #[test]
    fn base64_decodes_meshtastic_default_key() {
        assert_eq!(
            base64("1PG7OiApB1nwvP+rz05pAQ=="),
            hex("d4f1bb3a20290759f0bcffabcf4e6901")
        );
    }

    #[test]
    fn base64_accepts_url_safe_unpadded_and_whitespace() {
        assert_eq!(
            base64("  1PG7OiApB1nwvP-rz05pAQ \n"),
            base64("1PG7OiApB1nwvP+rz05pAQ==")
        );
        assert_eq!(base64("_w"), Some(vec![0xff]));
        assert_eq!(base64("TWFu"), Some(b"Man".to_vec()));
        assert_eq!(base64("TWE="), Some(b"Ma".to_vec()));
        assert_eq!(base64(""), Some(Vec::new()));
    }

    #[test]
    fn base64_rejects_bad_input() {
        assert_eq!(base64("TW*u"), None);
        assert_eq!(base64("TWFuT"), None);
        assert_eq!(base64("TQ==="), None);
        assert_eq!(base64("TWE=="), None);
        assert_eq!(base64("T=Fu"), None);
    }

    #[test]
    fn hex_accepts_separators_prefix_and_case() {
        assert_eq!(hex("0xDEadBEef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(hex("de:ad-be ef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(hex(" 01 "), Some(vec![0x01]));
    }

    #[test]
    fn hex_rejects_bad_input() {
        assert_eq!(hex("abc"), None);
        assert_eq!(hex("zz"), None);
        assert_eq!(hex("0x0g"), None);
    }
}
