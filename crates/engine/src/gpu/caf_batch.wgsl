@group(0) @binding(0) var<uniform> shape: Shape;
@group(0) @binding(2) var<storage, read_write> spectra: array<vec2<f32>>;
@group(0) @binding(3) var<storage, read_write> sums: array<vec2<f32>>;
@group(0) @binding(4) var<storage, read> turns: array<vec2<f32>>;
@group(0) @binding(5) var<storage, read> ranges: array<vec2<u32>>;
@group(0) @binding(8) var<storage, read_write> cube: array<vec2<f32>>;
@group(0) @binding(9) var<storage, read> taper: array<f32>;

fn multiply(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

fn conjugate(a: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x, -a.y);
}

fn flat_index(id: vec3<u32>, groups: vec3<u32>) -> u32 {
    return id.x + id.y * groups.x * 256u;
}

/// Cross spectrum of `slot` against the reference, `slot` times the reference's conjugate.
fn product(slot: u32, batch: u32, m: u32) -> vec2<f32> {
    let reference = spectra[batch * shape.fft_len + m];
    return multiply(spectra[(slot * shape.batches + batch) * shape.fft_len + m], conjugate(reference));
}

@compute @workgroup_size(256)
fn group_sums(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = flat_index(id, groups);
    if (index >= shape.groups * shape.vectors * shape.fft_len) { return; }
    let m = index % shape.fft_len;
    let vector = (index / shape.fft_len) % shape.vectors;
    let group = index / (shape.fft_len * shape.vectors);
    var slot = 1u;
    var turn = vector;
    if (vector >= shape.shifts) {
        let lane_tap = vector - shape.shifts;
        slot = shape.first_lane + lane_tap / shape.taps;
        turn = shape.shifts + lane_tap % shape.taps;
    }
    let range = ranges[group];
    let per_batch = shape.shifts + shape.taps;
    var total = vec2<f32>(0.0, 0.0);
    for (var batch = range.x; batch < range.y; batch++) {
        let phase = turns[(group * shape.batches + batch) * per_batch + turn];
        total += multiply(phase, product(slot, batch, m));
    }
    sums[index] = total;
}

@compute @workgroup_size(256)
fn gather(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = flat_index(id, groups);
    if (index >= shape.lanes * shape.gates * shape.batches) { return; }
    let gate = index % shape.gates;
    let batch = (index / shape.gates) % shape.batches;
    let lane = index / (shape.gates * shape.batches);
    let source = ((shape.first_lane + lane) * shape.batches + batch) * shape.fft_len + gate + shape.lead;
    let row = (lane * shape.gates + gate) * shape.batches + batch;
    cube[row] = spectra[source] * taper[batch];
}

@compute @workgroup_size(256)
fn shift(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = flat_index(id, groups);
    let half = shape.batches / 2u;
    if (index >= shape.lanes * shape.gates * half) { return; }
    let low = index / half * shape.batches + index % half;
    let value = cube[low];
    cube[low] = cube[low + half];
    cube[low + half] = value;
}
