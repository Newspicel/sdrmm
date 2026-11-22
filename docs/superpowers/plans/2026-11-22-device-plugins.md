# Device Plugins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Radio drivers can ship as native plugins that load beside the built-in drivers through one SDK.

**Architecture:** `DeviceBackend` becomes an open, `Copy` id. Each driver declares a `BackendSpec`, and the registry serves the backend table from those specs. `sdrmm-device-plugin` wraps any `DeviceDriver` in a versioned C table (guest) and unwraps it back into `Box<dyn DeviceDriver>` (host). Control data crosses as wire-type JSON; samples cross as a pointer and a length.

**Tech Stack:** Rust 2024, `libloading`, serde_json, `tracing-core`, React/TS web app.

**Spec:** `docs/superpowers/specs/2026-11-22-device-plugins-design.md`

## Global Constraints
- AGENTS.md: no `//` comments; no `unwrap`/`expect` outside tests and startup; files at most 3000 lines, functions at most 200; no em dash; short text.
- Hot path (sample push): no locks, no allocation, no async.
- Stored patch format unchanged: a backend is a plain JSON string.
- Backend id: 1 to 31 bytes of `[a-z0-9_-]`.
- `DEVICE_ABI_VERSION: u32 = 1`, symbol `sdrmm_device_plugin_v1`.
- `PLUGIN_PRIORITY = 27` (above `NATIVE_PRIORITY = 25`).
- Native libraries load from the existing plugins folder: `.dylib` on macOS, `.so` on Linux, `.dll` on Windows.

## Review Focus
- A stored patch naming a backend this build lacks: it loads, saves unchanged and shows a missing radio. Pinned in Task 1 and Task 2.
- A plugin panicking inside `probe`, `open`, `apply` or a stream thread: the server stays up, and the error reaches the doctor or the stream's fault. Pinned in Task 4.
- A plugin file that isn't a plugin (random `.so`, missing symbol, old ABI): it's refused with a doctor row, and other plugins still load. Pinned in Task 5.
- Two plugins, or a plugin and a built-in, claiming one backend id: the second is refused. Pinned in Task 5.
- An invalid backend string over REST (too long, bad chars): a 400, no panic, nothing interned. Pinned in Task 1.

---

### Task 1: Open backend id

**Files:** Modify `crates/wire/src/device/backend.rs`, call sites across `crates/`, `apps/`.

**Produces:**
- `pub struct DeviceBackend { len: u8, bytes: [u8; 31] }`: `Copy`, `Eq`, `Ord`, `Hash`.
- `pub const fn new(id: &str) -> Self`: panics at compile time on a bad id. Only for consts.
- `pub fn parse(text: &str) -> Option<Self>` validates.
- `pub fn as_str(&self) -> &str`.
- Consts for the built-ins: `VIRTUAL, RECORDING, RTLSDR, HACKRF, AIRSPY, AIRSPYHF, FUNCUBE, CR8, SDRPLAY, AARONIA, SOAPY, RTLTCP, SPYSERVER, SDRCONNECT, KIWISDR, AD936X, ESPSDR`.
- Serde as a string; `ToSchema` as `type: string`.
- `BackendReach` stays.
- New wire types: `UsbIdentity { vendor: u16, product: Option<u16>, name: String }` and `BackendSpec { backend, label, reach, address_hint: Option<String>, soapy_module: Option<String>, usb: Vec<UsbIdentity> }`. `BackendEntry` is replaced by `BackendSpec`.
- The `backends!` macro and `backend_table()` are removed. `is_radio`/`node_owned`/`addressable` move to `BackendSpec`.

- [ ] Tests: `an_id_round_trips_as_a_plain_string`, `an_unknown_id_still_parses`, `a_bad_id_is_refused` (empty, 32 bytes, `"Rtl"`, `"a b"`), `a_patch_with_an_unknown_backend_round_trips` (in `crates/wire/src/patch/tests.rs`).
- [ ] Implement, then rename `DeviceBackend::Variant` to the consts with sed, and fix every `.reach()`, `.label()` and `soapy_module()` caller to go through specs (Task 2 supplies them; keep it compiling with a temporary `builtin_specs()` in `sdrmm-device` if needed).
- [ ] `cargo check --workspace --all-targets`, then `cargo test -p sdrmm-wire`.

### Task 2: Drivers declare specs, the registry serves them

**Files:** `crates/device/src/{lib.rs,registry.rs,usb.rs,usb/nodes.rs}`, every `crates/device-*/src/lib.rs`, `crates/engine/src/lib.rs`, `crates/server/src/{doctor.rs,rest/devices.rs,store/steps/backends.rs}`, `crates/wire/src/rest.rs`.

