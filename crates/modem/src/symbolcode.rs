mod biphase;
mod differential;
mod manchester;
mod nrz;
mod pair;

pub use biphase::{BiphaseDecoder, BiphaseEncoder, BiphasePair, BiphaseVariant, biphase_decode};
pub use differential::{
    DifferentialDecoder, DifferentialEncoder, DifferentialSymbolDecoder, DifferentialSymbolEncoder,
};
pub use manchester::{
    ManchesterConvention, manchester_decode, manchester_decode_pair, manchester_encode,
};
pub use nrz::{NrziConvention, NrziDecoder, NrziEncoder, nrz_decode, nrz_encode};
pub use pair::{Alignment, PairDecode};

#[cfg(test)]
mod tests;
