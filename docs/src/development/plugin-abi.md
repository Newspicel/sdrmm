# Plugins in any language

A plugin is a core WebAssembly module. Rust gets the SDK; any other language that builds
`wasm32` without WASI implements the ABI below. The contract lives in
`crates/wire/src/plugin.rs`; this page follows it.

## Toolchains

| Language | Build | Notes |
|---|---|---|
| C, C++ | `zig cc -target wasm32-freestanding -O2 -nostdlib -Wl,--no-entry -Wl,--export-dynamic -o p.wasm p.c` | Or `clang --target=wasm32` with the same flags and `wasm-ld` installed |
| Zig | `zig build-exe p.zig -target wasm32-freestanding -fno-entry -rdynamic -O ReleaseSmall` | `export fn` for each export |
| AssemblyScript | `asc p.ts -o p.wasm -O --use abort=` | Hand out buffers from `heap.alloc`, which the GC never moves or frees |
| Go | `tinygo build -target=wasm-unknown -o p.wasm .` | `//export name` on each function. Standard Go needs WASI or JS imports and is refused |

## Module rules

- Export the linear memory as `memory`, your only one. It grows to at most 256 MiB; a larger
  `memory.grow` fails.
- Import nothing but the `sdrmm` module below: no WASI, no `env`. An unknown import fails the
  load.
- Numbers in memory are little-endian. Strings are UTF-8 JSON.
- The host opens one instance for control calls and one more per channel, so per-channel state
  can be global.
- A call that runs past half a second stops its channel.

## Exports

| Export | Signature | Does |
|---|---|---|
| `sdrmm_abi` | `() -> u32` | ABI version, currently `1` |
| `sdrmm_alloc` | `(len: u32) -> ptr` | A buffer for the host to write `len` bytes into |
| `sdrmm_manifest` | `() -> i64` | Packed manifest JSON |
| `sdrmm_call` | `(op: i32, ptr, len) -> i64` | A control call, JSON in, packed JSON reply out |
| `sdrmm_open` | `(rate: f64, ptr, len) -> i32` | Opens a channel at `rate` Hz with settings JSON; a handle, or `-1` |
| `sdrmm_apply` | `(handle, ptr, len) -> i32` | New settings JSON; `0`, or `-1` |
| `sdrmm_input` | `(handle, samples: u32) -> ptr` | Where the host writes the next block |
| `sdrmm_process` | `(handle, samples: u32) -> ptr` | Runs the block; where the output header is |
| `sdrmm_retuned` | `(handle)` | Optional. The channel moved; drop what you accumulated |
| `sdrmm_close` | `(handle)` | The channel is gone |
| `sdrmm_error` | `() -> i64` | Packed message for the last `-1` |

`ptr`, `len` and `handle` are `u32`, `u32` and `i32`. A packed `i64` is `ptr << 32 | len`. Every
buffer you return stays yours and must stay valid until the host's next call into the instance.
A buffer from `sdrmm_alloc` is yours again once the call it was written for returns.

## Control calls

`sdrmm_call` answers `{"ok": value}` or `{"err": "message"}`.

| `op` | Input | `ok` value |
|---|---|---|
| `1` layout | Settings | `{"input_rate_hz": 48000.0, "band": [-6000.0, 6000.0], "audio_channels": 1}` |
| `2` normalize | Settings, `{}` for the defaults | The settings with every default filled in |
| `3` present | `{"kind": "burst", "data": {...}}` | A `Presentation`: `{"summary": "Burst", "fields": [...]}` |

Settings are the JSON object your form produces. Treat absent fields as defaults.

## Manifest

