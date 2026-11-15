#[must_use]
pub fn nrz_encode(bits: &[bool]) -> Vec<bool> {
    bits.to_vec()
}

#[must_use]
pub fn nrz_decode(levels: &[bool]) -> Vec<bool> {
    levels.to_vec()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NrziConvention {
    TransitionOnZero,
    TransitionOnOne,
}

impl NrziConvention {
    fn toggles(self, bit: bool) -> bool {
        match self {
            Self::TransitionOnZero => !bit,
            Self::TransitionOnOne => bit,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NrziEncoder {
    convention: NrziConvention,
    level: bool,
}

impl NrziEncoder {
    #[must_use]
    pub fn new(convention: NrziConvention) -> Self {
        Self {
            convention,
            level: false,
        }
    }

    pub fn encode(&mut self, bit: bool) -> bool {
        if self.convention.toggles(bit) {
            self.level = !self.level;
        }
        self.level
    }

    pub fn encode_all(&mut self, bits: &[bool]) -> Vec<bool> {
        bits.iter().map(|&b| self.encode(b)).collect()
    }

    pub fn reset(&mut self) {
        self.level = false;
    }
}

#[derive(Clone, Debug)]
pub struct NrziDecoder {
    convention: NrziConvention,
    last: bool,
}

impl NrziDecoder {
    #[must_use]
    pub fn new(convention: NrziConvention) -> Self {
        Self {
            convention,
            last: false,
        }
    }

    pub fn decode(&mut self, level: bool) -> bool {
        let held = level == self.last;
        self.last = level;
        match self.convention {
            NrziConvention::TransitionOnZero => held,
            NrziConvention::TransitionOnOne => !held,
        }
    }

    pub fn decode_all(&mut self, levels: &[bool]) -> Vec<bool> {
        levels.iter().map(|&l| self.decode(l)).collect()
    }

    pub fn reset(&mut self) {
        self.last = false;
    }
}
