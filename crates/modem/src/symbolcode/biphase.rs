use super::pair::PairDecode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BiphaseVariant {
    Mark,
    Space,
}

impl BiphaseVariant {
    fn mid_transition(self, bit: bool) -> bool {
        match self {
            Self::Mark => bit,
            Self::Space => !bit,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BiphaseEncoder {
    variant: BiphaseVariant,
    level: bool,
}

impl BiphaseEncoder {
    #[must_use]
    pub fn new(variant: BiphaseVariant) -> Self {
        Self {
            variant,
            level: false,
        }
    }

    pub fn encode(&mut self, bit: bool) -> (bool, bool) {
        let first = !self.level;
        let second = if self.variant.mid_transition(bit) {
            !first
        } else {
            first
        };
        self.level = second;
        (first, second)
    }

    pub fn encode_all(&mut self, bits: &[bool]) -> Vec<bool> {
        let mut out = Vec::with_capacity(bits.len() * 2);
        for &bit in bits {
            let (first, second) = self.encode(bit);
            out.push(first);
            out.push(second);
        }
        out
    }

    pub fn reset(&mut self) {
        self.level = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BiphasePair {
    pub bit: bool,
    pub boundary_transition: bool,
}

#[derive(Clone, Debug)]
pub struct BiphaseDecoder {
    variant: BiphaseVariant,
    last: Option<bool>,
}

impl BiphaseDecoder {
    #[must_use]
    pub fn new(variant: BiphaseVariant) -> Self {
        Self {
            variant,
            last: None,
        }
    }

    pub fn decode(&mut self, first: bool, second: bool) -> BiphasePair {
        let boundary_transition = self.last.is_none_or(|level| level != first);
        self.last = Some(second);
        BiphasePair {
            bit: self.variant.mid_transition(true) == (first != second),
            boundary_transition,
        }
    }

    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[must_use]
pub fn biphase_decode(variant: BiphaseVariant, levels: &[bool]) -> PairDecode {
    let mut decoder = BiphaseDecoder::new(variant);
    let mut bits = Vec::with_capacity(levels.len() / 2);
    let mut violations = 0;
    for pair in levels.as_chunks::<2>().0 {
        let out = decoder.decode(pair[0], pair[1]);
        bits.push(out.bit);
        violations += usize::from(!out.boundary_transition);
    }
    let slipped = boundary_violations(levels.get(1..).unwrap_or_default());
    PairDecode::judged(bits, violations, slipped)
}

fn boundary_violations(levels: &[bool]) -> usize {
    let mut prev = None;
    let mut violations = 0;
    for pair in levels.as_chunks::<2>().0 {
        violations += usize::from(prev == Some(pair[0]));
        prev = Some(pair[1]);
    }
    violations
}
