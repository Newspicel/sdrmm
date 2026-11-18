# Plugins

A plugin brings a decoder, an encoder or an array processor, or any mix of them, compiled to
WebAssembly. Drop the `.wasm` into the plugins folder (`--plugins-dir`, default `sdrmm/plugins` in
the platform data folder) and each part appears next to the built-in ones: same palette entry,
same settings form, same logs, maps and wires. The folder is
watched, so a rebuilt plugin is loaded again on its own, and its running channels reopen on the new
build with their settings and wires. A channel the new build refuses keeps the old one and the
reason shows as an error. A plugin whose type id a built-in or another plugin already has is
refused with an error; the others still load.

Compiled plugins are cached in `sdrmm/plugins` in the user cache folder, so a start only compiles
what changed.

## Write one

This section is for Rust. For C, Zig, Go, AssemblyScript and others, see
[Plugins in any language](plugin-abi.md).

Start a new plugin from `crates/plugin-example`, a carrier detector. Pick the tag of the server
you run:

```sh
cargo install cargo-generate
cargo generate --git https://github.com/newspicel/sdrmm --tag v2.4.3 crates/plugin-example \
  --name my-plugin --define sdrmm_tag=v2.4.3
cd my-plugin
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/my_plugin.wasm ~/Library/Application\ Support/sdrmm/plugins/
```

An existing crate adds the SDK as a git dependency:

```toml
sdrmm-plugin = { git = "https://github.com/newspicel/sdrmm", tag = "v2.4.3" }
```

A plugin crate depends on `sdrmm-plugin` and builds as a `cdylib`. The SDK re-exports
`sdrmm-decoder`, the API the built-in decoders and encoders use, so a plugin's parts are written
exactly like built-in ones. It implements `Plugin` and calls `export!` once:

| `Plugin` item | Gives |
|---|---|
| `decoder()` | The decoder, built with `Native` |
| `encoder()` | The encoder, built with `NativeEncoder` |
| `processor()` | The array processor's type |
| `kinds()` | Event kinds the decoder does not declare, such as the processor's |

The manifest, the presenters and every control call follow from these.

## Decoders

A channel implements `Channel` and opens through `NativeChannel`. `Native` declares the rest:

| `Native` item | Gives |
|---|---|
| `new::<C>(band)` | The channel type and the band the settings occupy |
| `filter()` | The `FilterSpec` between the downconverter and the channel: lowpass, band or none |
| `input_rate()`, `audio_channels()` | Rates and channels that follow the settings |
| `form()`, `home()` | The settings form and the frequency a new channel starts on |
| `kinds()`, `feeds()` | Event kinds with their presenters, and feed formats |
| `tune()`, `signatures()` | How Signal ID names the signal and shapes settings to it |

`Channel` takes full channel settings on `apply()`, and hears `retuned()`, `position_changed()`,
`captured_at()` and `lo_artifact_at()`. `process()` fills `ChannelOutputs`: audio, events,
status, video frames, images and symbols. A trunking decoder cannot be a plugin yet; a manifest
that declares trunking is refused.

The settings form is what the channel node shows: choices, numbers with ranges and units,
toggles, text, conditional fields and lists. The frontend draws it from the manifest, so a plugin
ships no UI code. Event kinds carry view hints (table, meter, trend, picture) and a map symbol; a
status carries the health footer (lock, SNR, frequency error, errors per second). A family or
symbol the server does not know gets its own palette group or a dot on the map.

## Encoders

A modulator implements `Modulator` and opens through `NativeModulator`. `NativeEncoder` declares
its width as a function of its settings, its form and, for a message encoder, the message form
and how a filled one becomes a payload. It shows under **Encoders** like a built-in one. The
example's `Beacon` keys a text field as two-tone FSK at a set baud.

## Array processors

An array processor runs on every lane of an Array node at once, like the direction finder.
It is written like a built-in one: implement `array::ArrayProcessor` on settings that implement
`array::Settings`, return its `ProcessorType` from `Plugin::processor()` and call
`export_processor!(Plugin, Processor, Settings)` once. A block carries geometry, calibration,
pose, time and gaps, as for a built-in. Its type may declare bearing, steer and lane ports, a
range-Doppler, spatial spectrum or visibility surface, actions and a band; position and track
ports are refused. The example's `LaneMeter` reports each lane's power and phase and writes their
sum to a `sum` lane.

## ABI

The server runs plugins built for its own ABI version, currently 4, and refuses any other. A
plugin built for an older ABI needs a rebuild against the current SDK.

## Speed

`sdrmm_plugin::dsp`, `modem`, `codec` and `crypto` are the building blocks the built-in decoders use.
The heavy kernels in `sdrmm_plugin::kernels` (FIR, FFT, resampler) run natively in the server on
the plugin's own memory. Under `cargo test` they run in-process, so a plugin tests like any crate.

## Safety

Each call has a time budget and each instance at most 256 MiB of memory. A plugin stuck in a loop
stops its own channel, which then says so on its node, and never holds the receiver. One that
grows past the cap fails its own allocation. A plugin is code you chose to install: it is
isolated from crashing the server, not vetted.
