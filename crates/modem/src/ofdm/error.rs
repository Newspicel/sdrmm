use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfdmError {
    FftSize(usize),
    CyclicPrefix { fft: usize, cp: usize },
    NoDataSubcarriers,
    SubcarrierOutOfRange { offset: i32, fft: usize },
    DuplicateSubcarrier(i32),
    NotHermitian(i32),
    ShortStride { fft: usize, stride: usize },
    ShortTrainingEmpty,
    Repeats { what: &'static str, repeats: usize },
    LongGuard { fft: usize, guard: usize },
    EmptyPilotPattern,
    PilotCountMismatch { pilots: usize, values: usize },
}

impl fmt::Display for OfdmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FftSize(n) => write!(f, "FFT size {n} is not a power of two of at least 8"),
            Self::CyclicPrefix { fft, cp } => {
                write!(f, "cyclic prefix {cp} is not in 1..{fft}")
            }
            Self::NoDataSubcarriers => write!(f, "the map carries no data subcarrier"),
            Self::SubcarrierOutOfRange { offset, fft } => {
                write!(f, "subcarrier {offset} is outside a {fft}-point transform")
            }
            Self::DuplicateSubcarrier(k) => write!(f, "subcarrier {k} is mapped twice"),
            Self::NotHermitian(k) => {
                write!(f, "subcarrier {k} cannot be mirrored in a Hermitian map")
            }
            Self::ShortStride { fft, stride } => {
                write!(f, "short-training stride {stride} does not divide {fft}")
            }
            Self::ShortTrainingEmpty => write!(f, "the short-training stride energises no bin"),
            Self::Repeats { what, repeats } => {
                write!(
                    f,
                    "{what} repeats {repeats} times, fewer than the two a \
                    repetition metric needs"
                )
            }
            Self::LongGuard { fft, guard } => {
                write!(
                    f,
                    "long-training guard {guard} exceeds the {fft}-sample symbol"
                )
            }
            Self::EmptyPilotPattern => write!(f, "a pilot pattern needs values and a polarity"),
            Self::PilotCountMismatch { pilots, values } => {
                write!(f, "{pilots} pilot subcarriers but {values} base values")
            }
        }
    }
}

impl std::error::Error for OfdmError {}
