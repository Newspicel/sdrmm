use std::f64::consts::{PI, TAU};

use num_complex::Complex;

use super::super::preamble;

pub(super) const RATE: f64 = 9_600.0;
const ROLLOFF: f64 = 0.25;
const SPAN_SYMBOLS: usize = 10;

pub(super) struct Noise(u64);

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    fn uniform(&mut self) -> f64 {
        (f64::from(self.next_u32()) + 0.5) / 4_294_967_296.0
    }

    fn gaussian(&mut self) -> f64 {
        (-2.0 * self.uniform().ln()).sqrt() * (TAU * self.uniform()).cos()
    }

    pub fn tribit(&mut self) -> u8 {
        (self.next_u32() % 8) as u8
    }
}

fn phasor(tribit: u8) -> Complex<f64> {
    Complex::from_polar(1.0, PI / 4.0 * f64::from(tribit % 8))
}

fn rrc(sps: usize) -> Vec<f64> {
    let half = (SPAN_SYMBOLS * sps / 2) as isize;
    let taps: Vec<f64> = (-half..=half)
        .map(|n| {
            let t = n as f64 / sps as f64;
            if t == 0.0 {
                1.0 - ROLLOFF + 4.0 * ROLLOFF / PI
            } else if (4.0 * ROLLOFF * t).abs() == 1.0 {
                ROLLOFF / 2f64.sqrt()
                    * ((1.0 + 2.0 / PI) * (PI / (4.0 * ROLLOFF)).sin()
                        + (1.0 - 2.0 / PI) * (PI / (4.0 * ROLLOFF)).cos())
            } else {
                ((PI * t * (1.0 - ROLLOFF)).sin()
                    + 4.0 * ROLLOFF * t * (PI * t * (1.0 + ROLLOFF)).cos())
                    / (PI * t * (1.0 - (4.0 * ROLLOFF * t).powi(2)))
            }
        })
        .collect();
    let energy = taps.iter().map(|t| t * t).sum::<f64>().sqrt();
    taps.into_iter().map(|t| t / energy).collect()
}

pub(super) fn psk(symbols: &[u8], baud: f64, carrier_hz: f64) -> Vec<Complex<f32>> {
    let sps = (RATE / baud).round() as usize;
    let pulse = rrc(sps);
    let len = symbols.len() * sps + pulse.len();
    let mut shaped = vec![Complex::<f64>::default(); len];
    for (index, &tribit) in symbols.iter().enumerate() {
        let symbol = phasor(tribit);
        for (tap, &weight) in pulse.iter().enumerate() {
            shaped[index * sps + tap] += symbol * weight;
        }
    }
    shaped
        .iter()
        .enumerate()
        .map(|(n, &value)| {
            let mixed = value * Complex::from_polar(1.0, TAU * carrier_hz * n as f64 / RATE);
            Complex::new(mixed.re as f32, mixed.im as f32)
        })
        .collect()
}

fn lfsr(state: &mut u32, degree: u32, taps: &[u32]) -> u32 {
    let out = (*state >> (degree - 1)) & 1;
    let feedback = taps
        .iter()
        .fold(0, |acc, &tap| acc ^ ((*state >> (tap - 1)) & 1));
    *state = ((*state << 1) | feedback) & ((1 << degree) - 1);
    out
}

fn stanag_4285_frames(frames: usize, noise: &mut Noise) -> Vec<u8> {
    let mut symbols = Vec::new();
    for _ in 0..frames {
        let mut sync = 0b11010;
        for _ in 0..80 {
            symbols.push(4 * lfsr(&mut sync, 5, &[5, 2]) as u8);
        }
        let mut scrambler = 0x1ff;
        for index in 0..176 {
            let mut value = 0;
            for _ in 0..3 {
                value = (value << 1) | lfsr(&mut scrambler, 9, &[9, 4]);
            }
            let probe = matches!(index % 48, 32..=47);
            let data = if probe { 0 } else { noise.tribit() };
            symbols.push((data + value as u8) % 8);
        }
    }
    symbols
}

pub(super) fn stanag_4285(seconds: f64, carrier_hz: f64, seed: u64) -> Vec<Complex<f32>> {
    let frames = (seconds * 2_400.0 / 256.0).ceil() as usize;
    psk(
        &stanag_4285_frames(frames, &mut Noise::new(seed)),
        2_400.0,
        carrier_hz,
    )
}

pub(super) fn stanag_4529(seconds: f64, carrier_hz: f64, seed: u64) -> Vec<Complex<f32>> {
    let frames = (seconds * 1_200.0 / 256.0).ceil() as usize;
    psk(
        &stanag_4285_frames(frames, &mut Noise::new(seed)),
        1_200.0,
        carrier_hz,
    )
}

fn scrambler_110a() -> [u8; 160] {
    let mut state: u32 = 0xbad;
    std::array::from_fn(|_| {
        for _ in 0..8 {
            let carry = (state >> 11) & 1;
            state = ((state << 1) & 0xfff) ^ if carry == 1 { 0b0000_0101_0011 } else { 0 };
        }
        (state & 7) as u8
    })
}

pub(super) fn preamble_110a(d1: u8, d2: u8, blocks: u8) -> Vec<u8> {
    let mut symbols = Vec::new();
    for count in (0..blocks).rev() {
        let counts = [4 + (count >> 4 & 3), 4 + (count >> 2 & 3), 4 + (count & 3)];
        let segments = preamble::COMMON
            .iter()
            .copied()
            .chain([d1, d2])
            .chain(counts)
            .chain([0]);
        for pattern in segments {
            symbols.extend(preamble::tribits(pattern));
        }
    }
    symbols
}

