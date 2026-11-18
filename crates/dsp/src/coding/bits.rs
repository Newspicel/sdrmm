mod hdlc;
mod line;
mod pack;
mod scrambler;
mod stream;
mod sync_word;

pub use self::{
    hdlc::{HDLC_FLAG, HdlcDeframer, HdlcFramer},
    line::{DifferentialDecoder, NrziDecoder, manchester_decode},
    pack::{
        bits_be, field_msb, pack_lsb, pack_msb, pack_msb_into, pack_msb_slice, reverse_byte,
        word_msb,
    },
    scrambler::{Descrambler, Scrambler},
    stream::{BitReader, BitWriter, byte_bits, crc_msb_inverted, sign_extend},
    sync_word::{SyncDetector, hamming_distance},
};
