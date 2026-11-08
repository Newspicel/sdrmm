use std::arch::x86_64::{
    __m512, __mmask16, _mm512_add_ps, _mm512_fmadd_ps, _mm512_fmaddsub_ps, _mm512_loadu_ps,
    _mm512_maskz_loadu_ps, _mm512_permute_ps, _mm512_reduce_add_ps, _mm512_set1_ps,
    _mm512_setzero_ps, _mm512_storeu_ps,
};

use num_complex::Complex;

use crate::fir::{
    kernel::{BLOCK_FLOATS, Plan, reach_of},
    run::Run,
};

const SETS: usize = 4;
const WIDTH: usize = 16;
const _: () = assert!(WIDTH == BLOCK_FLOATS);

#[target_feature(enable = "avx512f")]
unsafe fn load_block(floats: *const f32, offset: usize) -> __m512 {
    unsafe { _mm512_loadu_ps(floats.add(offset)) }
}

#[target_feature(enable = "avx512f")]
unsafe fn store(block: __m512, lanes: &mut [f32; BLOCK_FLOATS]) {
    unsafe { _mm512_storeu_ps(lanes.as_mut_ptr(), block) };
}

#[target_feature(enable = "avx512f")]
fn add_product(sum: __m512, samples: __m512, tap: f32) -> __m512 {
    _mm512_fmadd_ps(samples, _mm512_set1_ps(tap), sum)
}

#[target_feature(enable = "avx512f")]
unsafe fn samples<const FOLD: bool>(floats: *const f32, run: &Run, at: usize) -> __m512 {
    let (front, back) = run.offsets(at);
    let front = unsafe { load_block(floats, front) };
    if FOLD {
        _mm512_add_ps(front, unsafe { load_block(floats, back) })
    } else {
        front
    }
}

#[target_feature(enable = "avx512f")]
unsafe fn real_sum<const FOLD: bool>(floats: *const f32, plan: &Plan<f32>) -> __m512 {
    let mut sets = [_mm512_setzero_ps(); SETS];
    for run in &plan.runs {
        let (groups, rest) = plan.values[run.range()].as_chunks::<SETS>();
        for (index, group) in groups.iter().enumerate() {
            for (lane, (set, &value)) in sets.iter_mut().zip(group).enumerate() {
                let samples = unsafe { samples::<FOLD>(floats, run, SETS * index + lane) };
                *set = add_product(*set, samples, value);
            }
        }
        for (lane, (set, &value)) in sets.iter_mut().zip(rest).enumerate() {
            let samples = unsafe { samples::<FOLD>(floats, run, SETS * groups.len() + lane) };
            *set = add_product(*set, samples, value);
        }
    }
    _mm512_add_ps(
        _mm512_add_ps(sets[0], sets[1]),
        _mm512_add_ps(sets[2], sets[3]),
    )
}

#[target_feature(enable = "avx512f")]
pub(super) fn real_blocks<const FOLD: bool>(floats: &[f32], plan: &Plan<f32>, out: &mut [f32]) {
    let Some(floats) = reach_of(floats, plan, out) else {
        return;
    };
    let base = floats.as_ptr();
    for (index, lanes) in out.as_chunks_mut::<BLOCK_FLOATS>().0.iter_mut().enumerate() {
        unsafe {
            store(
                real_sum::<FOLD>(base.add(index * BLOCK_FLOATS), plan),
                lanes,
            )
        };
    }
}

#[target_feature(enable = "avx512f")]
fn rotated(by_re: __m512, by_im: __m512) -> __m512 {
    _mm512_fmaddsub_ps(by_re, _mm512_set1_ps(1.0), _mm512_permute_ps::<0xB1>(by_im))
}

#[target_feature(enable = "avx512f")]
unsafe fn complex_sum<const FOLD: bool>(floats: *const f32, plan: &Plan<Complex<f32>>) -> __m512 {
    let mut sets = [[_mm512_setzero_ps(); 2]; SETS / 2];
    for run in &plan.runs {
        let (groups, rest) = plan.values[run.range()].as_chunks::<{ SETS / 2 }>();
        for (index, group) in groups.iter().enumerate() {
            for (lane, (set, value)) in sets.iter_mut().zip(group).enumerate() {
                let samples = unsafe { samples::<FOLD>(floats, run, SETS / 2 * index + lane) };
                *set = [
                    add_product(set[0], samples, value.re),
                    add_product(set[1], samples, value.im),
                ];
            }
        }
        for (lane, (set, value)) in sets.iter_mut().zip(rest).enumerate() {
            let at = SETS / 2 * groups.len() + lane;
            let samples = unsafe { samples::<FOLD>(floats, run, at) };
            *set = [
                add_product(set[0], samples, value.re),
                add_product(set[1], samples, value.im),
            ];
        }
    }
    rotated(
        _mm512_add_ps(sets[0][0], sets[1][0]),
        _mm512_add_ps(sets[0][1], sets[1][1]),
    )
}

#[target_feature(enable = "avx512f")]
pub(super) fn complex_blocks<const FOLD: bool>(
    floats: &[f32],
    plan: &Plan<Complex<f32>>,
    out: &mut [f32],
) {
    let Some(floats) = reach_of(floats, plan, out) else {
        return;
    };
    let base = floats.as_ptr();
    for (index, lanes) in out.as_chunks_mut::<BLOCK_FLOATS>().0.iter_mut().enumerate() {
        unsafe {
            store(
                complex_sum::<FOLD>(base.add(index * BLOCK_FLOATS), plan),
                lanes,
            )
        };
    }
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