**Produces:**
- `DeviceDriver::spec(&self) -> BackendSpec` (required); `fn id(&self) -> DeviceBackend { self.spec().backend }` (provided).
- `DeviceRegistry::specs(&self) -> Vec<BackendSpec>`; `spec(&self, DeviceBackend) -> Option<BackendSpec>`; `usb_identities(&self) -> Vec<UsbIdentity>`.
- `usb::radio_nodes(identities: &[UsbIdentity]) -> Vec<RadioNode>` (the `RADIOS` table moves into the driver specs; bladeRF, Lime and USRP into Soapy's).
- `engine::registry(recordings_dir, playback_speed, extra: Vec<(u8, Box<dyn DeviceDriver>)>) -> DeviceRegistry`: Soapy is built last, excluding every `soapy_module` the other specs name.
- `DevicesResponse.backends: Vec<BackendSpec>`.

- [ ] Tests: `the_registry_serves_every_registered_spec`, `usb_names_come_from_the_winning_driver`, `soapy_hides_modules_a_plugin_claims`, plus the existing discovery tests updated.
- [ ] Implement.
- [ ] Run `cargo test -p sdrmm-device -p sdrmm-engine --lib` and `cargo test -p sdrmm-server doctor`.

### Task 3: Forwarding sink

**Files:** `crates/device/src/lib.rs`.

**Produces:** `pub trait SinkForward: Send { fn samples(&mut self, samples: &[Sample], index: u64); fn event(&mut self, event: LaneEvent); fn dropped(&mut self, samples: u64); }` and `RxSink::forwarding(forward: impl SinkForward + 'static, fatal_fn) -> RxSink`. `RxSink::dropped` tells the forward, so a mirrored sink keeps the same index.

- [ ] Test `a_forwarded_sink_mirrors_every_index_move`: push 4, dropped 10, deaf 5, dropped_estimate 3. The mirror ends at the same `index()` and sees the same events.
- [ ] Implement; `cargo test -p sdrmm-device`.

### Task 4: `sdrmm-device-plugin` crate

**Files:** Create `crates/device-plugin/{Cargo.toml,src/lib.rs,src/abi.rs,src/guest.rs,src/guest/{sink.rs,device.rs,log.rs},src/host.rs,src/host/{sink.rs,device.rs,load.rs},src/tests.rs}`.

**Produces:**
- `abi.rs`: `#[repr(C)]` `PluginV1` (abi, size, build rev, driver handle, fn pointers for every `DeviceDriver`/`SdrDevice`/`TxStream` method), `HostV1` (sink and sweep callbacks, log), `Bytes { ptr, len }` owned by the guest and freed with `free_bytes`, `ErrorCode` mirroring `DeviceError`.
- `guest::export_driver!($ctor)`: defines `#[unsafe(no_mangle)] extern "C" fn sdrmm_device_plugin_v1(host: *const HostV1) -> *const PluginV1` and installs a `tracing` subscriber that forwards to `HostV1.log`.
- `host::load(path: &Path) -> Result<Loaded, LoadError>`, where `Loaded { driver: Box<dyn DeviceDriver>, backend, build }`. The library is kept for the process (`std::mem::forget`).
- `host::from_table(table) -> Box<dyn DeviceDriver>` for in-process tests.
- Every guest entry runs under `catch_unwind`. A panic returns `ErrorCode::Io` with "plugin panicked: …".
- `DEVICE_ABI_VERSION`.

- [ ] Tests (in-process, `tests.rs` with a fake driver built through the guest macro internals): `probe_open_apply_round_trip`, `samples_and_events_cross_unchanged`, `tx_writes_cross`, `sweep_blocks_cross`, `a_panic_in_open_is_an_io_error`, `a_panic_in_a_stream_thread_fails_the_stream`, `logs_reach_the_host`.
- [ ] Implement; `cargo test -p sdrmm-device-plugin`.

### Task 5: Example plugin and loading

**Files:** Create `crates/device-plugin-example/` (cdylib tone device, id `tone`). Modify `crates/engine/src/lib.rs` (`load_device_plugins(dir) -> (Vec<(u8, Box<dyn DeviceDriver>)>, Vec<PluginReport>)`), `apps/sdrmm/src/main.rs`, `apps/desktop/src/main.rs`, `crates/server/src/doctor.rs`, `xtask/src/architecture.rs`.

- [ ] Tests in `crates/device-plugin/tests/load.rs`: build the example with `cargo build -p sdrmm-device-plugin-example --target-dir $CARGO_TARGET_TMPDIR/plugin`; `the_example_loads_and_streams`, `a_library_without_the_symbol_is_refused`, `an_old_abi_is_refused`, `a_duplicate_backend_is_refused`.
- [ ] Implement, with doctor rows: file, backend, ABI, build, loaded or why not.
- [ ] Run the tests.

### Task 6: Web reads backends at runtime

**Files:** `web/src/components/devices.ts` and its callers, `xtask/gen/src/main.rs` (drop `backends.json`), `web/src/generated/*` regenerated.

- [ ] `devices.ts` takes its table from `DevicesResponse.backends`; update `OpenRadio.test.ts`.
- [ ] `cargo xtask gen` (or the repo's codegen command), `npm test`, `npm run lint`, `tsc`.

### Task 7: `sdrmm device-bench`

**Files:** `apps/sdrmm/src/main.rs` (subcommand), `crates/engine/src/bench.rs`.

**Produces:** `sdrmm device-bench <device-id> [--seconds N]`. It opens the device through the registry (plugins included), runs it through `Capture` into a counting sink, and prints rate, samples, dropped and gaps.

- [ ] Test `the_bench_counts_a_virtual_device`.
- [ ] Implement; run it on `virtual:…`.

### Task 8: Contract guard, docs, changeset

- [ ] Test `device_abi_types_are_pinned` in `crates/device-plugin/src/tests.rs`: hash of the utoipa schema JSON for every crossing wire type equals a constant, with a failure message saying to bump `DEVICE_ABI_VERSION`.
- [ ] Docs page `docs/src/development/device-plugins.md` plus a link next to the WASM plugin docs.
- [ ] `cargo xtask changeset minor "Radio drivers can be plugins"`.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets`, the tests of the changed crates.
