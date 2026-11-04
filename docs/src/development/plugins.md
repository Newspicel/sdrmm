# Decoder plugins

A plugin is a decoder compiled to WebAssembly. Drop the `.wasm` into the plugins folder
(`--plugins-dir`, default `sdrmm/plugins` in the platform data folder) and it appears next to the
built-in decoders: same palette entry, same settings form, same logs, maps and wires. The folder is
watched, so a rebuilt plugin is loaded again on its own.

## Write one

Start from `crates/plugin-example`, a carrier detector:

```sh
cargo build -p sdrmm-plugin-example --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/sdrmm_plugin_example.wasm ~/Library/Application\ Support/sdrmm/plugins/
```

A plugin crate depends on `sdrmm-plugin`, builds as a `cdylib`, implements `Plugin` and calls
`export!` once:

| `Plugin` item | Gives |
|---|---|
| `Settings` | Your settings struct, read from JSON with defaults for absent fields |
| `manifest()` | Type id, name, family, input rate, settings form and event kinds |
| `layout()` | Input rate and the band the settings occupy |
| `open()`, `apply()` | A running channel, and new settings for it |
| `process()` | One block of IQ in; audio, events and node readouts out |
| `present()` | How one of your events reads in logs and on maps |

The settings form is what the channel node shows: choices, numbers with ranges and units,
toggles, text, conditional fields and lists. The frontend draws it from the manifest, so a plugin
ships no UI code.

## Speed

`sdrmm_plugin::dsp` and `sdrmm_plugin::modem` are the building blocks the built-in decoders use.
The heavy kernels in `sdrmm_plugin::kernels` (FIR, FFT, resampler) run natively in the server on
the plugin's own memory. Under `cargo test` they run in-process, so a plugin tests like any crate.

## Safety

Each call has a time budget. A plugin stuck in a loop stops its own channel, which then says so on
its node, and never holds the receiver. A plugin is code you chose to install: it is isolated from
crashing the server, not vetted.
