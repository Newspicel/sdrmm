@group(0) @binding(3) var<uniform> shape: Shape;
@group(0) @binding(4) var<storage, read> samples: array<vec2<f32>>;

fn sample(transform: u32, index: u32) -> vec2<f32> {
    let slot = transform / shape.batches;
    let batch = transform % shape.batches;
    var lane = 0u;
    var start = shape.pre + batch * shape.batch_len;
    var len = shape.batch_len;
    if (slot >= shape.first_lane) {
        lane = slot - shape.first_lane + 1u;
        start = shape.pre + batch * shape.batch_len - shape.lead;
        len = shape.batch_len + shape.span - 1u;
    } else if (slot == 1u) {
        start = batch * shape.batch_len;
        len = shape.batch_len + shape.span + shape.order - 2u;
    }
    if (index >= len) { return vec2<f32>(0.0, 0.0); }
    return samples[lane * shape.window + start + index];
}
