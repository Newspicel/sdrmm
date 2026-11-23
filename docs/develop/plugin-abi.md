---
title: Plugins in any language
description: Freestanding WebAssembly ABI v5 reference for C, C++, Zig, Go, and AssemblyScript plugins.
---

Authoritative contract: `crates/wire/src/plugin.rs`. Must export linear `memory` (≤ 256 MiB), import only `sdrmm` below (no WASI/`env`), use little-endian numbers and UTF-8 JSON, and finish every call within 500 ms. The host loads ABI 5 only. An unknown plane code stops the channel with an error. One host instance serves control calls; each channel gets its own instance.

## Toolchains

| Language | Command | Notes |
|---|---|---|
| C / C++ | `zig cc -target wasm32-freestanding -O2 -nostdlib -Wl,--no-entry -Wl,--export-dynamic -o p.wasm p.c` | Or `clang --target=wasm32` + `wasm-ld` |
| Zig | `zig build-exe p.zig -target wasm32-freestanding -fno-entry -rdynamic -O ReleaseSmall` | `export fn` |
| AssemblyScript | `asc p.ts -o p.wasm -O --use abort=` | Allocate buffers with `heap.alloc` |
| Go | `tinygo build -target=wasm-unknown -o p.wasm .` | `//export name` (standard Go needs WASI) |

## Exports and control calls

`ptr`/`len` are `u32`, `handle` is `i32`, packed `i64` is `(ptr << 32) | len`. Returned buffers stay valid until the next host call on that instance.

- **Common:** `sdrmm_abi() -> 5`, `sdrmm_alloc(len) -> ptr`, `sdrmm_manifest() -> i64`, `sdrmm_present(ptr, len) -> i64` (if `kinds`), `sdrmm_error() -> i64`, `sdrmm_call(op, ptr, len) -> i64` (`{"ok": ...}` or `{"err": "..."}`).
- **Control ops (`sdrmm_call`):** `1` layout, `2` normalize, `3` filter, `4` tune, `5` feed, `6` width, `7` compose.
- **Decoder:** `sdrmm_open(rate, ptr, len) -> handle`, `sdrmm_apply(handle, ptr, len) -> i32`, `sdrmm_input(handle, samples) -> ptr`, `sdrmm_process(handle, samples, flags) -> ptr` (flag `1` = fill symbols), `sdrmm_close(handle)`, plus optional `sdrmm_retuned`, `sdrmm_position`, `sdrmm_clock`, `sdrmm_lo_artifact`, `sdrmm_gated`.
- **Encoder (`crates/wire/src/plugin/encoder.rs`):** `sdrmm_encoder_open`, `sdrmm_encoder_submit` (kind `0` `f32` audio / `1` bytes), `sdrmm_encoder_generate`, `sdrmm_encoder_close`.
- **Array (`crates/wire/src/plugin/processor.rs`):** `sdrmm_array_input`, `sdrmm_array_context`, `sdrmm_array_process`, optional `sdrmm_array_action`.

## `OutputHeader` (22 `u32` words)

Returned by `sdrmm_process` (`crates/wire/src/plugin/output.rs`; floats as raw IEEE-754 bits):

| Words | Meaning |
|---|---|
| `0, 1, 2` | Audio ptr, `f32` count (interleaved), audio rate Hz |
| `3, 4` / `5, 6` | Events JSON array ptr/len; Status JSON ptr/len (`0` = none/unchanged) |
| `7, 8` | Cumulative rejected frames, dropped blocks |
| `9, 10` | Picture records ptr/len (5 `u32` header + luma + RGB + optional image JSON) |
| `11..15` | Symbols ptr/count, reference ptr/count, plane (`0` none, `1` complex, `2` levels, `3` carriers) |
| `16, 17` / `18..21` | Symbol rate (`f64`, low word first); EVM, MER dB, margin, freq error Hz (`f32`) |

## Host imports (`sdrmm` module)

`log(level, ptr, len)`, `fir_new(taps_ptr, taps_len, dec)`, `fir_process(h, in, in_len, out, cap)`, `fft_new(len, inv)`, `fft_run(h, ptr, len)`, `resampler_new(ratio)`, `resampler_process(h, in, in_len, out, cap)`, `kernel_drop(h)`. Negative returns indicate refusal.

## Complete C example

Tested against the live host by `crates/plugins/tests/c_plugin.rs`:

```c
{{#include crates/plugin-example/c/burst.c}}
```
