pub mod dsss;
pub mod ofdm;
mod receiver;
pub mod transmit;

use num_complex::Complex;
use sdrmm_dsp::crc32_ieee;

pub use self::receiver::Receiver;

pub const RATE_HZ: f64 = 20_000_000.0;
pub const MAX_MPDU_BYTES: usize = 4_095;
const FCS_BYTES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiPhy {
    Dsss1m,
    Dsss2m,
    Cck5m5,
    Cck11m,
    Ofdm { mbps: u8 },
}

#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    pub mpdu: &'a [u8],
    pub phy: WifiPhy,
    pub level_dbfs: f32,
    pub sample: u64,
}

pub trait Sink {
    fn accepts(&self, frame_control: u8) -> bool;
    fn frame(&mut self, frame: Frame<'_>);
}

#[must_use]
pub fn fcs_ok(mpdu: &[u8]) -> bool {
    let Some(split) = mpdu.len().checked_sub(FCS_BYTES) else {
        return false;
    };
    let (body, fcs) = mpdu.split_at(split);
    crc32_ieee(body).to_le_bytes() == fcs
}

#[must_use]
pub fn append_fcs(mut mpdu: Vec<u8>) -> Vec<u8> {
    let fcs = crc32_ieee(&mpdu);
    mpdu.extend_from_slice(&fcs.to_le_bytes());
    mpdu
}

pub(crate) fn level_dbfs(samples: &[Complex<f32>]) -> f32 {
    let mean = samples.iter().map(Complex::norm_sqr).sum::<f32>() / samples.len().max(1) as f32;
    10.0 * mean.max(1e-20).log10()
}

pub(crate) fn byte(bits: &[bool]) -> u8 {
    bits.iter()
        .enumerate()
        .fold(0u8, |acc, (bit, &value)| acc | u8::from(value) << bit)
}

#[cfg(test)]
mod tests;
