struct Term {
    group: u32,
    tap: u32,
    factor: vec2<f32>,
}

@group(0) @binding(3) var<uniform> shape: Shape;
@group(0) @binding(4) var<storage, read> weights: array<vec2<f32>>;
@group(0) @binding(5) var<storage, read> terms: array<Term>;

fn against_reference(slot: u32, batch: u32, m: u32) -> vec2<f32> {
    let reference = data[batch * shape.fft_len + m];
    return mul(data[(slot * shape.batches + batch) * shape.fft_len + m], vec2<f32>(reference.x, -reference.y));
}

/// Residual of lane `transform / batches` after the clutter model, loaded straight into the
/// inverse FFT.
fn sample(transform: u32, index: u32) -> vec2<f32> {
    let lane = transform / shape.batches;
    let batch = transform % shape.batches;
    let product = against_reference(shape.first_lane + lane, batch, index);
    if (shape.eca == 0u) {
        return product * shape.scale;
    }
    var kernel = vec2<f32>(0.0, 0.0);
    for (var j = 0u; j < 2u * shape.taps; j++) {
        let term = terms[batch * 2u * shape.taps + j];
        let at = ((term.group * shape.lanes + lane) * shape.taps + term.tap) * shape.fft_len + index;
        kernel += mul(term.factor, weights[at]);
    }
    return (product - mul(kernel, against_reference(1u, batch, index))) * shape.scale;
}
