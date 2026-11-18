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

Every plugin:

| Export | Signature | Does |
|---|---|---|
| `sdrmm_abi` | `() -> u32` | ABI version, currently `4` |
| `sdrmm_alloc` | `(len: u32) -> ptr` | A buffer for the host to write `len` bytes into |
| `sdrmm_manifest` | `() -> i64` | Packed manifest JSON |
| `sdrmm_present` | `(ptr, len) -> i64` | Only with `kinds`. `{"kind": "burst", "data": {...}}` in, a packed reply out whose `ok` is a `Presentation`: `{"summary": "Burst", "fields": [...]}` |
| `sdrmm_error` | `() -> i64` | Packed message for the last `-1` |
| `sdrmm_call` | `(op: i32, ptr, len) -> i64` | With a decoder or encoder. A control call, JSON in, packed JSON reply out |

A decoder:

| Export | Signature | Does |
|---|---|---|
| `sdrmm_open` | `(rate: f64, ptr, len) -> i32` | Opens a channel at `rate` Hz with channel settings JSON; a handle, or `-1` |
| `sdrmm_apply` | `(handle, ptr, len) -> i32` | New channel settings JSON; `0`, or `-1` |
| `sdrmm_input` | `(handle, samples: u32) -> ptr` | Where the host writes the next block |
| `sdrmm_process` | `(handle, samples: u32, flags: u32) -> ptr` | Runs the block; where the output header is. Flag `1`: fill the symbols |
| `sdrmm_retuned` | `(handle)` | Optional. The channel moved; drop what you accumulated |
| `sdrmm_position` | `(handle, ptr, len)` | Optional. The receiver's position JSON, or `null` |
| `sdrmm_clock` | `(handle, unix_ns: i64)` | Optional. When the next block's first sample was captured |
| `sdrmm_lo_artifact` | `(handle, offset_hz: f64)` | Optional. Where the receiver's LO artifact sits, NaN once outside |
| `sdrmm_gated` | `(handle) -> i32` | Optional. `0` when the channel wants nothing while squelched |
| `sdrmm_close` | `(handle)` | The channel is gone |

`ptr`, `len` and `handle` are `u32`, `u32` and `i32`. A packed `i64` is `ptr << 32 | len`. Every
buffer you return stays yours and must stay valid until the host's next call into the instance.
A buffer from `sdrmm_alloc` is yours again once the call it was written for returns.

## Control calls

`sdrmm_call` and `sdrmm_present` answer `{"ok": value}` or `{"err": "message"}`.

| `op` | Input | `ok` value |
|---|---|---|
| `1` layout | Settings | `{"input_rate_hz": 48000.0, "band": [-6000.0, 6000.0], "audio_channels": 1}` |
| `2` normalize | Settings, `{}` for the defaults | The settings with every default filled in |
| `3` filter | Settings | `{"type": "lowpass", "taps": 65, "cutoff_hz": 6000.0}`, a `band` with `low_hz` and `high_hz`, or `{"type": "passthrough"}` |
| `4` tune | `{"settings": {...}, "signal": {...}}` | `{"settings": {...}, "offset_hz": 0.0}` |
| `5` feed | `{"format": "...", "event": {...}}` | The frame's bytes as an array |
| `6` width | Encoder settings | Width in Hz |
| `7` compose | A filled message form | `{"frame": [bytes]}` or `{"audio": [samples]}` |

Settings are the JSON object your form produces. Treat absent fields as defaults. A lowpass may
add `decimation` and `stopband_db`; the host designs its taps.

## Manifest

```json
{
  "abi": 4,
  "kinds": [
    { "kind": "burst", "name": "Burst", "noun": "Bursts", "caps": ["log"],
      "hue": "amber", "lifetime": { "type": "instant" } }
  ],
  "decoder": {
    "descriptor": {
      "type_id": "burst", "name": "Burst", "summary": "Strong bursts",
      "family": "utility", "bandwidth_hz": 12000.0, "input_rate_hz": 48000.0,
      "emits": ["burst"],
      "settings": [
        { "key": "threshold_db", "label": "Threshold",
          "control": { "type": "number", "min": -60.0, "max": 0.0, "step": 1.0, "unit": "dB" } }
      ]
    },
    "feeds": [
      { "format": "burst-text", "name": "Burst text", "default_address": "127.0.0.1:30300",
        "kinds": ["burst"] }
    ]
  }
}
```

A plugin brings at least one of `decoder`, `encoder` and `processor`. `descriptor`, its
`settings` entries, `kinds` entries, `feeds` entries and `encoder` are the `ChannelDescriptor`,
`SettingField`, `EventKindEntry`, `FeedEntry` and `EncoderDescriptor` schemas in
[`openapi.json`](https://github.com/Newspicel/sdrmm/blob/main/openapi.json); `processor` is a
`ProcessorType`. A feed may add `heartbeat` bytes. The descriptor's `defaults` set where a new
channel starts; without them the host asks `normalize`. A descriptor with `trunking` is refused.

## One block

1. `sdrmm_input(handle, n)` returns a pointer; the host writes `n` samples there, each an `f32` I
   then an `f32` Q.
2. `sdrmm_process(handle, n, flags)` returns a pointer to 22 `u32` words, laid out in
   `OutputHeader` in `crates/wire/src/plugin/output.rs`. Floats travel as their bits:

| Word | Holds |
|---|---|
| 0, 1 | Audio pointer and length in `f32` samples, interleaved by `audio_channels` |
| 2 | Audio rate in Hz |
| 3, 4 | Events: pointer and byte length of a JSON array, `0` for none |
| 5, 6 | Status: pointer and byte length of a JSON object, `0` when unchanged |
| 7 | Frames heard but failing their checks, counted over the channel's life |
| 8 | Blocks dropped, counted over the channel's life |
| 9, 10 | Pictures: pointer and byte length of the records below, `0` for none |
| 11, 12 | Symbols: pointer and count of `f32`, I/Q pairs on the complex plane |
| 13, 14 | Reference points: pointer and count of `f32` |
| 15 | Plane: `0` no symbols, `1` complex, `2` levels |
| 16, 17 | Symbol rate, an `f64`, low word first |
| 18 to 21 | EVM, MER in dB, margin and frequency error in Hz, each an `f32` |

A picture record is five `u32` words (width, height, luma, RGB and image byte lengths), then the
luma and RGB bytes. A video frame stops there; an image adds JSON
`{"source": "...", "mode": "...", "complete": true, "lines": 240}`.

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

## Encoders and arrays

An encoder declares `encoder` in the manifest and adds the exports in
`crates/wire/src/plugin/encoder.rs`:

| Export | Signature |
|---|---|
| `sdrmm_encoder_open` | `(ptr, len) -> handle`, settings JSON in |
| `sdrmm_encoder_submit` | `(handle, kind: u32, ptr, len) -> i32`, kind `0` audio as `f32`, `1` frame bytes |
| `sdrmm_encoder_generate` | `(handle, max_samples: u32) -> i64`, packed pointer and sample count of I/Q |
| `sdrmm_encoder_close` | `(handle)` |

Its width and its message forms go through `sdrmm_call` as `width` and `compose`.

An array processor declares `processor` in the manifest and adds the exports in
`crates/wire/src/plugin/processor.rs`. `sdrmm_array_input` takes every lane's block one after the
other and `sdrmm_array_context` the block's `BlockContext`: time, gap, calibration, pose, steer
and lane centres. `sdrmm_array_process` returns the `ArrayHeader` laid out there: report, health,
events, steer, an encoded surface frame and up to two lanes of I/Q. A type with actions also
exports `sdrmm_array_action(handle, action) -> i32`.
