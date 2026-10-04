use std::arch::x86_64::{
    __m512, __mmask16, _mm512_add_ps, _mm512_fmadd_ps, _mm512_fmaddsub_ps, _mm512_loadu_ps,
    _mm512_maskz_loadu_ps, _mm512_permute_ps, _mm512_reduce_add_ps, _mm512_set1_ps,
    _mm512_setzero_ps, _mm512_storeu_ps,
};

use num_complex::Complex;

use crate::fir::kernel::{BLOCK_FLOATS, Plan, Tap};

const SETS: usize = 4;
const WIDTH: usize = 16;
const _: () = assert!(WIDTH == BLOCK_FLOATS);

#[target_feature(enable = "avx512f")]
unsafe fn load_block(floats: *const f32, offset: usize) -> __m512 {
    unsafe { _mm512_loadu_ps(floats.add(offset)) }
}

#[target_feature(enable = "avx512f")]
fn store(block: __m512) -> [f32; BLOCK_FLOATS] {
    let mut lanes = [0.0; BLOCK_FLOATS];
    unsafe { _mm512_storeu_ps(lanes.as_mut_ptr(), block) };
    lanes
}

#[target_feature(enable = "avx512f")]
fn add_product(sum: __m512, samples: __m512, tap: f32) -> __m512 {
    _mm512_fmadd_ps(samples, _mm512_set1_ps(tap), sum)
}

#[target_feature(enable = "avx512f")]
unsafe fn samples<const FOLD: bool, C>(floats: *const f32, tap: &Tap<C>) -> __m512 {
    let front = unsafe { load_block(floats, tap.front) };
    if FOLD {
        _mm512_add_ps(front, unsafe { load_block(floats, tap.back) })
    } else {
        front
    }
}

#[target_feature(enable = "avx512f")]
pub(super) fn real_block<const FOLD: bool>(
    floats: &[f32],
    plan: &Plan<f32>,
) -> [f32; BLOCK_FLOATS] {
    let floats = floats[..plan.floats].as_ptr();
    let mut sets = [_mm512_setzero_ps(); SETS];
    let (groups, rest) = plan.taps.as_chunks::<SETS>();
    for group in groups {
        for (set, tap) in sets.iter_mut().zip(group) {
            *set = add_product(
                *set,
                unsafe { samples::<FOLD, f32>(floats, tap) },
                tap.value,
            );
        }
    }
    for tap in rest {
        sets[0] = add_product(
            sets[0],
            unsafe { samples::<FOLD, f32>(floats, tap) },
            tap.value,
        );
    }
    store(_mm512_add_ps(
        _mm512_add_ps(sets[0], sets[1]),
        _mm512_add_ps(sets[2], sets[3]),
    ))
}

#[target_feature(enable = "avx512f")]
unsafe fn complex_step<const FOLD: bool>(
    sums: [__m512; 2],
    floats: *const f32,
    tap: &Tap<Complex<f32>>,
) -> [__m512; 2] {
    let samples = unsafe { samples::<FOLD, Complex<f32>>(floats, tap) };
    [
        add_product(sums[0], samples, tap.value.re),
        add_product(sums[1], samples, tap.value.im),
    ]
}

#[target_feature(enable = "avx512f")]
fn rotated(by_re: __m512, by_im: __m512) -> __m512 {
    _mm512_fmaddsub_ps(by_re, _mm512_set1_ps(1.0), _mm512_permute_ps::<0xB1>(by_im))
}

#[target_feature(enable = "avx512f")]
pub(super) fn complex_block<const FOLD: bool>(
    floats: &[f32],
    plan: &Plan<Complex<f32>>,
) -> [f32; BLOCK_FLOATS] {
    let floats = floats[..plan.floats].as_ptr();
    let mut sets = [[_mm512_setzero_ps(); 2]; SETS / 2];
    let (groups, rest) = plan.taps.as_chunks::<{ SETS / 2 }>();
    for group in groups {
        for (set, tap) in sets.iter_mut().zip(group) {
            *set = unsafe { complex_step::<FOLD>(*set, floats, tap) };
        }
    }
    for tap in rest {
        sets[0] = unsafe { complex_step::<FOLD>(sets[0], floats, tap) };
    }
    store(rotated(
        _mm512_add_ps(sets[0][0], sets[1][0]),
        _mm512_add_ps(sets[0][1], sets[1][1]),
    ))
}

