use num_complex::Complex;
use sdrmm_dsp::Soft;

use super::tables::{FFT, Modulation, bin, data_carriers};

const SCALE: f32 = 32.0;
const LIMIT: f32 = 127.0;

pub fn demap(
    points: &[Complex<f32>; FFT],
    channel: &[Complex<f32>; FFT],
    gain: f32,
    modulation: Modulation,
    out: &mut [f32],
) {
    let bits = modulation.bits();
    for (index, carrier) in data_carriers().enumerate() {
        let point = points[bin(carrier)];
        let weight = channel[bin(carrier)].norm_sqr() / gain;
        let slot = &mut out[index * bits..(index + 1) * bits];
        match modulation {
            Modulation::Bpsk => slot[0] = point.re,
            Modulation::Qpsk => {
                let scale = std::f32::consts::SQRT_2;
                slot[0] = point.re * scale;
                slot[1] = point.im * scale;
            }
            Modulation::Qam16 => {
                let scale = 10.0f32.sqrt();
                axis16(point.re * scale, &mut slot[..2]);
                axis16(point.im * scale, &mut slot[2..]);
            }
            Modulation::Qam64 => {
                let scale = 42.0f32.sqrt();
                axis64(point.re * scale, &mut slot[..3]);
                axis64(point.im * scale, &mut slot[3..]);
            }
        }
        slot.iter_mut().for_each(|llr| *llr *= weight);
    }
}

fn axis16(y: f32, out: &mut [f32]) {
    out[0] = y;
    out[1] = 2.0 - y.abs();
}

fn axis64(y: f32, out: &mut [f32]) {
    out[0] = y;
    out[1] = 4.0 - y.abs();
    out[2] = 2.0 - (y.abs() - 4.0).abs();
}

pub fn soft(llr: f32) -> Soft {
    (llr * SCALE).round().clamp(-LIMIT, LIMIT) as Soft
}

pub fn map(modulation: Modulation, bits: &[bool]) -> Complex<f32> {
    let level = |bits: &[bool]| -> f32 {
        match bits {
            [b0] => {
                if *b0 {
                    1.0
                } else {
                    -1.0
                }
            }
            [b0, b1] => {
                let magnitude = if *b1 { 1.0 } else { 3.0 };
                if *b0 { magnitude } else { -magnitude }
            }
            [b0, b1, b2] => {
                let magnitude = match (*b1, *b2) {
                    (true, false) => 1.0,
                    (true, true) => 3.0,
                    (false, true) => 5.0,
                    (false, false) => 7.0,
                };
                if *b0 { magnitude } else { -magnitude }
            }
            _ => 0.0,
        }
    };
    match modulation {
        Modulation::Bpsk => Complex::new(level(&bits[..1]), 0.0),
        Modulation::Qpsk => Complex::new(level(&bits[..1]), level(&bits[1..2])) / 2.0f32.sqrt(),
        Modulation::Qam16 => Complex::new(level(&bits[..2]), level(&bits[2..4])) / 10.0f32.sqrt(),
        Modulation::Qam64 => Complex::new(level(&bits[..3]), level(&bits[3..6])) / 42.0f32.sqrt(),
    }
}
