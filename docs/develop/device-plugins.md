---
title: Device plugins
description: Write a radio driver as a native plugin that loads beside the built-in ones.
---

A device plugin is a native library that brings one radio driver. Drop it into `--plugins-dir` (default `<data>/sdrmm/plugins`): `.so` on Linux, `.dylib` on macOS, `.dll` on Windows. Plugins load once at start. Restart to pick up a new build.

## Quick start

Copy `crates/device-plugin-example` (a radio that hears one tone), then:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
sdrmm-device-plugin = { git = "https://github.com/newspicel/sdrmm", rev = "<rev>" }
```

```rust
use sdrmm_device_plugin::{DeviceDriver, export_driver};

export_driver!(MyDriver::default());
```

```sh
cargo build --release
cp target/release/libmy_radio.so ~/.local/share/sdrmm/plugins/
```

## The driver

A plugin implements the same `DeviceDriver`, `SdrDevice` and `TxStream` traits as every built-in driver. `sdrmm_device_plugin::device` is `sdrmm-device`, so capture helpers, converters and the USB stream work as they do in-tree.

- **`id()`** names the backend: 1 to 31 of `a-z`, `0-9`, `_` and `-`. It must not match a built-in or another plugin.
- **`spec()`** gives its label, reach (`Local`, `Network`), the address hint for network radios, the Soapy module it replaces and its USB identities. The browser, the doctor and USB permission checks read only this.
- A plugin wins a radio a built-in also sees.

## Boundary

- Control calls cross as the wire-type JSON the REST API speaks. Samples cross as a pointer and a length, with no copy.
- Panics are caught at every call and become device errors. A panic in a stream thread fails that stream.
- `tracing` logs from the plugin land in the server's log.
- Plugins are native code with the server's rights. Install only code you trust.

## ABI

The device plugin ABI is versioned on its own. A plugin only needs a rebuild when the version moves; other SDRmm releases load it as is. `sdrmm --doctor` lists every plugin with its backend, ABI and the SDRmm revision it was built from, or why it did not load.

## Check the rate

```sh
sdrmm device-bench <backend>:<key> --seconds 30
```

This streams the radio flat out and prints the rate it reached, the samples it lost and its gaps.
