//! The DVB FEC chains DVB-S, DVB-S2, DVB-T and DVB-T2 share: the convolutional plus
//! Reed-Solomon chain, the BCH plus LDPC chain and the baseband frame.

pub mod bb;
pub mod bch;
pub mod dvbs;
pub mod ldpc;

use sdrmm_dsp::PrbsSpec;

pub const PACKET: usize = 188;
pub const CODEWORD: usize = 204;
pub const SYNC: u8 = 0x47;
pub const INVERTED_SYNC: u8 = 0xB8;

/// Energy dispersal: 1 + x^14 + x^15 from 100101010000000.
pub const DISPERSAL: PrbsSpec = PrbsSpec {
    stages: 15,
    taps: 0x6000,
    seed: 0x00A9,
};

/// A punctured rate of the K=7 mother code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PunctureRate {
    Half,
    TwoThirds,
    ThreeQuarters,
    FiveSixths,
    SevenEighths,
}

impl PunctureRate {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Half => "1/2",
            Self::TwoThirds => "2/3",
            Self::ThreeQuarters => "3/4",
            Self::FiveSixths => "5/6",
            Self::SevenEighths => "7/8",
        }
    }
}
