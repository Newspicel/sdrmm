//! PSK31 Varicode: shorter codes for commoner characters, no code holding two zeros in a row,
//! so "00" marks the gap between characters.

/// The code of every ASCII character, indexed by its byte.
pub const VARICODE: [&str; 128] = [
    "1010101011",
    "1011011011",
    "1011101101",
    "1101110111",
    "1011101011",
    "1101011111",
    "1011101111",
    "1011111101",
    "1011111111",
    "11101111",
    "11101",
    "1101101111",
    "1011011101",
    "11111",
    "1101110101",
    "1110101011",
    "1011110111",
    "1011110101",
    "1110101101",
    "1110101111",
    "1101011011",
    "1101101011",
    "1101101101",
    "1101010111",
    "1101111011",
    "1101111101",
    "1110110111",
    "1101010101",
    "1101011101",
    "1110111011",
    "1011111011",
    "1101111111",
    "1",
    "111111111",
    "101011111",
    "111110101",
    "111011011",
    "1011010101",
    "1010111011",
    "101111111",
    "11111011",
    "11110111",
    "101101111",
    "111011111",
    "1110101",
    "110101",
    "1010111",
    "110101111",
    "10110111",
    "10111101",
    "11101101",
    "11111111",
    "101110111",
    "101011011",
    "101101011",
    "110101101",
    "110101011",
    "110110111",
    "11110101",
    "110111101",
    "111101101",
    "1010101",
    "111010111",
    "1010101111",
    "1010111101",
    "1111101",
    "11101011",
    "10101101",
    "10110101",
    "1110111",
    "11011011",
    "11111101",
    "101010101",
    "1111111",
    "111111101",
    "101111101",
    "11010111",
    "10111011",
    "11011101",
    "10101011",
    "11010101",
    "111011101",
    "10101111",
    "1101111",
    "1101101",
    "101010111",
    "110110101",
    "101011101",
    "101110101",
    "101111011",
    "1010101101",
    "111110111",
    "111101111",
    "111111011",
    "1010111111",
    "101101101",
    "1011011111",
    "1011",
    "1011111",
    "101111",
    "101101",
    "11",
    "111101",
    "1011011",
    "101011",
    "1101",
    "111101011",
    "10111111",
    "11011",
    "111011",
    "1111",
    "111",
    "111111",
    "110111111",
    "10101",
    "10111",
    "101",
    "110111",
    "1111011",
    "1101011",
    "11011111",
    "1011101",
    "111010101",
    "1010110111",
    "110111011",
    "1010110101",
    "1011010111",
    "1110110101",
];

/// Turns Varicode bits back into characters.
#[derive(Clone, Copy, Debug, Default)]
pub struct VaricodeDecoder {
    code: u16,
    bits: u8,
    zero_run: u8,
}

impl VaricodeDecoder {
    /// Takes one bit; returns the character a completed gap closes, if the code is known and
    /// printable or a line break.
    pub fn push(&mut self, bit: bool) -> Option<char> {
        if bit {
            if self.zero_run == 1 {
                self.push_bit(false);
            }
            self.zero_run = 0;
            self.push_bit(true);
            None
        } else {
            self.zero_run = self.zero_run.saturating_add(1);
            if self.zero_run == 2 {
                self.finish_character()
            } else {
                None
            }
        }
    }

    fn push_bit(&mut self, bit: bool) {
        if self.bits >= 10 {
            self.bits = 11;
            return;
        }
        self.code = (self.code << 1) | u16::from(bit);
        self.bits += 1;
    }

    fn finish_character(&mut self) -> Option<char> {
        let decoded = if self.bits <= 10
            && let Some(index) = VARICODE
                .iter()
                .position(|candidate| code_matches(candidate, self.code, self.bits))
            && let Some(ch) = char::from_u32(index as u32)
            && (ch == '\n' || ch == '\r' || !ch.is_control())
        {
            Some(ch)
        } else {
            None
        };
        self.code = 0;
        self.bits = 0;
        decoded
    }
}

fn code_matches(candidate: &str, code: u16, bits: u8) -> bool {
    candidate.len() == usize::from(bits)
        && candidate
            .bytes()
            .fold(0u16, |value, bit| (value << 1) | u16::from(bit == b'1'))
            == code
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn the_table_is_complete_prefix_safe_and_unique() {
        assert_eq!(VARICODE[usize::from(b' ')], "1");
        assert_eq!(VARICODE[usize::from(b'e')], "11");
        assert_eq!(VARICODE[usize::from(b'Z')], "1010101101");
        let unique: HashSet<&str> = VARICODE.into_iter().collect();
        assert_eq!(unique.len(), VARICODE.len());
        for code in VARICODE {
            assert!(code.starts_with('1') && code.ends_with('1'), "{code}");
            assert!(!code.contains("00"), "{code}");
            assert!(code.len() <= 10, "{code}");
        }
    }

    #[test]
    fn text_round_trips_through_the_bits() {
        let mut decoder = VaricodeDecoder::default();
        let mut text = String::new();
        for byte in "Hi 73\n".bytes() {
            let bits = VARICODE[usize::from(byte)].bytes().map(|bit| bit == b'1');
            for bit in bits.chain([false, false]) {
                text.extend(decoder.push(bit));
            }
        }
        assert_eq!(text, "Hi 73\n");
    }

    #[test]
    fn control_codes_and_overlong_runs_print_nothing() {
        let mut decoder = VaricodeDecoder::default();
        let bell = VARICODE[7].bytes().map(|bit| bit == b'1');
        assert!(
            bell.chain([false, false])
                .all(|bit| decoder.push(bit).is_none())
        );
        let overlong = std::iter::repeat_n(true, 12).chain([false, false]);
        assert!(overlong.map(|bit| decoder.push(bit)).all(|ch| ch.is_none()));
    }
}
