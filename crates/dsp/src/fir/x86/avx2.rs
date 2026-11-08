use std::arch::x86_64::{
    __m256, _mm_add_ps, _mm_cvtss_f32, _mm_movehdup_ps, _mm_movehl_ps, _mm256_add_ps,
    _mm256_addsub_ps, _mm256_castps256_ps128, _mm256_extractf128_ps, _mm256_fmadd_ps,
    _mm256_loadu_ps, _mm256_permute_ps, _mm256_set1_ps, _mm256_setzero_ps, _mm256_storeu_ps,
};

use num_complex::Complex;

use crate::fir::{
    kernel::{BLOCK_FLOATS, GROUP_BLOCKS, Plan, interpolated_plane_tail, plane_tail, reach_of},
    run::Run,
};

type Block = [__m256; 2];

#[target_feature(enable = "avx2,fma")]
fn zero() -> Block {
    [_mm256_setzero_ps(); 2]
}

#[target_feature(enable = "avx2,fma")]
unsafe fn load_block(floats: *const f32, offset: usize) -> Block {
    unsafe {
        [
            _mm256_loadu_ps(floats.add(offset)),
            _mm256_loadu_ps(floats.add(offset + BLOCK_FLOATS / 2)),
        ]
    }
}

#[target_feature(enable = "avx2,fma")]
unsafe fn store(block: Block, lanes: &mut [f32; BLOCK_FLOATS]) {
    unsafe {
        _mm256_storeu_ps(lanes.as_mut_ptr(), block[0]);
        _mm256_storeu_ps(lanes.as_mut_ptr().add(BLOCK_FLOATS / 2), block[1]);
    }
}

#[target_feature(enable = "avx2,fma")]
fn add(a: Block, b: Block) -> Block {
    [_mm256_add_ps(a[0], b[0]), _mm256_add_ps(a[1], b[1])]
}

#[target_feature(enable = "avx2,fma")]
fn add_product(sum: Block, samples: Block, tap: f32) -> Block {
    let tap = _mm256_set1_ps(tap);
    [
        _mm256_fmadd_ps(samples[0], tap, sum[0]),
        _mm256_fmadd_ps(samples[1], tap, sum[1]),
    ]
}

#[target_feature(enable = "avx2,fma")]
unsafe fn samples<const FOLD: bool>(floats: *const f32, run: &Run, at: usize) -> Block {
    let (front, back) = run.offsets(at);
    let front = unsafe { load_block(floats, front) };
    if FOLD {
        add(front, unsafe { load_block(floats, back) })
    } else {
        front
    }
}

/// Blocks computed side by side. Each coefficient is broadcast once for all of them, and their
/// independent chains hide the latency one block would expose. Every block sums its taps in run
/// order, so an output does not depend on how the input was split.
#[target_feature(enable = "avx2,fma")]
unsafe fn real_group<const FOLD: bool, const BLOCKS: usize>(
    floats: *const f32,
    plan: &Plan<f32>,
) -> [Block; BLOCKS] {
    let mut sums = [zero(); BLOCKS];
    for run in &plan.runs {
        for (at, &value) in plan.values[run.range()].iter().enumerate() {
            for (block, sum) in sums.iter_mut().enumerate() {
                let samples = unsafe { samples::<FOLD>(floats.add(block * BLOCK_FLOATS), run, at) };
                *sum = add_product(*sum, samples, value);
            }
        }
    }
    sums
}

const REAL_GROUP: usize = GROUP_BLOCKS;

#[target_feature(enable = "avx2,fma")]
pub(super) fn real_blocks<const FOLD: bool>(floats: &[f32], plan: &Plan<f32>, out: &mut [f32]) {
    let Some(floats) = reach_of(floats, plan, out) else {
        return;
    };
    let base = floats.as_ptr();
    let (groups, rest) = out.as_chunks_mut::<{ REAL_GROUP * BLOCK_FLOATS }>();
    for (index, lanes) in groups.iter_mut().enumerate() {
        let at = index * REAL_GROUP * BLOCK_FLOATS;
        let sums = unsafe { real_group::<FOLD, REAL_GROUP>(base.add(at), plan) };
        unsafe { store_all(&sums, lanes) };
    }
    let done = groups.len() * REAL_GROUP * BLOCK_FLOATS;
    for (index, lanes) in rest
        .as_chunks_mut::<BLOCK_FLOATS>()
        .0
        .iter_mut()
        .enumerate()
    {
        let at = done + index * BLOCK_FLOATS;
        let sums = unsafe { real_group::<FOLD, 1>(base.add(at), plan) };
        unsafe { store_all(&sums, lanes) };
    }
}

#[target_feature(enable = "avx2,fma")]
unsafe fn store_all(sums: &[Block], lanes: &mut [f32]) {
    for (&sum, lanes) in sums.iter().zip(lanes.as_chunks_mut::<BLOCK_FLOATS>().0) {
        unsafe { store(sum, lanes) };
    }
}

#[target_feature(enable = "avx2,fma")]
fn rotated(by_re: __m256, by_im: __m256) -> __m256 {
    _mm256_addsub_ps(by_re, _mm256_permute_ps::<0xB1>(by_im))
}

