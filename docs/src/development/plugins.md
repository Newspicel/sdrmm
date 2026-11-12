# Decoder plugins

A plugin is a decoder compiled to WebAssembly. Drop the `.wasm` into the plugins folder
(`--plugins-dir`, default `sdrmm/plugins` in the platform data folder) and it appears next to the
built-in decoders: same palette entry, same settings form, same logs, maps and wires. The folder is
watched, so a rebuilt plugin is loaded again on its own, and its running channels reopen on the new
build with their settings and wires. A channel the new build refuses keeps the old one and the
reason shows as an error.

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
ships no UI code. Event kinds carry view hints (table, meter, trend, picture) and a map symbol; a
status carries the health footer (lock, SNR, frequency error, errors per second). A family or
symbol the server does not know gets its own palette group or a dot on the map.

## Transmit

Set `can_transmit` on the descriptor, implement `Transmitter` and call `export_transmit!` once.
`submit()` takes audio or a frame, `generate()` fills baseband at the input rate. The example's
`Beacon` sends frames as two-tone FSK.

A Modulator node feeds it. Set `transmits_audio` to take an audio wire. List fields in the
descriptor's `message` to get a form with a Send button: each filled form arrives as UTF-8 JSON
in a frame. The example's `Beacon` declares one text field and keys what is typed into it.

## Array processors

A plugin can also bring an array processor: it runs on every lane of an Array node at once, like
the direction finder. Declare it as the manifest's `processor` (type id, name, lane range, needs,
settings form, readouts, emitted kinds), implement `Processor` and call `export_processor!` once.
`process()` gets one block of every lane and sets readouts by their declared order, the health
footer and events. Its node has the same form and readouts as any other, without a plot. The
example's `LaneMeter` reports each lane's power and phase against the first.

## ABI

The server runs plugins built for its own ABI version and refuses any other.

## Speed

`sdrmm_plugin::dsp` and `sdrmm_plugin::modem` are the building blocks the built-in decoders use.
The heavy kernels in `sdrmm_plugin::kernels` (FIR, FFT, resampler) run natively in the server on
the plugin's own memory. Under `cargo test` they run in-process, so a plugin tests like any crate.

## Safety

Each call has a time budget. A plugin stuck in a loop stops its own channel, which then says so on
its node, and never holds the receiver. A plugin is code you chose to install: it is isolated from
crashing the server, not vetted.
