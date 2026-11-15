mod dsss_burst;
mod ofdm_burst;

pub use self::{dsss_burst::dsss, ofdm_burst::ofdm};

fn lsb(value: u32, bits: usize) -> impl Iterator<Item = bool> {
    (0..bits).map(move |bit| value >> bit & 1 == 1)
}

fn bytes_bits(bytes: &[u8]) -> impl Iterator<Item = bool> + '_ {
    bytes.iter().flat_map(|&byte| lsb(u32::from(byte), 8))
}
