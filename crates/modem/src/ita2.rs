pub const LTRS: u8 = 0x1F;
pub const FIGS: u8 = 0x1B;
pub const SPACE: u8 = 0x04;

pub const LETTERS: [char; 32] = [
    '\0', 'E', '\n', 'A', ' ', 'S', 'I', 'U', '\r', 'D', 'R', 'J', 'N', 'F', 'C', 'K', 'T', 'Z',
    'L', 'W', 'H', 'Y', 'P', 'Q', 'O', 'B', 'G', '\0', 'M', 'X', 'V', '\0',
];

/// The figures row of US teleprinters, which amateur RTTY and NAVTEX print. A code with no
/// printable figure reads as `'\0'`.
pub const FIGURES_US: [char; 32] = [
    '\0', '3', '\n', '-', ' ', '\0', '8', '7', '\r', '$', '4', '\'', ',', '!', ':', '(', '5', '"',
    ')', '2', '#', '6', '0', '1', '9', '?', '&', '\0', '.', '/', ';', '\0',
];

/// The international ITA2 figures row. WRU and bell come through as the control characters
/// ENQ and BEL.
pub const FIGURES_INTERNATIONAL: [char; 32] = [
    '\0', '3', '\n', '-', ' ', '\'', '8', '7', '\r', '\u{5}', '4', '\u{7}', ',', '!', ':', '(',
    '5', '+', ')', '2', '\u{a3}', '6', '0', '1', '9', '?', '&', '\0', '.', '/', ';', '\0',
];

#[derive(Clone, Copy, Debug)]
pub struct ShiftDecoder {
    figures_row: &'static [char; 32],
    figures: bool,
}

impl ShiftDecoder {
    #[must_use]
    pub const fn new(figures_row: &'static [char; 32]) -> Self {
        Self {
            figures_row,
            figures: false,
        }
    }

    /// The character a five-bit code prints, or `None` for a shift, which only changes rows.
    pub fn decode(&mut self, code: u8) -> Option<char> {
        let code = code & 0x1F;
        match code {
            LTRS => self.figures = false,
            FIGS => self.figures = true,
            _ => {
                let row = if self.figures {
                    self.figures_row
                } else {
                    &LETTERS
                };
                return Some(row[usize::from(code)]);
            }
        }
        None
    }

    pub fn unshift(&mut self) {
        self.figures = false;
    }

    #[must_use]
    pub const fn is_figures(&self) -> bool {
        self.figures
    }
}

/// Five-bit codes that print `text`, with a shift before each row change. Characters with no
/// code are skipped. With `unshift_on_space` a space drops the receiver back to letters, so the
/// next figure needs a fresh shift.
#[must_use]
pub fn encode(text: &str, figures_row: &[char; 32], unshift_on_space: bool) -> Vec<u8> {
    let mut codes = Vec::new();
    let mut figures = false;
    for ch in text.chars() {
        let Some((code, row)) = code_for(ch.to_ascii_uppercase(), figures_row) else {
            continue;
        };
        if let Some(row) = row
            && row != figures
        {
            codes.push(if row { FIGS } else { LTRS });
            figures = row;
        }
        codes.push(code);
        if code == SPACE && unshift_on_space {
            figures = false;
        }
    }
    codes
}

fn code_for(ch: char, figures_row: &[char; 32]) -> Option<(u8, Option<bool>)> {
    let find = |row: &[char; 32]| {
        (0u8..)
            .zip(row)
            .find_map(|(code, &c)| (c == ch && c != '\0').then_some(code))
    };
    match (find(&LETTERS), find(figures_row)) {
        (Some(code), Some(_)) => Some((code, None)),
        (Some(code), None) => Some((code, Some(false))),
        (None, Some(code)) => Some((code, Some(true))),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(row: &'static [char; 32], codes: &[u8]) -> String {
        let mut decoder = ShiftDecoder::new(row);
        codes
            .iter()
            .filter_map(|&code| decoder.decode(code))
            .collect()
    }

    #[test]
    fn the_rows_match_the_standard_alphabet() {
        #[rustfmt::skip]
        const STANDARD: [(u8, char, char, char); 32] = [
            (0x00, '\0', '\0', '\0'), (0x01, 'E', '3', '3'), (0x02, '\n', '\n', '\n'),
            (0x03, 'A', '-', '-'), (0x04, ' ', ' ', ' '), (0x05, 'S', '\0', '\''),
            (0x06, 'I', '8', '8'), (0x07, 'U', '7', '7'), (0x08, '\r', '\r', '\r'),
            (0x09, 'D', '$', '\u{5}'), (0x0A, 'R', '4', '4'), (0x0B, 'J', '\'', '\u{7}'),
            (0x0C, 'N', ',', ','), (0x0D, 'F', '!', '!'), (0x0E, 'C', ':', ':'),
            (0x0F, 'K', '(', '('), (0x10, 'T', '5', '5'), (0x11, 'Z', '"', '+'),
            (0x12, 'L', ')', ')'), (0x13, 'W', '2', '2'), (0x14, 'H', '#', '\u{a3}'),
            (0x15, 'Y', '6', '6'), (0x16, 'P', '0', '0'), (0x17, 'Q', '1', '1'),
            (0x18, 'O', '9', '9'), (0x19, 'B', '?', '?'), (0x1A, 'G', '&', '&'),
            (0x1B, '\0', '\0', '\0'), (0x1C, 'M', '.', '.'), (0x1D, 'X', '/', '/'),
            (0x1E, 'V', ';', ';'), (0x1F, '\0', '\0', '\0'),
        ];
        for (code, letter, us, international) in STANDARD {
            let index = usize::from(code);
            assert_eq!(LETTERS[index], letter, "letters row at {code:#04x}");
            assert_eq!(FIGURES_US[index], us, "US figures row at {code:#04x}");
            assert_eq!(
                FIGURES_INTERNATIONAL[index], international,
                "international figures row at {code:#04x}"
            );
        }
    }

    #[test]
    fn shifts_change_rows_and_print_nothing() {
        let row = &FIGURES_INTERNATIONAL;
        assert_eq!(decode_all(row, &[0x14, 0x01, 0x12, 0x12, 0x18]), "HELLO");
        assert_eq!(
            decode_all(row, &[FIGS, 0x17, 0x13, 0x01, 0x0A, 0x10]),
            "12345"
        );
        assert_eq!(decode_all(row, &[FIGS, 0x10, LTRS, 0x01]), "5E");
        assert_eq!(decode_all(row, &[0xE3]), "A");
    }

    #[test]
    fn unshift_returns_to_letters() {
        let mut decoder = ShiftDecoder::new(&FIGURES_US);
        assert_eq!(decoder.decode(FIGS), None);
        assert!(decoder.is_figures());
        decoder.unshift();
        assert_eq!(decoder.decode(0x03), Some('A'));
    }

    #[test]
    fn encoding_shifts_only_when_the_row_changes() {
        assert_eq!(
            encode("a1 2", &FIGURES_US, true),
            [0x03, FIGS, 0x17, SPACE, FIGS, 0x13]
        );
        assert_eq!(
            encode("a1 2", &FIGURES_US, false),
            [0x03, FIGS, 0x17, SPACE, 0x13]
        );
    }

    #[test]
    fn encoded_text_decodes_back() {
        let codes = encode("CQ DE DL1ABC 599", &FIGURES_US, false);
        assert_eq!(decode_all(&FIGURES_US, &codes), "CQ DE DL1ABC 599");
    }
}
