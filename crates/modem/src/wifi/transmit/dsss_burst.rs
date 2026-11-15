use std::f32::consts::{FRAC_PI_2, PI};

use num_complex::Complex;

use super::{bytes_bits, lsb};
use crate::{
    spread::PnError,
    wifi::{
        WifiPhy,
        dsss::{
            CHIP_SAMPLES, LENGTH_EXTENSION, LONG_SFD, SHORT_SFD, SYMBOL as DSSS_SYMBOL, barker,
            cck_codeword, header_crc, signal_code,
        },
        scrambler::Scrambler,
    },
};

const LONG_SYNC_BITS: usize = 128;
const SHORT_SYNC_BITS: usize = 56;
const LONG_SEED: u8 = 0b110_1100;
const SHORT_SEED: u8 = 0b001_1011;
/// The SERVICE field's locked-clocks bit, set by every transmitter here.
const LOCKED_CLOCKS: u32 = 0x04;
const HEADER_FIELD_BITS: usize = 8;
const LENGTH_BITS: usize = 16;
const CRC_BITS: usize = 16;
const SFD_BITS: usize = 16;
const BYTE_BITS: usize = 8;
const CCK_BITS: usize = 11;
const CCK11_LABEL_BITS: usize = 6;
const CCK5_LABEL_BITS: usize = 2;
const DQPSK_BITS: usize = 2;

fn length_field(phy: WifiPhy, bytes: usize) -> (u32, u32) {
    let bits = bytes * BYTE_BITS;
    match phy {
        WifiPhy::Dsss2m => (bits.div_ceil(2) as u32, 0),
        WifiPhy::Cck5m5 => ((bits * 2).div_ceil(CCK_BITS) as u32, 0),
        WifiPhy::Cck11m => {
            let length = bits.div_ceil(CCK_BITS);
            let extension = length * CCK_BITS - bits >= BYTE_BITS;
            (
                length as u32,
                if extension {
                    u32::from(LENGTH_EXTENSION)
                } else {
                    0
                },
            )
        }
        _ => (bits as u32, 0),
    }
}

pub fn dsss(mpdu: &[u8], phy: WifiPhy, short_preamble: bool) -> Result<Vec<Complex<f32>>, PnError> {
    let sequence = barker()?;
    let (length, extension) = length_field(phy, mpdu.len());
    let mut header: Vec<bool> = lsb(u32::from(signal_code(phy)), HEADER_FIELD_BITS)
        .chain(lsb(LOCKED_CLOCKS | extension, HEADER_FIELD_BITS))
        .chain(lsb(length, LENGTH_BITS))
        .collect();
    let crc = header_crc(&header);
    header.extend((0..CRC_BITS).map(|bit| crc >> (CRC_BITS - 1 - bit) & 1 == 1));
    let (sync, sfd, seed) = if short_preamble {
        (vec![false; SHORT_SYNC_BITS], SHORT_SFD, SHORT_SEED)
    } else {
        (vec![true; LONG_SYNC_BITS], LONG_SFD, LONG_SEED)
    };
    let mut scrambler = Scrambler::new(seed);
    let mut scramble = |bits: Vec<bool>| -> Vec<bool> {
        bits.into_iter().map(|bit| scrambler.push(bit)).collect()
    };
    let preamble = scramble(
        sync.into_iter()
            .chain(lsb(u32::from(sfd), SFD_BITS))
            .collect(),
    );
    let header = scramble(header);
    let payload = scramble(bytes_bits(mpdu).collect());
    let mut chips: Vec<Complex<f32>> = Vec::new();
    let mut phase = 0.0f32;
    let spread = |phase: f32, chips: &mut Vec<Complex<f32>>| {
        chips.extend(
            sequence
                .iter()
                .map(|&chip| Complex::from_polar(chip, phase)),
        );
    };
    for &bit in &preamble {
        phase += if bit { PI } else { 0.0 };
        spread(phase, &mut chips);
    }
    if short_preamble {
        for pair in header.as_chunks::<2>().0 {
            phase += dqpsk(pair[0], pair[1]);
            spread(phase, &mut chips);
        }
    } else {
        for &bit in &header {
            phase += if bit { PI } else { 0.0 };
            spread(phase, &mut chips);
        }
    }
    match phy {
        WifiPhy::Dsss2m => {
            for pair in payload.chunks(2) {
                phase += dqpsk(pair[0], pair.get(1).copied().unwrap_or(false));
                spread(phase, &mut chips);
            }
        }
        WifiPhy::Cck5m5 | WifiPhy::Cck11m => cck(&payload, phy, phase, &mut chips),
        _ => {
            for &bit in &payload {
                phase += if bit { PI } else { 0.0 };
                spread(phase, &mut chips);
            }
        }
    }
    Ok(render_chips(&chips))
}

fn dqpsk(first: bool, second: bool) -> f32 {
    match (first, second) {
        (false, false) => 0.0,
        (false, true) => FRAC_PI_2,
        (true, true) => PI,
        (true, false) => 3.0 * FRAC_PI_2,
    }
}

fn cck(payload: &[bool], phy: WifiPhy, mut phi1: f32, chips: &mut Vec<Complex<f32>>) {
    let label_bits = if phy == WifiPhy::Cck11m {
        CCK11_LABEL_BITS
    } else {
        CCK5_LABEL_BITS
    };
    for (symbol, bits) in payload.chunks(label_bits + DQPSK_BITS).enumerate() {
        let bit = |index: usize| bits.get(index).copied().unwrap_or(false);
        phi1 += dqpsk(bit(0), bit(1)) + if symbol % 2 == 1 { PI } else { 0.0 };
        let labels: [bool; CCK11_LABEL_BITS] = std::array::from_fn(|index| bit(index + DQPSK_BITS));
        chips.extend(cck_codeword(phy, phi1, &labels[..label_bits]));
    }
}

fn render_chips(chips: &[Complex<f32>]) -> Vec<Complex<f32>> {
    let len = (chips.len() as f64 * CHIP_SAMPLES).ceil() as usize + DSSS_SYMBOL;
    (0..len)
        .map(|n| {
            let chip = (n as f64 / CHIP_SAMPLES) as usize;
            chips.get(chip).copied().unwrap_or_default()
        })
        .collect()
}
