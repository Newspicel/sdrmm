use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstellationError {
    SizeNotPowerOfTwo(usize),
    LabelCountMismatch { points: usize, labels: usize },
    BadLabel(u32),
    ZeroPower,
    UnsupportedOrder { family: &'static str, m: u32 },
}

impl fmt::Display for ConstellationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SizeNotPowerOfTwo(n) => {
                write!(f, "constellation size {n} is not 2^k with 1 <= k <= 32")
            }
            Self::LabelCountMismatch { points, labels } => {
                write!(f, "{points} points but {labels} labels")
            }
            Self::BadLabel(l) => write!(f, "label {l:#b} repeats or does not fit the bit width"),
            Self::ZeroPower => write!(f, "all-zero constellation cannot be normalised"),
            Self::UnsupportedOrder { family, m } => {
                write!(f, "{family} is not defined at order {m}")
            }
        }
    }
}

impl std::error::Error for ConstellationError {}