```json
{
  "abi": 1,
  "descriptor": {
    "type_id": "burst", "name": "Burst", "summary": "Strong bursts",
    "family": "utility", "bandwidth_hz": 12000.0, "input_rate_hz": 48000.0,
    "emits": ["burst"]
  },
  "form": [
    { "key": "threshold_db", "label": "Threshold",
      "control": { "type": "number", "min": -60.0, "max": 0.0, "step": 1.0, "unit": "dB" } }
  ],
  "kinds": [
    { "kind": "burst", "name": "Burst", "noun": "Bursts", "caps": ["log"],
      "hue": "amber", "lifetime": { "type": "instant" } }
  ]
}
```

`descriptor`, `form` entries and `kinds` entries are the `ChannelDescriptor`, `SettingField` and
`EventKindEntry` schemas in [`openapi.json`](https://github.com/Newspicel/sdrmm/blob/main/openapi.json).
Optional: `home_hz`, the frequency a new channel starts on, and `processor`, a `ProcessorType`.

## One block

1. `sdrmm_input(handle, n)` returns a pointer; the host writes `n` samples there, each an `f32` I
   then an `f32` Q.
2. `sdrmm_process(handle, n)` returns a pointer to nine `u32` words:

| Word | Holds |
|---|---|
| 0, 1 | Audio pointer and length in `f32` samples, interleaved by `audio_channels` |
| 2 | Audio rate in Hz |
| 3, 4 | Events: pointer and byte length of a JSON array, `0` for none |
| 5, 6 | Status: pointer and byte length of a JSON object, `0` when unchanged |
| 7 | Frames heard but failing their checks, counted over the channel's life |
| 8 | Blocks dropped, counted over the channel's life |

```json
[{"kind": "burst", "data": {"db": -12.5}}]
```

```json
{"health": {"lock": true, "snr_db": 21.5, "freq_error_hz": 30.0, "errors_per_s": 0.0}}
```

A status may also carry `readouts`, a list of fields like those a presentation has.

## Host imports

The `sdrmm` module runs the heavy kernels natively on your memory. Samples are the same
interleaved `f32` pairs. A negative return is a refusal.

| Import | Signature | Does |
|---|---|---|
| `log` | `(level: u32, ptr, len)` | Writes a UTF-8 line to the server log |
| `fir_new` | `(taps_ptr, taps_len, decimation: u32) -> i32` | A complex FIR over real `f32` taps |
| `fir_process` | `(handle, in_ptr, in_len, out_ptr, out_cap) -> i32` | Samples written |
| `fft_new` | `(len: u32, inverse: u32) -> i32` | An FFT of `len` points |
| `fft_run` | `(handle, ptr, len) -> i32` | Transforms `len` samples in place |
| `resampler_new` | `(ratio: f64) -> i32` | A fractional resampler, output rate over input rate |
| `resampler_process` | `(handle, in_ptr, in_len, out_ptr, out_cap) -> i32` | Samples written |
| `kernel_drop` | `(handle)` | Frees a kernel |

## Example

A complete plugin in C without libc. It builds as in the table and is tested against the real
host by `crates/plugins/tests/c_plugin.rs`.

```c
{{#include ../../../crates/plugin-example/c/burst.c}}
```

## Transmit and arrays

A transmitter sets `can_transmit` and adds the exports in `crates/wire/src/plugin/transmit.rs`:

| Export | Signature |
|---|---|
| `sdrmm_tx_open` | `(rate: f64, ptr, len) -> handle` |
| `sdrmm_tx_apply` | `(handle, ptr, len) -> i32` |
| `sdrmm_tx_submit` | `(handle, kind: u32, ptr, len) -> i32`, kind `0` audio as `f32`, `1` frame bytes |
| `sdrmm_tx_generate` | `(handle, max_samples: u32) -> i64`, packed pointer and sample count of I/Q |
| `sdrmm_tx_close` | `(handle)` |

An array processor declares `processor` in the manifest and adds the exports in
`crates/wire/src/plugin/processor.rs`. `sdrmm_array_input` takes every lane's block one after the
other; `sdrmm_array_process` returns a ten-word header laid out in `ArrayHeader` there.
