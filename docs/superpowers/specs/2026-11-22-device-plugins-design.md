# Device plugins

## Why
Anyone can add a radio driver without it living in this repo. A driver is a native library dropped into the plugins folder and loads beside the built-in drivers. Built-in and plugin drivers write against one API.

## Goals
- One device SDK. Built-in drivers and plugins implement the same `DeviceDriver`, `SdrDevice` and `TxStream` traits from `sdrmm-device`. Plugins don't get a second, thinner API.
- A plugin is a native `cdylib`, loaded at runtime from the plugins folder by official release builds.
- Full rate. Samples cross the boundary as a pointer and a length, with no copy and no serialization, so drivers in the GB/s range work.
- A plugin brings everything about its radio: backend id, label, reach, address hint, Soapy module to hide, and USB vendor/product names. The host names no plugin radio anywhere.
- Load faults show up in the doctor: ABI mismatch, missing symbol, duplicate backend id, a panic at load.
- An example device plugin in the repo, used as a template and as the test fixture.

## Non-goals
- WASM device plugins. The sandbox can't keep up with wideband radios.
- Hot reload or unload. A loaded library stays loaded until the process exits.
- Sandboxing native code. A device plugin runs with the server's rights, like any driver.
- Phones.

## Anti-goals
- A plugin crash in normal operation that takes the server down silently. Every boundary call catches panics and turns them into `DeviceError`.
- A plugin built for an older ABI that loads and corrupts memory. Version checks refuse it with a clear doctor entry.
- Per-plugin special cases in engine, server, wire or web.

## Constraints
- AGENTS.md: one source of truth for wire types; hot DSP path has no locks, no allocation, no async; no `unwrap` outside tests and startup; files at most 3000 lines, functions at most 200.
- The existing traits are the contract. Changing them changes the plugin ABI version.
- macOS, Linux, Windows.

## Approaches
1. **Native library behind a stable C table (picked).** A versioned `#[repr(C)]` function table wraps the traits. Runs at full speed, works with release builds, and survives Rust toolchain bumps.
2. Separate process with shared memory for samples. It isolates crashes, but brings cross-platform shared memory and process lifecycle. More moving parts than the use needs.
3. Rust-ABI dylib passing `dyn DeviceDriver`. The cheapest, but host and plugin must use the same compiler and the same sdr-- version, or it crashes.

WASM was ruled out on rate.

## Decided

### Open backend ids
- `DeviceBackend` stops being a closed enum. It becomes a `Copy` interned id that serializes as its string, as today. Stored patches keep their format.
- Per-backend facts move out of the `backends!` macro into a `BackendSpec` each driver declares:
  - id, label, reach, address hint, Soapy module
  - USB identities (vendor, optional product, name)
- `DeviceDriver` gains `fn spec(&self) -> BackendSpec`. `id()` derives from it.
- The registry serves the backend table and the USB name lookup from the registered drivers, replacing `backend_table()`, `RADIOS` in `usb/nodes.rs` and `native_backends()`.
- Host code that has to name a backend (Virtual, Recording) uses constants. Everything else reads the spec.
- The web app reads the backend table from REST at runtime, not from `web/src/generated/backends.json`.
- Several specs may claim one USB identity. The name lookup returns the name from the winning driver.
- An id stored in a patch whose driver isn't loaded stays valid and shows as a missing radio, the way an unplugged one does.

### Plugin crate `sdrmm-device-plugin`
- Guest side: `export_driver!(MyDriver)` turns a `DeviceDriver` into the C table, behind one exported symbol `sdrmm_device_plugin_v1`. The table carries its ABI version, its size and the sdr-- version it was built against.
- Host side: a loader that opens a library with `libloading`, checks the version and wraps the table back into `Box<dyn DeviceDriver>`. The registry can't tell it apart from a built-in.
- Control plane over the boundary: wire types as JSON (`DeviceInfo`, `Capabilities`, `DeviceSettings`, `Readout`, `AgcGain`, `DoctorCheck`, `SweepPlan`, `BackendSpec`). Errors are a `#[repr(C)]` kind code plus a message. Strings and buffers the guest returns are freed by a guest function.
- Data plane: `RxSink` crosses as a host handle plus `#[repr(C)]` callbacks for push, dropped, deaf, dropped_estimate, realigned, mark, stamp_hardware, fail and room. The guest calls them from its own threads. Push takes `*const Complex<f32>` and a length; nothing allocates.
- TX: `TxStream` as a guest handle with write_channels (a pointer array) and stop.
- Sweep: `SweepSink` crosses like `RxSink`.
- Every exported function runs under `catch_unwind`. A panic becomes `DeviceError::Io`, or fails the stream.
- Plugins spawn their own threads and use any crate they want (`nusb`, `sdrmm-usb-stream`, `sdrmm-dsp`).
- `playback()` doesn't cross. Playback is a host-only path.

### Loading
- Native libraries (`.dylib`, `.so`, `.dll`) sit in the same plugins folder as the WASM plugins. Loaded once at start, before the first probe.
- Plugins register at `PLUGIN_PRIORITY`, above `NATIVE_PRIORITY`. A driver the operator installed wins a serial that a built-in also sees. *(Builder call.)*
- One shared loader function builds the registry for `apps/sdrmm`, `apps/desktop` and `--doctor`.
- A plugin whose backend id matches a built-in, or another plugin, is refused and reported.
- The doctor lists each device plugin: file, backend, ABI version, the sdr-- git rev it was built from, loaded or why not.

### Versions *(builder call)*
- Only the C table's integer ABI version is strict. A mismatch refuses the load.
- The ABI version covers the table and every wire type that crosses it as JSON. A test pins a hash of those types' JSON schemas, so changing one fails CI until the ABI version is bumped.
- The build rev is informational. A plugin from another rev with the same ABI loads, so a plugin only needs a rebuild when the ABI moves.

### Built-ins
- Built-in driver crates keep implementing the same traits and declare a `BackendSpec`. Any of them could be built as a plugin by adding `export_driver!`.

### Docs and tests
- `crates/device-plugin-example`: a small synthetic-tone device as a `cdylib`, the template, built and loaded by the loader tests.
- Tests: ABI round trip of every trait method; sample push without allocation; panic at each boundary; version mismatch refused; duplicate id refused; an unknown stored backend id survives a load and a save.
- `sdrmm device-bench <device-id> [--seconds N]`: runs any device, built-in or plugin, through the real capture path and reports rate and dropped samples. It doubles as the throughput check for plugins kept outside this repo. *(Builder call.)*
- Docs page: "Write a device plugin".
- Changeset (minor): device drivers can be plugins.

## Left to the builder
- Interning scheme for `DeviceBackend` (leaked `&'static str` table or similar), as long as it stays `Copy` and serde stays a plain string.
- The exact C table layout and how JSON buffers are owned.
- Crate split: guest and host halves in one crate behind features, or two crates.
- Whether `id()` stays on the trait as a provided method.