/// [`real_group`] for complex taps, each block carrying a sum per tap part.
#[target_feature(enable = "avx2,fma")]
unsafe fn complex_group<const FOLD: bool, const BLOCKS: usize>(
    floats: *const f32,
    plan: &Plan<Complex<f32>>,
) -> [Block; BLOCKS] {
    let mut sums = [[zero(); 2]; BLOCKS];
    for run in &plan.runs {
        for (at, value) in plan.values[run.range()].iter().enumerate() {
            for (block, sum) in sums.iter_mut().enumerate() {
                let samples = unsafe { samples::<FOLD>(floats.add(block * BLOCK_FLOATS), run, at) };
                *sum = [
                    add_product(sum[0], samples, value.re),
                    add_product(sum[1], samples, value.im),
                ];
            }
        }
    }
    sums.map(|[by_re, by_im]| [rotated(by_re[0], by_im[0]), rotated(by_re[1], by_im[1])])
}

const COMPLEX_GROUP: usize = GROUP_BLOCKS / 2;

#[target_feature(enable = "avx2,fma")]
pub(super) fn complex_blocks<const FOLD: bool>(
    floats: &[f32],
    plan: &Plan<Complex<f32>>,
    out: &mut [f32],
) {
    let Some(floats) = reach_of(floats, plan, out) else {
        return;
    };
    let base = floats.as_ptr();
    let (groups, rest) = out.as_chunks_mut::<{ COMPLEX_GROUP * BLOCK_FLOATS }>();
    for (index, lanes) in groups.iter_mut().enumerate() {
        let at = index * COMPLEX_GROUP * BLOCK_FLOATS;
        let sums = unsafe { complex_group::<FOLD, COMPLEX_GROUP>(base.add(at), plan) };
        unsafe { store_all(&sums, lanes) };
    }
    let done = groups.len() * COMPLEX_GROUP * BLOCK_FLOATS;
    for (index, lanes) in rest
        .as_chunks_mut::<BLOCK_FLOATS>()
        .0
        .iter_mut()
        .enumerate()
    {
        let at = done + index * BLOCK_FLOATS;
        let sums = unsafe { complex_group::<FOLD, 1>(base.add(at), plan) };
        unsafe { store_all(&sums, lanes) };
    }
}

#[target_feature(enable = "avx2,fma")]
fn horizontal(sum: __m256) -> f32 {
    let half = _mm_add_ps(_mm256_castps256_ps128(sum), _mm256_extractf128_ps::<1>(sum));
    let pair = _mm_add_ps(half, _mm_movehl_ps(half, half));
    _mm_cvtss_f32(_mm_add_ps(pair, _mm_movehdup_ps(pair)))
}

#[target_feature(enable = "avx2,fma")]
pub(super) unsafe fn plane_dot<const N: usize>(
    re: [*const f32; N],
    im: [*const f32; N],
    taps: &[f32],
) -> [Complex<f32>; N] {
    let (blocks, _) = taps.as_chunks::<8>();
    let mut sums = [[_mm256_setzero_ps(); 2]; N];
    for (index, block) in blocks.iter().enumerate() {
        let taps = unsafe { _mm256_loadu_ps(block.as_ptr()) };
        let at = 8 * index;
        for ((sum, re), im) in sums.iter_mut().zip(re).zip(im) {
            unsafe {
                *sum = [
                    _mm256_fmadd_ps(_mm256_loadu_ps(re.add(at)), taps, sum[0]),
                    _mm256_fmadd_ps(_mm256_loadu_ps(im.add(at)), taps, sum[1]),
                ];
            }
        }
    }
    std::array::from_fn(|output| {
        let sum = Complex::new(horizontal(sums[output][0]), horizontal(sums[output][1]));
        unsafe { plane_tail(sum, re[output], im[output], taps, 8 * blocks.len()) }
    })
}

#[target_feature(enable = "avx2,fma")]
pub(super) fn plane_interpolated(
    re: &[f32],
    im: &[f32],
    lower: &[f32],
    slope: &[f32],
    mu: f32,
) -> Complex<f32> {
    let scale = _mm256_set1_ps(mu);
    let (re_blocks, re) = re.as_chunks::<8>();
    let (im_blocks, im) = im.as_chunks::<8>();
    let (lower_blocks, lower) = lower.as_chunks::<8>();
    let (slope_blocks, slope) = slope.as_chunks::<8>();
    let mut sums = [_mm256_setzero_ps(); 2];
    let blocks = re_blocks
        .iter()
        .zip(im_blocks)
        .zip(lower_blocks.iter().zip(slope_blocks));
    for ((re, im), (lower, slope)) in blocks {
        unsafe {
            let taps = _mm256_fmadd_ps(
                _mm256_loadu_ps(slope.as_ptr()),
                scale,
                _mm256_loadu_ps(lower.as_ptr()),
            );
            sums = [
                _mm256_fmadd_ps(_mm256_loadu_ps(re.as_ptr()), taps, sums[0]),
                _mm256_fmadd_ps(_mm256_loadu_ps(im.as_ptr()), taps, sums[1]),
            ];
        }
    }
    let sum = Complex::new(horizontal(sums[0]), horizontal(sums[1]));
    interpolated_plane_tail(sum, re, im, lower, slope, mu)
}
