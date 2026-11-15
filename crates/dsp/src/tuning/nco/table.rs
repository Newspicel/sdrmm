use std::sync::LazyLock;

use num_complex::Complex;

const TABLE_BITS: u32 = 9;
const TABLE_LEN: usize = 1 << TABLE_BITS;
const LOOKUP_SHIFT: u32 = u32::BITS;
const INDEX_SHIFT: u32 = u32::BITS - TABLE_BITS;
const FRACTION_MASK: u32 = (1 << INDEX_SHIFT) - 1;
const DELTA_PER_STEP: f32 = std::f32::consts::TAU / TABLE_LEN as f32 / (1u32 << INDEX_SHIFT) as f32;
const SIXTH: f32 = 1.0 / 6.0;
/// One turn in the 64-bit phase word.
pub(super) const PHASE_SCALE: f64 = 18_446_744_073_709_551_616.0;
const HALF_PHASE_SCALE: f64 = PHASE_SCALE / 2.0;

pub(super) type Table = [Complex<f32>; TABLE_LEN];

pub(super) static PHASORS: LazyLock<Table> = LazyLock::new(|| {
    std::array::from_fn(|index| {
        let phase = std::f64::consts::TAU * index as f64 / TABLE_LEN as f64;
        let (sin, cos) = phase.sin_cos();
        Complex::new(cos as f32, sin as f32)
    })
});

#[inline(always)]
pub(super) fn phasor(table: &Table, phase: u64) -> Complex<f32> {
    let top = (phase >> LOOKUP_SHIFT) as u32;
    let first = table[(top >> INDEX_SHIFT) as usize];
    let delta = (top & FRACTION_MASK) as f32 * DELTA_PER_STEP;
    let square = delta * delta;
    let sin = delta * (1.0 - square * SIXTH);
    let cos = 1.0 - square * 0.5;
    Complex::new(
        first.re * cos - first.im * sin,
        first.im * cos + first.re * sin,
    )
}

/// The unit phasor at `turns` of a cycle, from the oscillator's table. The whole turns go by a
/// truncating cast rather than a library remainder, and the rest wraps into the phase word.
#[must_use]
pub fn unit_phasor(turns: f64) -> Complex<f32> {
    let fraction = turns - turns as i64 as f64;
    let phase = ((fraction * HALF_PHASE_SCALE) as i64 as u64).wrapping_shl(1);
    phasor(&PHASORS, phase)
}
