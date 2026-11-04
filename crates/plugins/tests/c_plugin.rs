//! Builds the C plugin in `crates/plugin-example/c` with `zig cc` and runs it the way the server
//! does, so the guide's non-Rust example stays true to the ABI.

use std::{path::Path, process::Command};

use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs, Decoder};
use sdrmm_plugins::{PluginModule, WasmDecoder};
use sdrmm_wire::EventKind;

fn build() -> WasmDecoder {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = workspace.join("crates/plugin-example/c/burst.c");
    let wasm = Path::new(env!("CARGO_TARGET_TMPDIR")).join("burst.wasm");
    let status = Command::new("zig")
        .args([
            "cc",
            "-target",
            "wasm32-freestanding",
            "-O2",
            "-nostdlib",
            "-Wl,--no-entry",
            "-Wl,--export-dynamic",
            "-o",
        ])
        .arg(&wasm)
        .arg(&source)
        .status()
        .unwrap_or_else(|error| panic!("zig did not start: {error}"));
    assert!(status.success(), "the C plugin did not build");
    let module = PluginModule::load(&wasm).unwrap_or_else(|error| panic!("plugin loads: {error}"));
    WasmDecoder::new(module)
}

#[test]
#[ignore = "needs zig on PATH"]
fn a_c_plugin_logs_a_burst_once_and_reports_lock() {
    let decoder = build();
    assert_eq!(decoder.descriptor().type_id, "burst");
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let mut channel = decoder
        .create_rx(
            ChannelCtx {
                input_rate: 48_000.0,
            },
            &settings,
        )
        .unwrap_or_else(|error| panic!("channel opens: {error}"));
    let mut out = ChannelOutputs::default();
    channel.process(&[Complex::new(0.0, 0.0); 512], &mut out);
    assert!(out.events.is_empty());
    for _ in 0..3 {
        channel.process(&[Complex::new(0.5, 0.5); 512], &mut out);
    }
    assert_eq!(out.events.len(), 1, "{:?}", out.events);
    assert_eq!(out.events[0].kind, EventKind::named("burst"));
    assert_eq!(out.status.expect("a status").health.lock, Some(true));
    let view = decoder.kinds()[0]
        .codec
        .present(&out.events[0].kind, &out.events[0].data())
        .expect("presents");
    assert_eq!(view.summary, "Burst");
}
