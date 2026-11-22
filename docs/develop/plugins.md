---
title: Plugins
description: Write WebAssembly decoder, encoder, and array processor plugins in Rust with hot reload and native DSP kernels.
---

Drop a `.wasm` into `--plugins-dir` (default `<data>/sdrmm/plugins`). The folder is hot-watched: rebuilt plugins reload live, reopening active channels with their wires and settings intact. Compiled modules cache under the user cache dir. For C/Zig/Go/AssemblyScript, see [Any language ABI](plugin-abi.md). Radio drivers are [device plugins](device-plugins.md).

## Quick start (Rust)

```sh
cargo install cargo-generate
cargo generate --git https://github.com/newspicel/sdrmm --tag v2.4.3 crates/plugin-example \
  --name my-plugin --define sdrmm_tag=v2.4.3
cd my-plugin
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/my_plugin.wasm ~/Library/Application\ Support/sdrmm/plugins/
```

Or add `sdrmm-plugin = { git = "https://github.com/newspicel/sdrmm", tag = "v2.4.3" }` to a `cdylib` crate.

## SDK traits

`sdrmm-plugin` re-exports `sdrmm-decoder` so plugins match built-ins. Implement `Plugin` (`decoder()`, `encoder()`, `processor()`, `kinds()`) and call `export!` once:

- **Decoders:** implement `Channel` + `NativeChannel` and declare band, `FilterSpec`, rates, UI `form()`, `home()`, `kinds()`, `feeds()`, `tune()`, and `signatures()` via `Native`. Frontend renders forms and views straight from the manifest (no plugin UI code; trunking plugins not supported yet).
- **Encoders:** implement `Modulator` + `NativeModulator` + `NativeEncoder` (width, form, message composer).
- **Array processors:** implement `array::ArrayProcessor` + `array::Settings` and call `export_processor!(Plugin, Processor, Settings)` (bearing/steer/lane/surface/action ports supported; position/track refused).

## ABI, speed, and safety

- **ABI v4:** older builds are refused until recompiled against the current SDK.
- **Native kernels:** `sdrmm_plugin::kernels` runs host FIR, FFT, and fractional resamplers directly on guest memory (and in-process under `cargo test`).
- **Isolation:** per-call time budget + 256 MiB memory cap. Runaway plugins stop only their own channel. Plugins are crash-isolated, not security-sandboxed; install only trusted code.
