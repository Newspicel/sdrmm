mod bch;
pub mod block;
pub mod bptc;
mod burst;
pub mod ccsds_rs;
mod chase;
pub mod conv;
pub mod conv7;
pub mod conv_soft;
mod crc;
mod crc_parity;
mod digest;
pub mod fano;
mod golay;
pub mod hamming;
mod hdlc;
pub mod ldpc;
pub mod prbs;
mod remainder;
mod rs129;
pub mod rs256;
pub mod syndrome;
mod tail_biting;
#[cfg(test)]
mod test_data;
mod trellis;

pub use bch::{Bch, BchScratch, ShortBch};
pub use burst::BurstCode;
pub use chase::chase;
pub use crc::{
    crc_msb_bits, crc4_msb, crc8_msb, crc16_ccitt, crc16_genibus, crc16_genibus_ok, crc16_msb,
    crc16_msb_bits, crc16_x25, crc24_reflected, crc32_ieee, crc32_mpeg, push_crc16_genibus,
};
pub use crc_parity::CrcParity;
pub use digest::{lfsr_digest8, lfsr_digest8_reflect, lfsr_digest16};
pub use golay::{golay23_correct, golay23_encode, golay23_ok};
pub use hdlc::{hdlc_fcs_ok, hdlc_repair};
pub use rs129::rs129_parity;
pub use tail_biting::TailBitingViterbi;
pub use trellis::TrellisCode;
