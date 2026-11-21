use std::sync::Arc;

use num_complex::Complex;
use sdrmm_dsp::math::fft::FftPair;
use wgpu::util::DeviceExt;

use super::*;
use crate::gpu::buffer;

type C32 = Complex<f32>;

fn gpu() -> Arc<Context> {
    Arc::clone(crate::gpu::context().expect("GPU adapter"))
}

fn noise(len: usize, seed: u64) -> Vec<C32> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    };
    (0..len).map(|_| C32::new(next(), next())).collect()
}

fn flat(values: &[C32]) -> Vec<[f32; 2]> {
    values.iter().map(|value| [value.re, value.im]).collect()
}

fn upload(context: &Context, values: &[[f32; 2]]) -> wgpu::Buffer {
    context
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(values),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        })
}

fn run(context: &Context, fft: &FftBatch, data: &wgpu::Buffer, len: usize) -> Vec<C32> {
    run_from(context, fft, data, len, 0)
}

fn run_from(
    context: &Context,
    fft: &FftBatch,
    data: &wgpu::Buffer,
    len: usize,
    input: usize,
) -> Vec<C32> {
    let bytes = len as u64 * 8;
    let mut encoder = context
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        fft.dispatch_from(&mut pass, input);
    }
    let readback = buffer(
        &context.device,
        "FFT test readback",
        bytes,
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    );
    encoder.copy_buffer_to_buffer(data, 0, &readback, 0, bytes);
    context.queue.submit([encoder.finish()]);
    readback.map_async(wgpu::MapMode::Read, .., |result| result.expect("mapped"));
    context
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU finished");
    let view = readback.get_mapped_range(..).expect("mapped range");
    bytemuck::cast_slice::<u8, [f32; 2]>(&view)
        .iter()
        .map(|value| C32::new(value[0], value[1]))
        .collect()
}

fn relative_error(actual: &[C32], expected: &[C32]) -> f64 {
    let error: f64 = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| f64::from((a - b).norm_sqr()))
        .sum();
    let norm: f64 = expected.iter().map(|b| f64::from(b.norm_sqr())).sum();
    (error / norm).sqrt()
}

fn expected(input: &[C32], size: usize, inverse: bool) -> Vec<C32> {
    let mut pair = FftPair::new(size);
    let mut out = input.to_vec();
    for chunk in out.chunks_mut(size) {
        if inverse {
            pair.inverse(chunk);
        } else {
            pair.forward(chunk);
        }
    }
    out
}

