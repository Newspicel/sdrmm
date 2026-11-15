/// Initial register bit the spec sets above the six channel bits.
const SEED_BIT: u8 = 0x02;
/// Feedback taps of the x^7 + x^4 + 1 whitening polynomial.
const TAPS: u8 = 0x11;

pub struct Whitener(u8);

impl Whitener {
    #[must_use]
    pub fn new(channel_index: u8) -> Self {
        Self(channel_index.reverse_bits() | SEED_BIT)
    }

    pub fn bit(&mut self) -> bool {
        let out = self.0 & 0x80 != 0;
        if out {
            self.0 ^= TAPS;
        }
        self.0 <<= 1;
        out
    }

    pub fn byte(&mut self, byte: u8) -> u8 {
        (0..8).fold(byte, |acc, bit| acc ^ (u8::from(self.bit()) << bit))
    }
}
