---
title: GPU measurements
description: Apple M4 Max wgpu compute and WebGL2 waterfall benchmarks showing where GPU offload wins.
---

Apple M4 Max (40 GPU cores, Metal, `wgpu` 30.0.1, release; 8 warmups + 101 iterations including upload and readback). Raw CSV: [gpu-m4-max.csv](/docs/develop/data/gpu-m4-max.csv). Passive radar numbers: [Hardware tests](hardware-tests.md#measured-reference).

| Workload | CPU median | GPU median | Default |
|---|---:|---:|---|
| Spectrum, 64 × 4,096 | 0.48 ms | 0.33 ms | CPU |
| Spectrum, 64 × 32,768 | 4.89 ms | 1.23 ms | GPU |
| Spectrum, 64 × 65,536 | 11.3 ms | 2.29 ms | GPU |
| Spectrum, 64 × 262,144 | 56.0 ms | 7.89 ms | GPU |
| Filter bank, 13 bands × 8,192 | 79.5 µs | 177.3 µs | CPU |

- **`gpu-fft`:** portable WGSL (`f32`); software adapters rejected; CI `gpu` verifies Vulkan parity on Lavapipe.
- **Policy:** spectra < 32,768 pts stay on CPU; larger offload only if startup probe beats CPU by ≥ 20% (capped at 25% GPU time). Radar caps at 512 MiB and falls back to CPU on any error.
- **WebGL2 waterfall (Chrome 153 Metal vs SwiftShader):** `640×240` @ 2× = `0.076 ms` vs `2.236 ms`; `1280×720` @ 2× = `0.246 ms` vs `12.242 ms`. Repaints on change only (zero idle draws). Raw JSONL: [before](/docs/develop/data/rendering-m4-max-before.jsonl), [after](/docs/develop/data/rendering-m4-max-after.jsonl), [WebKit](/docs/develop/data/rendering-m4-max-webkit.jsonl).

<details>
<summary>Reproduce benchmarks</summary>

```sh
cargo test -p sdrmm-engine --no-default-features --features gpu-fft --release --lib -- --ignored --nocapture --test-threads=1 gpu::benchmarks
cargo test -p sdrmm-engine --no-default-features --features gpu-fft --release --lib -- --include-ignored --nocapture --test-threads=1 spectrum::tests gpu::caf::tests array::radar::gpu
node web/scripts/benchmark-gpu.mjs
```

</details>
