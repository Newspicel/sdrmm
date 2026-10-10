use std::f32::consts::{FRAC_PI_2, PI};

use num_complex::Complex;
use sdrmm_dsp::{ConvCode, fft::FftPair, puncture};

use super::{
    WifiPhy,
    dsss::{CHIP_SAMPLES, SYMBOL as DSSS_SYMBOL, barker, cck_codeword, header_crc},
    ofdm::{
        map_point,
        tables::{
            CP, FFT, PILOT_CARRIERS, PILOT_VALUES, RATES, Rate, SERVICE_BITS, TAIL_BITS, bin,
            data_carriers, interleave_table, long_training, polarity, short_training,
        },
    },
};
use crate::spread::PnError;

const LONG_SYNC_BITS: usize = 128;
const SHORT_SYNC_BITS: usize = 56;
const LONG_SFD: u16 = 0xF3A0;
const SHORT_SFD: u16 = 0x05CF;
const LONG_SEED: u8 = 0b110_1100;
const SHORT_SEED: u8 = 0b001_1011;
const OFDM_SEED: u8 = 0b101_1101;

fn lsb(value: u32, bits: usize) -> impl Iterator<Item = bool> {
    (0..bits).map(move |bit| value >> bit & 1 == 1)
}

fn bytes_bits(bytes: &[u8]) -> impl Iterator<Item = bool> + '_ {
    bytes.iter().flat_map(|&byte| lsb(u32::from(byte), 8))
}

struct Scrambler(u8);

impl Scrambler {
    fn push(&mut self, bit: bool) -> bool {
        let out = bit ^ (self.0 >> 3 & 1 == 1) ^ (self.0 >> 6 & 1 == 1);
        self.0 = (self.0 << 1 | u8::from(out)) & 0x7F;
        out
    }
}

fn signal_code(phy: WifiPhy) -> u32 {
    match phy {
        WifiPhy::Dsss2m => 0x14,
        WifiPhy::Cck5m5 => 0x37,
        WifiPhy::Cck11m => 0x6E,
        _ => 0x0A,
    }
}

fn length_field(phy: WifiPhy, bytes: usize) -> (u32, u32) {
    let bits = bytes * 8;
    match phy {
        WifiPhy::Dsss2m => (bits.div_ceil(2) as u32, 0),
        WifiPhy::Cck5m5 => ((bits * 2).div_ceil(11) as u32, 0),
        WifiPhy::Cck11m => {
            let length = bits.div_ceil(11);
            let extension = u32::from(length * 11 - bits >= 8);
            (length as u32, extension << 7)
        }
        _ => (bits as u32, 0),
    }
}

