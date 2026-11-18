//! International Morse code: the table both ways, and a reader that turns a keyed carrier's
//! envelope into text while it learns the sender's speed.

mod reader;

pub use reader::{FIT_MAX, MorseChunk, MorseReader};

/// Every character as its dots and dashes. `SK` is the end-of-work prosign.
pub const TABLE: &[(&str, &str)] = &[
    (".-", "A"),
    ("-...", "B"),
    ("-.-.", "C"),
    ("-..", "D"),
    (".", "E"),
    ("..-.", "F"),
    ("--.", "G"),
    ("....", "H"),
    ("..", "I"),
    (".---", "J"),
    ("-.-", "K"),
    (".-..", "L"),
    ("--", "M"),
    ("-.", "N"),
    ("---", "O"),
    (".--.", "P"),
    ("--.-", "Q"),
    (".-.", "R"),
    ("...", "S"),
    ("-", "T"),
    ("..-", "U"),
    ("...-", "V"),
    (".--", "W"),
    ("-..-", "X"),
    ("-.--", "Y"),
    ("--..", "Z"),
    ("-----", "0"),
    (".----", "1"),
    ("..---", "2"),
    ("...--", "3"),
    ("....-", "4"),
    (".....", "5"),
    ("-....", "6"),
    ("--...", "7"),
    ("---..", "8"),
    ("----.", "9"),
    (".-.-.-", "."),
    ("--..--", ","),
    ("..--..", "?"),
    (".----.", "'"),
    ("-.-.--", "!"),
    ("-..-.", "/"),
    ("-.--.", "("),
    ("-.--.-", ")"),
    (".-...", "&"),
    ("---...", ":"),
    ("-.-.-.", ";"),
    ("-...-", "="),
    (".-.-.", "+"),
    ("-....-", "-"),
    ("..--.-", "_"),
    (".-..-.", "\""),
    ("...-..-", "$"),
    (".--.-.", "@"),
    ("...-.-", "SK"),
];

/// A pattern as one number: a leading 1, then a bit per element, 1 for a dash.
#[must_use]
pub const fn key(pattern: &str) -> u16 {
    let bytes = pattern.as_bytes();
    let mut key = 1;
    let mut i = 0;
    while i < bytes.len() {
        key = key * 2 + (bytes[i] == b'-') as u16;
        i += 1;
    }
    key
}

/// The text a [`key`] stands for.
#[must_use]
pub fn decode(key_value: u16) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(pattern, _)| key(pattern) == key_value)
        .map(|(_, text)| *text)
}

/// The dots and dashes of one character, either case.
#[must_use]
pub fn pattern(ch: char) -> Option<&'static str> {
    let upper = ch.to_ascii_uppercase();
    TABLE
        .iter()
        .find(|(_, text)| text.len() == 1 && text.starts_with(upper))
        .map(|(pattern, _)| *pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_round_trip() {
        for (index, (pattern, text)) in TABLE.iter().enumerate() {
            assert_eq!(decode(key(pattern)), Some(*text));
            assert!(
                TABLE[..index]
                    .iter()
                    .all(|(other, _)| key(other) != key(pattern))
            );
        }
        assert_eq!(key(".-"), 0b101);
        assert_eq!(decode(key("........")), None);
    }

    #[test]
    fn characters_find_their_pattern_in_either_case() {
        assert_eq!(pattern('q'), Some("--.-"));
        assert_eq!(pattern('@'), Some(".--.-."));
        assert_eq!(pattern('S'), Some("..."));
        assert_eq!(pattern('#'), None);
    }
}