#[test]
fn sizes_outside_16_to_2_22_are_refused() {
    for size in [0, 1, 8, 24, 3 << 10, 1 << 23] {
        assert!(check_size(size).is_err(), "{size}");
    }
    for bits in 4..=22 {
        assert!(check_size(1 << bits).is_ok(), "{bits}");
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn every_power_of_two_from_16_to_2_22_matches_rustfft() {
    let context = gpu();
    for bits in 4..=22 {
        let size = 1usize << bits;
        let batches = ((1usize << 16) / size).max(1) + 1;
        let input = noise(size * batches, bits as u64);
        for inverse in [false, true] {
            let data = upload(&context, &flat(&input));
            let fft = FftBatch::new(&context, &data, size, batches, inverse).expect("fft");
            let actual = run(&context, &fft, &data, input.len());
            let error = relative_error(&actual, &expected(&input, size, inverse));
            assert!(error < 1e-5, "size {size} inverse {inverse}: {error}");
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn a_range_leaves_the_rest_of_the_buffer_alone() {
    let context = gpu();
    let size = 64;
    let input = noise(5 * size, 7);
    let original = flat(&input);
    let data = upload(&context, &original);
    let plan = Transforms {
        size,
        offset: 2 * size,
        batches: 2,
        inverse: false,
    };
    let fft = FftBatch::over(&context, &data, plan).expect("fft");
    let actual = run(&context, &fft, &data, input.len());
    let untouched: Vec<C32> = original
        .iter()
        .map(|value| C32::new(value[0], value[1]))
        .collect();
    assert_eq!(actual[..2 * size], untouched[..2 * size]);
    assert_eq!(actual[4 * size..], untouched[4 * size..]);
    let transformed = expected(&input[2 * size..4 * size], size, false);
    assert!(relative_error(&actual[2 * size..4 * size], &transformed) < 1e-5);
}

fn options() -> Vec<Options> {
    let mut all = Vec::new();
    for radix in [4, 8, 16] {
        for (span, columns) in [(4096, 1), (2048, 2), (1024, 4), (1024, 1)] {
            for twiddles in [Twiddles::Computed, Twiddles::Table] {
                all.push(Options {
                    radix,
                    span,
                    columns,
                    twiddles,
                });
            }
        }
    }
    all
}

#[test]
fn every_layout_covers_the_size_and_ends_in_data() {
    for options in options() {
        for bits in 4..=22 {
            let size = 1usize << bits;
            let layout = Layout::new(size, 3, options, true).unwrap();
            let loaded = Layout::new(size, 3, options, false).unwrap();
            assert!(loaded.passes.len() <= layout.passes.len());
            let loaded_covers: usize = loaded.radices.iter().chain(&loaded.passes).product();
            assert_eq!(loaded_covers, size);
            let covered: usize = layout.radices.iter().chain(&layout.passes).product();
            assert_eq!(covered, size, "{options:?} {size}");
            assert!(layout.passes.is_empty() || layout.passes.len() % 2 == 1);
            assert!(layout.threads <= MAX_THREADS && layout.tile * layout.columns <= options.span);
            assert!(
                layout
                    .radices
                    .iter()
                    .chain(&layout.passes)
                    .all(|&r| r <= options.radix)
            );
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn every_option_matches_rustfft() {
    let context = gpu();
    for options in options() {
        for bits in [4, 5, 6, 10, 12, 13, 15, 17, 20] {
            let size = 1usize << bits;
            let batches = 3;
            let input = noise(size * (batches + 1), bits as u64);
            for inverse in [false, true] {
                let data = upload(&context, &flat(&input));
                let plan = Transforms {
                    size,
                    offset: size,
                    batches,
                    inverse,
                };
                let fft = FftBatch::with_options(&context, &data, plan, options).expect("fft");
                let actual = run(&context, &fft, &data, input.len());
                let error =
                    relative_error(&actual[size..], &expected(&input[size..], size, inverse));
                assert!(
                    error < 1e-5,
                    "{options:?} size {size} inverse {inverse}: {error}"
                );
                assert_eq!(actual[..size], input[..size]);
            }
        }
    }
}

const WINDOWED: &str = "
@group(0) @binding(3) var<storage, read> input: array<vec2<f32>>;
@group(0) @binding(4) var<storage, read> window: array<f32>;
@group(0) @binding(5) var<storage, read> params: array<u32>;

fn sample(transform: u32, index: u32) -> vec2<f32> {
    if (transform >= params[1]) { return vec2<f32>(0.0); }
    return input[transform * params[0] + index] * window[index];
}
";

fn windowed_reference(
    samples: &[C32],
    window: &[f32],
    hop: usize,
    count: usize,
    batches: usize,
) -> Vec<C32> {
    let size = window.len();
    let mut out = vec![C32::default(); size * batches];
    for (t, chunk) in out.chunks_mut(size).enumerate().take(count) {
        for (i, value) in chunk.iter_mut().enumerate() {
            *value = samples[t * hop + i] * window[i];
        }
    }
    expected(&out, size, false)
}

#[test]
#[ignore = "requires a GPU adapter"]
fn windowed_loads_match_rustfft_of_windowed_hops() {
    let context = gpu();
    for bits in [4, 10, 12, 13, 15, 16, 17, 18] {
        let size = 1usize << bits;
        let (batches, count, hop) = (3, 2, size / 2 + 3);
        let window: Vec<f32> = (0..size).map(|i| 0.5 + i as f32 / size as f32).collect();
        let streams = [
            noise(2 * hop + size, bits as u64),
            noise(2 * hop + size, 99),
        ];
        let inputs: Vec<_> = streams.iter().map(|s| upload(&context, &flat(s))).collect();
        let params = crate::gpu::compute::initialized(&context, &[hop as u32, count as u32]);
        let window_buffer = crate::gpu::compute::initialized(&context, &window);
        let data = upload(&context, &vec![[0.0; 2]; size * batches]);
        let plan = Transforms {
            size,
            offset: 0,
            batches,
            inverse: false,
        };
        let loader = Loader {
            wgsl: WINDOWED,
            bindings: inputs
                .iter()
                .map(|input| vec![(3, input), (4, &window_buffer), (5, &params)])
                .collect(),
            in_place: false,
        };
        let fft = FftBatch::loaded(&context, &data, plan, &loader).expect("fft");
        for (input, stream) in streams.iter().enumerate() {
            let actual = run_from(&context, &fft, &data, size * batches, input);
            let want = windowed_reference(stream, &window, hop, count, batches);
            let error = relative_error(&actual, &want);
            assert!(error < 1e-5, "size {size} input {input}: {error}");
            assert!(actual[count * size..].iter().all(|v| v.norm() == 0.0));
        }
    }
}
