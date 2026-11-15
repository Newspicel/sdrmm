mod bch;
pub mod block;
pub mod bptc;
pub mod ccsds_rs;
pub mod conv;
pub mod conv7;
pub mod conv_soft;
mod crc;
mod digest;
mod gf;
mod golay;
mod hdlc;
mod mode_s;
pub mod prbs;
mod rds;
mod remainder;
mod rs129;
pub mod rs256;
mod rs64;
pub mod syndrome;
#[cfg(test)]
mod test_data;

pub use bch::{ermes_bch_decode, ermes_bch_encode, pocsag_bch_decode, pocsag_bch_encode};
pub use crc::{
    crc_msb_bits, crc4_msb, crc8_msb, crc16_ccitt, crc16_msb, crc16_msb_bits, crc16_x25,
    crc24_reflected, crc32_ieee, crc32_mpeg,
};
pub use digest::{lfsr_digest8, lfsr_digest8_reflect, lfsr_digest16};
pub use golay::{golay23_correct, golay23_encode, golay23_ok};
pub use hdlc::{hdlc_fcs_ok, hdlc_repair};
pub use mode_s::{
    mode_s_append_overlaid_parity, mode_s_append_parity, mode_s_bit_overlays,
    mode_s_fix_single_bit, mode_s_overlay, mode_s_syndrome,
};
pub use rds::{RdsOffset, rds_check_block, rds_correct_block, rds_encode_block, rds_syndrome};
pub use rs64::{rs64_decode, rs64_encode};
pub use rs129::rs129_parity;
