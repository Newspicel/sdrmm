/// The four Eureka-147 transmission modes of ETSI EN 300 401.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TransmissionMode {
    #[default]
    I,
    Ii,
    Iii,
    Iv,
}

impl TransmissionMode {
    pub const ALL: [Self; 4] = [Self::I, Self::Ii, Self::Iii, Self::Iv];
}

#[derive(Clone, Copy, Debug)]
pub struct Mode {
    pub useful: usize,
    pub guard: usize,
    pub null: usize,
    pub symbols: usize,
    pub fic_symbols: usize,
    pub cifs: usize,
    pub fibs_per_block: usize,
}

impl Mode {
    #[must_use]
    pub const fn new(mode: TransmissionMode) -> Self {
        let (useful, guard, null, symbols, fic_symbols, cifs, fibs_per_block) = match mode {
            TransmissionMode::I => (2048, 504, 2656, 76, 3, 4, 3),
            TransmissionMode::Ii => (512, 126, 664, 76, 3, 1, 3),
            TransmissionMode::Iii => (256, 63, 345, 153, 8, 1, 4),
            TransmissionMode::Iv => (1024, 252, 1328, 76, 3, 2, 3),
        };
        Self {
            useful,
            guard,
            null,
            symbols,
            fic_symbols,
            cifs,
            fibs_per_block,
        }
    }

    #[must_use]
    pub const fn carriers(self) -> usize {
        self.useful * 3 / 4
    }

    #[must_use]
    pub const fn symbol(self) -> usize {
        self.useful + self.guard
    }

    #[must_use]
    pub const fn backoff(self) -> usize {
        self.guard / 8
    }

    #[must_use]
    pub const fn symbol_bits(self) -> usize {
        2 * self.carriers()
    }

    #[must_use]
    pub const fn frame_samples(self) -> usize {
        self.symbols * self.symbol()
    }

    #[must_use]
    pub const fn frame(self) -> usize {
        self.null + self.frame_samples()
    }

    #[must_use]
    pub const fn fic_block_bits(self) -> usize {
        self.fibs_per_block * 768
    }

    #[must_use]
    pub fn carrier_bin(self, carrier: i16) -> usize {
        i32::from(carrier).rem_euclid(self.useful as i32) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_partitions_match_en_300_401_tables_and_cif_duration() {
        for (mode, frame, carriers) in [
            (TransmissionMode::I, 196608, 1536),
            (TransmissionMode::Ii, 49152, 384),
            (TransmissionMode::Iii, 49152, 192),
            (TransmissionMode::Iv, 98304, 768),
        ] {
            let mode = Mode::new(mode);
            assert_eq!(mode.frame(), frame);
            assert_eq!(mode.carriers(), carriers);
            assert_eq!(mode.frame(), mode.cifs * 49152);
            assert_eq!(
                mode.fic_symbols * mode.symbol_bits(),
                mode.cifs * mode.fic_block_bits()
            );
            assert_eq!(
                (mode.symbols - mode.fic_symbols - 1) * mode.symbol_bits(),
                mode.cifs * 55296
            );
        }
    }
}
