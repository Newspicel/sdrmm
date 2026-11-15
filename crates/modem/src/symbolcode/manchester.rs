use super::pair::PairDecode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManchesterConvention {
    Ieee8023,
    GeThomas,
}

impl ManchesterConvention {
    #[must_use]
    pub fn halves(self, bit: bool) -> (bool, bool) {
        match self {
            Self::Ieee8023 => (!bit, bit),
            Self::GeThomas => (bit, !bit),
        }
    }
}

#[must_use]
pub fn manchester_encode(convention: ManchesterConvention, bits: &[bool]) -> Vec<bool> {
    let mut out = Vec::with_capacity(bits.len() * 2);
    for &bit in bits {
        let (first, second) = convention.halves(bit);
        out.push(first);
        out.push(second);
    }
    out
}

#[must_use]
pub fn manchester_decode_pair(
    convention: ManchesterConvention,
    first: bool,
    second: bool,
) -> Option<bool> {
    if first == second {
        return None;
    }
    Some(match convention {
        ManchesterConvention::Ieee8023 => second,
        ManchesterConvention::GeThomas => first,
    })
}

#[must_use]
pub fn manchester_decode(convention: ManchesterConvention, levels: &[bool]) -> PairDecode {
    let mut bits = Vec::with_capacity(levels.len() / 2);
    let mut violations = 0;
    for pair in levels.as_chunks::<2>().0 {
        match manchester_decode_pair(convention, pair[0], pair[1]) {
            Some(bit) => bits.push(bit),
            None => {
                violations += 1;
                bits.push(match convention {
                    ManchesterConvention::Ieee8023 => !pair[0],
                    ManchesterConvention::GeThomas => pair[0],
                });
            }
        }
    }
    let slipped = manchester_violations(levels.get(1..).unwrap_or_default());
    PairDecode::judged(bits, violations, slipped)
}

fn manchester_violations(levels: &[bool]) -> usize {
    levels
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|p| p[0] == p[1])
        .count()
}