pub fn dsss(mpdu: &[u8], phy: WifiPhy, short_preamble: bool) -> Result<Vec<Complex<f32>>, PnError> {
    let sequence = barker()?;
    let (length, extension) = length_field(phy, mpdu.len());
    let mut header: Vec<bool> = lsb(signal_code(phy), 8)
        .chain(lsb(0x04 | extension, 8))
        .chain(lsb(length, 16))
        .collect();
    let crc = header_crc(&header);
    header.extend((0..16).map(|bit| crc >> (15 - bit) & 1 == 1));
    let (sync, sfd, seed) = if short_preamble {
        (vec![false; SHORT_SYNC_BITS], SHORT_SFD, SHORT_SEED)
    } else {
        (vec![true; LONG_SYNC_BITS], LONG_SFD, LONG_SEED)
    };
    let mut scrambler = Scrambler(seed);
    let mut scramble = |bits: Vec<bool>| -> Vec<bool> {
        bits.into_iter().map(|bit| scrambler.push(bit)).collect()
    };
    let preamble = scramble(sync.into_iter().chain(lsb(u32::from(sfd), 16)).collect());
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
    let label_bits = if phy == WifiPhy::Cck11m { 6 } else { 2 };
    for (symbol, bits) in payload.chunks(label_bits + 2).enumerate() {
        let bit = |index: usize| bits.get(index).copied().unwrap_or(false);
        phi1 += dqpsk(bit(0), bit(1)) + if symbol % 2 == 1 { PI } else { 0.0 };
        let labels: [bool; 6] = std::array::from_fn(|index| bit(index + 2));
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

fn rate(mbps: u8) -> Rate {
    RATES
        .iter()
        .copied()
        .find(|rate| rate.mbps as u8 == mbps)
        .unwrap_or(RATES[0])
}

fn ofdm_symbol(
    fft: &mut FftPair,
    points: &[Complex<f32>],
    index: usize,
    out: &mut Vec<Complex<f32>>,
) {
    let pilots = polarity()[index % 127];
    let mut spectrum = [Complex::new(0.0f32, 0.0); FFT];
    for (carrier, point) in data_carriers().zip(points) {
        spectrum[bin(carrier)] = *point;
    }
    for (carrier, value) in PILOT_CARRIERS.iter().zip(PILOT_VALUES) {
        spectrum[bin(*carrier)] = Complex::new(value * pilots, 0.0);
    }
    fft.inverse(&mut spectrum);
    let scale = 1.0 / 52f32.sqrt();
    out.extend(spectrum[FFT - CP..].iter().map(|x| x * scale));
    out.extend(spectrum.iter().map(|x| x * scale));
}

fn modulate_bits(
    fft: &mut FftPair,
    rate: Rate,
    coded: &[bool],
    first: usize,
    out: &mut Vec<Complex<f32>>,
) {
    let table = interleave_table(rate);
    let bits = rate.modulation.bits();
    for (offset, block) in coded.chunks_exact(rate.coded_bits()).enumerate() {
        let mut interleaved = vec![false; block.len()];
        for (k, &position) in table.iter().enumerate() {
            interleaved[position] = block[k];
        }
        let points: Vec<Complex<f32>> = interleaved
            .chunks_exact(bits)
            .map(|chunk| map_point(rate.modulation, chunk))
            .collect();
        ofdm_symbol(fft, &points, first + offset, out);
    }
}

fn preamble(fft: &mut FftPair, out: &mut Vec<Complex<f32>>) {
    let scale = 1.0 / 52f32.sqrt();
    let mut short = [Complex::new(0.0f32, 0.0); FFT];
    let mut long = [Complex::new(0.0f32, 0.0); FFT];
    for carrier in -26..=26 {
        short[bin(carrier)] = short_training(carrier);
        long[bin(carrier)] = Complex::new(long_training(carrier), 0.0);
    }
    fft.inverse(&mut short);
    fft.inverse(&mut long);
    out.extend((0..160).map(|n| short[n % FFT] * scale));
    out.extend(long[FFT - 32..].iter().map(|x| x * scale));
    out.extend(long.iter().chain(long.iter()).map(|x| x * scale));
}

#[must_use]
pub fn ofdm(mpdu: &[u8], mbps: u8) -> Vec<Complex<f32>> {
    let rate = rate(mbps);
    let code = ConvCode::new(&[0o133, 0o171]);
    let mut fft = FftPair::new(FFT);
    let mut out = Vec::new();
    preamble(&mut fft, &mut out);
    let length = mpdu.len() as u32;
    let mut signal: Vec<bool> = (0..4).rev().map(|bit| rate.code >> bit & 1 == 1).collect();
    signal.push(false);
    signal.extend(lsb(length, 12));
    let parity = signal.iter().filter(|&&bit| bit).count() % 2 == 1;
    signal.push(parity);
    signal.extend([false; TAIL_BITS]);
    let mut coded = Vec::new();
    code.encode(&signal, &mut coded);
    modulate_bits(&mut fft, RATES[0], &coded, 0, &mut out);
    let symbols = rate.symbols(mpdu.len());
    let mut data: Vec<bool> = std::iter::repeat_n(false, SERVICE_BITS)
        .chain(bytes_bits(mpdu))
        .collect();
    data.resize(symbols * rate.data_bits, false);
    let mut register = OFDM_SEED;
    for bit in &mut data {
        let next = (register >> 6 ^ register >> 3) & 1;
        register = (register << 1 | next) & 0x7F;
        *bit ^= next == 1;
    }
    let tail = SERVICE_BITS + mpdu.len() * 8;
    data[tail..tail + TAIL_BITS].fill(false);
    let mut mother = Vec::new();
    code.encode(&data, &mut mother);
    let mut punctured = Vec::new();
    puncture(&mother, rate.coding.puncture(), &mut punctured);
    modulate_bits(&mut fft, rate, &punctured, 1, &mut out);
    out.extend(std::iter::repeat_n(Complex::new(0.0, 0.0), CP));
    out
}