pub(super) fn mil_110a(
    seconds: f64,
    carrier_hz: f64,
    preamble: Option<(u8, u8)>,
    seed: u64,
) -> Vec<Complex<f32>> {
    let mut noise = Noise::new(seed);
    let scramble = scrambler_110a();
    let mut symbols = preamble.map_or_else(Vec::new, |(d1, d2)| preamble_110a(d1, d2, 3));
    let (unknown, known) = match preamble {
        Some((6 | 4, 4) | (7, 6 | 7)) => (32, 16),
        _ => (20, 20),
    };
    let total = (seconds * 2_400.0) as usize;
    let mut index = 0;
    while symbols.len() < total {
        let data = if index % (unknown + known) < unknown {
            noise.tribit()
        } else {
            0
        };
        symbols.push((data + scramble[index % scramble.len()]) % 8);
        index += 1;
    }
    psk(&symbols, 2_400.0, carrier_hz)
}

pub(super) fn stanag_4539(seconds: f64, carrier_hz: f64, seed: u64) -> Vec<Complex<f32>> {
    let mut noise = Noise::new(seed);
    let probe: Vec<u8> = (0..32)
        .map(|k| ((k % 16 / 4) * (k % 4) * 2) as u8)
        .collect();
    let mut symbols = Vec::new();
    while (symbols.len() as f64) < seconds * 2_400.0 {
        symbols.extend((0..256).map(|_| noise.tribit()));
        symbols.extend(&probe);
    }
    psk(&symbols, 2_400.0, carrier_hz)
}

struct ToneModem {
    first_hz: f64,
    spacing_hz: f64,
    tones: usize,
    doppler_hz: f64,
    symbol_s: f64,
}

fn parallel_tones(modem: &ToneModem, seconds: f64, offset_hz: f64, seed: u64) -> Vec<Complex<f32>> {
    let mut noise = Noise::new(seed);
    let len = (seconds * RATE) as usize;
    let mut phases: Vec<f64> = vec![0.0; modem.tones];
    let guard = modem.symbol_s - 1.0 / modem.spacing_hz;
    let mut symbol = usize::MAX;
    (0..len)
        .map(|n| {
            let t = n as f64 / RATE;
            let current = (t / modem.symbol_s) as usize;
            if current != symbol {
                symbol = current;
                for phase in &mut phases {
                    *phase += PI / 2.0 * f64::from(noise.next_u32() % 4);
                }
            }
            let local = t - current as f64 * modem.symbol_s - guard;
            let data: Complex<f64> = phases
                .iter()
                .enumerate()
                .map(|(k, phase)| {
                    let hz = modem.first_hz + modem.spacing_hz * k as f64;
                    Complex::from_polar(1.0, TAU * hz * local + phase)
                })
                .sum();
            let doppler = Complex::from_polar(2.0, TAU * modem.doppler_hz * t);
            let value = (data + doppler) * Complex::from_polar(1.0, TAU * offset_hz * t)
                / (modem.tones as f64).sqrt();
            Complex::new(value.re as f32, value.im as f32)
        })
        .collect()
}

pub(super) fn thirty_nine_tone(seconds: f64, offset_hz: f64, seed: u64) -> Vec<Complex<f32>> {
    let modem = ToneModem {
        first_hz: 675.0,
        spacing_hz: 56.25,
        tones: 39,
        doppler_hz: 393.75,
        symbol_s: 0.0225,
    };
    parallel_tones(&modem, seconds, offset_hz, seed)
}

pub(super) fn sixteen_tone(seconds: f64, offset_hz: f64, seed: u64) -> Vec<Complex<f32>> {
    let modem = ToneModem {
        first_hz: 935.0,
        spacing_hz: 110.0,
        tones: 16,
        doppler_hz: 605.0,
        symbol_s: 1.0 / 75.0,
    };
    parallel_tones(&modem, seconds, offset_hz, seed)
}

pub(super) fn ale(seconds: f64, offset_hz: f64, seed: u64) -> Vec<Complex<f32>> {
    let mut noise = Noise::new(seed);
    let samples_per_symbol = RATE / 125.0;
    let mut phase: f64 = 0.0;
    let mut tone = 0.0;
    let mut symbol = usize::MAX;
    (0..(seconds * RATE) as usize)
        .map(|n| {
            let current = (n as f64 / samples_per_symbol) as usize;
            if current != symbol {
                symbol = current;
                tone = 750.0 + 250.0 * f64::from(noise.next_u32() % 8);
            }
            phase = (phase + TAU * (tone + offset_hz) / RATE).rem_euclid(TAU);
            Complex::from_polar(1.0, phase as f32)
        })
        .collect()
}

pub(super) fn awgn(signal: &[Complex<f32>], snr_db_in_3k: f64, seed: u64) -> Vec<Complex<f32>> {
    let mut noise = Noise::new(seed);
    let power = signal.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>() / signal.len() as f64;
    let in_band = power / 10f64.powf(snr_db_in_3k / 10.0);
    let sigma = (in_band * RATE / 3_000.0 / 2.0).sqrt();
    signal
        .iter()
        .map(|s| {
            Complex::new(
                s.re + (sigma * noise.gaussian()) as f32,
                s.im + (sigma * noise.gaussian()) as f32,
            )
        })
        .collect()
}
