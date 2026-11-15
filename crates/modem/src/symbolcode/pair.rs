use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    BitEdge,
    MidBit,
    Ambiguous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairDecode {
    pub bits: Vec<bool>,
    pub violations: usize,
    pub alignment: Alignment,
}

impl PairDecode {
    /// Weighs the violations read as given against those read half a level later.
    pub(super) fn judged(bits: Vec<bool>, violations: usize, slipped: usize) -> Self {
        let alignment = match violations.cmp(&slipped) {
            Ordering::Less => Alignment::BitEdge,
            Ordering::Greater => Alignment::MidBit,
            Ordering::Equal => Alignment::Ambiguous,
        };
        Self {
            bits,
            violations,
            alignment,
        }
    }
}
