use num_complex::Complex;
use sdrmm_dsp::{ConvCode, math::fft::FftPair, puncture};

use super::{bytes_bits, lsb};
use crate::wifi::{
    ofdm::{
        map_point,
        tables::{
            CONV_POLYS, CP, EDGE, FFT, OCCUPIED, PILOT_CARRIERS, PILOT_VALUES, POLARITY_LEN, RATES,
            Rate, SERVICE_BITS, TAIL_BITS, bin, data_carriers, interleave_table, long_training,
            polarity, short_training,
        },
    },
    scrambler::keystream,
};

const OFDM_SEED: u8 = 0b101_1101;
const SHORT_TRAINING_SAMPLES: usize = 160;
const LONG_GUARD: usize = 32;
const RATE_BITS: usize = 4;
const LENGTH_BITS: usize = 12;

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
    let pilots = polarity()[index % POLARITY_LEN];
    let mut spectrum = [Complex::new(0.0f32, 0.0); FFT];
    for (carrier, point) in data_carriers().zip(points) {
        spectrum[bin(carrier)] = *point;
    }
    for (carrier, value) in PILOT_CARRIERS.iter().zip(PILOT_VALUES) {
        spectrum[bin(*carrier)] = Complex::new(value * pilots, 0.0);
    }
    fft.inverse(&mut spectrum);
    let scale = 1.0 / (OCCUPIED as f32).sqrt();
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
    let scale = 1.0 / (OCCUPIED as f32).sqrt();
    let mut short = [Complex::new(0.0f32, 0.0); FFT];
    let mut long = [Complex::new(0.0f32, 0.0); FFT];
    for carrier in -EDGE..=EDGE {
        short[bin(carrier)] = short_training(carrier);
        long[bin(carrier)] = Complex::new(long_training(carrier), 0.0);
    }
    fft.inverse(&mut short);
    fft.inverse(&mut long);
    out.extend((0..SHORT_TRAINING_SAMPLES).map(|n| short[n % FFT] * scale));
    out.extend(long[FFT - LONG_GUARD..].iter().map(|x| x * scale));
    out.extend(long.iter().chain(long.iter()).map(|x| x * scale));
}

#[must_use]
pub fn ofdm(mpdu: &[u8], mbps: u8) -> Vec<Complex<f32>> {
    let rate = rate(mbps);
    let code = ConvCode::new(&CONV_POLYS);
    let mut fft = FftPair::new(FFT);
    let mut out = Vec::new();
    preamble(&mut fft, &mut out);
    let length = mpdu.len() as u32;
    let mut signal: Vec<bool> = (0..RATE_BITS)
        .rev()
        .map(|bit| rate.code >> bit & 1 == 1)
        .collect();
    signal.push(false);
    signal.extend(lsb(length, LENGTH_BITS));
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
        *bit ^= keystream(&mut register);
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