fn tail_mask(len: usize) -> __mmask16 {
    ((1u32 << (len % WIDTH)) - 1) as __mmask16
}

#[target_feature(enable = "avx512f")]
unsafe fn load_tail(values: *const f32, mask: __mmask16) -> __m512 {
    unsafe { _mm512_maskz_loadu_ps(mask, values) }
}

#[target_feature(enable = "avx512f")]
pub(super) unsafe fn plane_dot<const N: usize>(
    re: [*const f32; N],
    im: [*const f32; N],
    taps: &[f32],
) -> [Complex<f32>; N] {
    let (blocks, rest) = taps.as_chunks::<WIDTH>();
    let mut sums = [[_mm512_setzero_ps(); 2]; N];
    for (index, block) in blocks.iter().enumerate() {
        let taps = unsafe { _mm512_loadu_ps(block.as_ptr()) };
        let at = WIDTH * index;
        for ((sum, re), im) in sums.iter_mut().zip(re).zip(im) {
            unsafe {
                *sum = [
                    _mm512_fmadd_ps(_mm512_loadu_ps(re.add(at)), taps, sum[0]),
                    _mm512_fmadd_ps(_mm512_loadu_ps(im.add(at)), taps, sum[1]),
                ];
            }
        }
    }
    if !rest.is_empty() {
        let mask = tail_mask(rest.len());
        let taps = unsafe { load_tail(rest.as_ptr(), mask) };
        let at = WIDTH * blocks.len();
        for ((sum, re), im) in sums.iter_mut().zip(re).zip(im) {
            unsafe {
                *sum = [
                    _mm512_fmadd_ps(load_tail(re.add(at), mask), taps, sum[0]),
                    _mm512_fmadd_ps(load_tail(im.add(at), mask), taps, sum[1]),
                ];
            }
        }
    }
    sums.map(|[re, im]| Complex::new(_mm512_reduce_add_ps(re), _mm512_reduce_add_ps(im)))
}

#[target_feature(enable = "avx512f")]
unsafe fn interpolated_step(
    sums: [__m512; 2],
    [re, im, lower, slope]: [__m512; 4],
    scale: __m512,
) -> [__m512; 2] {
    let taps = _mm512_fmadd_ps(slope, scale, lower);
    [
        _mm512_fmadd_ps(re, taps, sums[0]),
        _mm512_fmadd_ps(im, taps, sums[1]),
    ]
}

#[target_feature(enable = "avx512f")]
pub(super) fn plane_interpolated(
    re: &[f32],
    im: &[f32],
    lower: &[f32],
    slope: &[f32],
    mu: f32,
) -> Complex<f32> {
    let scale = _mm512_set1_ps(mu);
    let planes = [re, im, lower, slope];
    let blocks = re.len() / WIDTH;
    let mut sums = [_mm512_setzero_ps(); 2];
    for block in 0..blocks {
        let at = WIDTH * block;
        let loaded = planes.map(|plane| unsafe { _mm512_loadu_ps(plane[at..at + WIDTH].as_ptr()) });
        sums = unsafe { interpolated_step(sums, loaded, scale) };
    }
    let at = WIDTH * blocks;
    if at < re.len() {
        let mask = tail_mask(re.len() - at);
        let loaded = planes.map(|plane| unsafe { load_tail(plane[at..].as_ptr(), mask) });
        sums = unsafe { interpolated_step(sums, loaded, scale) };
    }
    Complex::new(_mm512_reduce_add_ps(sums[0]), _mm512_reduce_add_ps(sums[1]))
}
