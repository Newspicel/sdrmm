@group(0) @binding(3) var<storage, read> input: array<vec2<f32>>;
@group(0) @binding(4) var<storage, read> window: array<f32>;

fn sample(transform: u32, index: u32) -> vec2<f32> {
    let head = input[0];
    if (f32(transform) >= head.y) { return vec2<f32>(0.0); }
    return input[1u + transform * u32(head.x) + index] * window[index];
}
