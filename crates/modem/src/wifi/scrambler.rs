/// Exponents of the 802.11 scrambler polynomial x^7 + x^4 + 1.
pub(crate) const SCRAMBLER_TAPS: [u8; 2] = [4, 7];
const REGISTER_BITS: usize = 7;
const REGISTER_MASK: u8 = (1 << REGISTER_BITS) - 1;

fn feedback(register: u8) -> u8 {
    let [low, high] = SCRAMBLER_TAPS;
    (register >> (high - 1) ^ register >> (low - 1)) & 1
}

fn shift(register: u8, bit: u8) -> u8 {
    (register << 1 | bit) & REGISTER_MASK
}

/// The next bit of the free-running OFDM keystream.
pub(crate) fn keystream(register: &mut u8) -> bool {
    let next = feedback(*register);
    *register = shift(*register, next);
    next == 1
}

/// The DSSS scrambler, which shifts in the bits it sends.
pub(crate) struct Scrambler(u8);

impl Scrambler {
    pub(crate) fn new(seed: u8) -> Self {
        Self(seed)
    }

    pub(crate) fn push(&mut self, bit: bool) -> bool {
        let out = u8::from(bit) ^ feedback(self.0);
        self.0 = shift(self.0, out);
        out == 1
    }
}

const PERIOD: usize = 127;

const fn build_tables() -> ([bool; 2 * PERIOD], [u8; 128]) {
    let mut stream = [false; 2 * PERIOD];
    let mut start = [0u8; 128];
    let mut reg = 1u8;
    let mut i = 0;
    while i < PERIOD {
        start[reg as usize] = i as u8;
        let next = ((reg >> (SCRAMBLER_TAPS[1] - 1)) ^ (reg >> (SCRAMBLER_TAPS[0] - 1))) & 1;
        stream[i] = next == 1;
        stream[i + PERIOD] = next == 1;
        reg = (reg << 1 | next) & REGISTER_MASK;
        i += 1;
    }
    (stream, start)
}

const TABLES: ([bool; 2 * PERIOD], [u8; 128]) = build_tables();

/// Descrambles an OFDM data field in place. Its first seven bits are zeros on air, so they hand
/// over the transmitter's register; they read back as zeros.
pub(crate) fn descramble(bits: &mut [bool]) {
    let seed = bits.len().min(REGISTER_BITS);
    let register = bits[..seed]
        .iter()
        .fold(0u8, |acc, &bit| acc << 1 | u8::from(bit));
    bits[..seed].fill(false);
    if register == 0 {
        return;
    }
    let pos = usize::from(TABLES.1[usize::from(register)]);
    let pattern = &TABLES.0[pos..pos + PERIOD];
    for chunk in bits[seed..].chunks_mut(PERIOD) {
        for (bit, &mask) in chunk.iter_mut().zip(pattern) {
            *bit ^= mask;
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::Descrambler;

    use super::*;

    #[test]
    fn the_service_field_seeds_the_descrambler() {
        let mut register = 0b101_1101u8;
        let data: Vec<bool> = (0..64).map(|k| k % 3 == 0 && k >= 16).collect();
        let mut scrambled: Vec<bool> = data
            .iter()
            .map(|&bit| bit ^ keystream(&mut register))
            .collect();
        descramble(&mut scrambled);
        assert_eq!(scrambled, data);
    }

    #[test]
    fn the_shared_descrambler_inverts_the_dsss_scrambler() {
        let data: Vec<bool> = (0..200)
            .map(|k: u32| k.count_ones().is_multiple_of(3))
            .collect();
        let mut scrambler = Scrambler::new(0b110_1100);
        let mut descrambler = Descrambler::new(&SCRAMBLER_TAPS);
        let back: Vec<bool> = data
            .iter()
            .map(|&bit| descrambler.push(scrambler.push(bit)))
            .collect();
        assert_eq!(back[REGISTER_BITS..], data[REGISTER_BITS..]);
    }
}
