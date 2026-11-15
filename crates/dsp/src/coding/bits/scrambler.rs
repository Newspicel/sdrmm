const G3RUH_TAPS: [u8; 2] = [17, 12];

/// Multiplicative descrambler: shifts in the received bits.
#[derive(Clone, Debug)]
pub struct Descrambler {
    lfsr: Lfsr,
}

impl Descrambler {
    #[must_use]
    pub fn g3ruh() -> Self {
        Self::new(&G3RUH_TAPS)
    }

    #[must_use]
    pub fn new(taps: &[u8]) -> Self {
        Self {
            lfsr: Lfsr::new(taps),
        }
    }

    pub fn push(&mut self, bit: bool) -> bool {
        let out = bit ^ self.lfsr.feedback();
        self.lfsr.shift(bit);
        out
    }

    pub fn reset(&mut self) {
        self.lfsr.reg = 0;
    }
}

/// Multiplicative scrambler: shifts in the bits it sends.
#[derive(Clone, Debug)]
pub struct Scrambler {
    lfsr: Lfsr,
}

impl Scrambler {
    #[must_use]
    pub fn g3ruh() -> Self {
        Self::new(&G3RUH_TAPS)
    }

    #[must_use]
    pub fn new(taps: &[u8]) -> Self {
        Self {
            lfsr: Lfsr::new(taps),
        }
    }

    pub fn push(&mut self, bit: bool) -> bool {
        let out = bit ^ self.lfsr.feedback();
        self.lfsr.shift(out);
        out
    }

    pub fn reset(&mut self) {
        self.lfsr.reg = 0;
    }
}

#[derive(Clone, Debug)]
struct Lfsr {
    reg: u32,
    taps: u32,
}

impl Lfsr {
    fn new(taps: &[u8]) -> Self {
        let mut mask = 0u32;
        for &k in taps {
            assert!((1..=32).contains(&k), "tap exponent {k} outside 1..=32");
            mask |= 1 << (k - 1);
        }
        Self { reg: 0, taps: mask }
    }

    fn feedback(&self) -> bool {
        (self.reg & self.taps).count_ones() % 2 == 1
    }

    fn shift(&mut self, bit: bool) {
        self.reg = (self.reg << 1) | u32::from(bit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descrambler_inverts_scrambler() {
        let data: Vec<bool> = (0..500).map(|i: u32| i % 7 < 3).collect();
        let mut tx = Scrambler::g3ruh();
        let mut rx = Descrambler::g3ruh();
        let out: Vec<bool> = data.iter().map(|&b| rx.push(tx.push(b))).collect();
        assert_eq!(out, data);
    }

    #[test]
    fn descrambler_multiplies_a_single_error_by_the_tap_count() {
        let data: Vec<bool> = (0..200)
            .map(|i: u32| i.count_ones().is_multiple_of(2))
            .collect();
        let mut tx = Scrambler::g3ruh();
        let mut channel: Vec<bool> = data.iter().map(|&b| tx.push(b)).collect();
        let flip = 100;
        channel[flip] = !channel[flip];

        let mut rx = Descrambler::g3ruh();
        let out: Vec<bool> = channel.iter().map(|&b| rx.push(b)).collect();
        let errors: Vec<usize> = (0..data.len()).filter(|&i| out[i] != data[i]).collect();
        assert_eq!(errors, vec![flip, flip + 12, flip + 17]);
    }

    #[test]
    fn scrambler_whitens_a_constant_input() {
        let mut tx = Scrambler::new(&[17, 12]);
        let out: Vec<bool> = (0..1024).map(|_| tx.push(true)).collect();
        let ones = out.iter().filter(|&&b| b).count();
        assert!(
            (412..612).contains(&ones),
            "unbalanced: {ones} ones in 1024"
        );

        let longest = out
            .chunk_by(|a, b| a == b)
            .map(<[bool]>::len)
            .max()
            .unwrap_or(0);
        assert!(longest < 24, "run of {longest} defeats the clock recovery");
    }
}
