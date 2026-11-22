use std::f64::consts::TAU;

use num_complex::Complex;

use super::planes::Planes;
use crate::math::fft::Transform;

/// Turns the branch sums into the wanted bins with one FFT and a phase per bin, which beats the
/// twiddle table once most bins are wanted.
#[derive(Clone)]
pub(super) struct Spectral {
    fft: Transform<f32>,
    buf: Vec<Complex<f32>>,
    slots: Vec<usize>,
    phases: Vec<Complex<f32>>,
}

impl std::fmt::Debug for Spectral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spectral")
            .field("bins", &self.buf.len())
            .field("slots", &self.slots)
            .finish_non_exhaustive()
    }
}

impl Spectral {
    pub(super) fn new(bins: usize, wanted: &[i32]) -> Self {
        let phases = (0..bins)
            .flat_map(|phase| wanted.iter().map(move |&bin| phase_turn(bins, phase, bin)))
            .collect();
        Self {
            fft: Transform::forward(bins),
            buf: vec![Complex::default(); bins],
            slots: wanted.iter().map(|&bin| slot(bins, bin)).collect(),
            phases,
        }
    }

    pub(super) fn rotate(&mut self, partial: &Planes, phase: usize, sums: &mut Planes) {
        for ((value, &re), &im) in self.buf.iter_mut().zip(&partial.re).zip(&partial.im) {
            *value = Complex::new(re, im);
        }
        self.fft.process(&mut self.buf);
        let wanted = self.slots.len();
        let row = &self.phases[phase * wanted..(phase + 1) * wanted];
        for (((&slot, &turn), re), im) in self
            .slots
            .iter()
            .zip(row)
            .zip(sums.re.iter_mut())
            .zip(sums.im.iter_mut())
        {
            let value = self.buf[slot] * turn;
            *re = value.re;
            *im = value.im;
        }
    }
}

fn slot(bins: usize, bin: i32) -> usize {
    bin.rem_euclid(bins as i32) as usize
}

fn phase_turn(bins: usize, phase: usize, bin: i32) -> Complex<f32> {
    let turns = f64::from(bin) * ((phase + 1) % bins) as f64 / bins as f64;
    let (sin, cos) = (-TAU * turns).sin_cos();
    Complex::new(cos as f32, sin as f32)
}

#[cfg(test)]
mod tests {
    use super::{super::planes::twiddle_table, *};
    use crate::tuning::channelizer::kernel::rotate;

    #[test]
    fn the_fft_matches_the_twiddle_table() {
        let bins = 16;
        let wanted: Vec<i32> = (-8..8).collect();
        let partial = Planes {
            re: (0..bins).map(|k| k as f32 * 0.07 - 0.4).collect(),
            im: (0..bins).map(|k| 0.25 - k as f32 * 0.03).collect(),
        };
        let tables = twiddle_table(bins, &wanted);
        let mut spectral = Spectral::new(bins, &wanted);
        for (phase, table) in tables.iter().enumerate() {
            let mut expected = Planes::zeroed(wanted.len());
            let mut got = Planes::zeroed(wanted.len());
            rotate(&partial, table, wanted.len(), &mut expected);
            spectral.rotate(&partial, phase, &mut got);
            for (a, b) in got
                .re
                .iter()
                .chain(&got.im)
                .zip(expected.re.iter().chain(&expected.im))
            {
                assert!((a - b).abs() < 1e-4, "phase {phase}: {a} vs {b}");
            }
        }
    }
}
