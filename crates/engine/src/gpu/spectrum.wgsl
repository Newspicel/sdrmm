@group(0) @binding(0) var<storage, read> input: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> data: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> params: array<u32>;
@group(0) @binding(3) var<storage, read> window: array<f32>;
@group(0) @binding(4) var<storage, read_write> power: array<f32>;
@group(0) @binding(5) var<storage, read_write> levels: array<f32>;

const SIZE = 0u;
const BITS = 1u;
const HOP = 2u;
const COUNT = 3u;
const AVERAGED = 4u;
const SCALE = 5u;

fn flat(id: vec3<u32>, groups: vec3<u32>) -> u32 {
    return id.x + id.y * groups.x * 256u;
}

@compute @workgroup_size(256)
fn prepare(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = flat(id, groups);
    let size = params[SIZE];
    let transform = index / size;
    if (transform >= params[COUNT]) { return; }
    let reversed = reverseBits(index % size) >> (32u - params[BITS]);
    data[index] = input[transform * params[HOP] + reversed] * window[reversed];
}

@compute @workgroup_size(256)
fn accumulate(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let bin = flat(id, groups);
    let size = params[SIZE];
    if (bin >= size) { return; }
    var sum = 0.0;
    for (var transform = 0u; transform < params[COUNT]; transform++) {
        let value = data[transform * size + bin];
        sum += dot(value, value);
    }
    power[bin] += sum;
}

@compute @workgroup_size(256)
fn finalize(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let bin = flat(id, groups);
    let size = params[SIZE];
    if (bin >= size) { return; }
    let mean = power[bin] * bitcast<f32>(params[SCALE]) / f32(max(params[AVERAGED], 1u));
    levels[(bin + size / 2u) % size] = 10.0 * log2(mean + 1e-24) * 0.30102999566;
    power[bin] = 0.0;
}
