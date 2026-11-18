mod dsss_burst;
mod ofdm_burst;

pub use self::{dsss_burst::dsss, ofdm_burst::ofdm};

/// A 24 Mbit/s OFDM frame of `bytes` filler bytes and its FCS, as busy air for tests.
#[must_use]
pub fn filler_frame(bytes: usize) -> Vec<num_complex::Complex<f32>> {
    let mpdu = super::append_fcs((0..bytes).map(|k| (k * 37 + 11) as u8).collect());
    ofdm(&mpdu, 24)
}

fn lsb(value: u32, bits: usize) -> impl Iterator<Item = bool> {
    (0..bits).map(move |bit| value >> bit & 1 == 1)
}

fn bytes_bits(bytes: &[u8]) -> impl Iterator<Item = bool> + '_ {
    bytes.iter().flat_map(|&byte| lsb(u32::from(byte), 8))
}
