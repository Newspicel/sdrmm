//! A plugin that never returns stops its own channel and leaves the caller free.

use std::{path::PathBuf, time::Instant};

use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs, Decoder};
use sdrmm_plugins::{PluginModule, WasmDecoder};
use sdrmm_wire::present::FieldValue;

const MANIFEST: &str = r#"{"abi":1,"descriptor":{"type_id":"runaway","name":"Runaway","bandwidth_hz":1000.0,"input_rate_hz":48000.0}}"#;
const REPLY: &str = r#"{"ok":{}}"#;

fn module_text() -> String {
    let escaped = |text: &str| text.replace('"', "\\\"");
    format!(
        r#"(module
  (memory (export "memory") 2)
  (data (i32.const 1024) "{manifest}")
  (data (i32.const 4096) "{reply}")
  (global $next (mut i32) (i32.const 8192))
  (func (export "sdrmm_abi") (result i32) i32.const 1)
  (func (export "sdrmm_alloc") (param $len i32) (result i32)
    (local $at i32)
    global.get $next
    local.set $at
    global.get $next
    local.get $len
    i32.add
    global.set $next
    local.get $at)
  (func (export "sdrmm_manifest") (result i64)
    i64.const {manifest_packed})
  (func (export "sdrmm_call") (param i32 i32 i32) (result i64)
    i64.const {reply_packed})
  (func (export "sdrmm_error") (result i64) i64.const 0)
  (func (export "sdrmm_open") (param f64 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_apply") (param i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_input") (param i32 i32) (result i32) i32.const 16384)
  (func (export "sdrmm_process") (param i32 i32) (result i32)
    (loop $forever (br $forever))
    i32.const 0)
  (func (export "sdrmm_close") (param i32)))"#,
        manifest = escaped(MANIFEST),
        reply = escaped(REPLY),
        manifest_packed = (1024_i64 << 32) | MANIFEST.len() as i64,
        reply_packed = (4096_i64 << 32) | REPLY.len() as i64,
    )
}

fn plugin_path() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join("runaway.wat");
    std::fs::write(&path, module_text()).unwrap_or_else(|error| panic!("write module: {error}"));
    path
}

#[test]
fn a_plugin_stuck_in_a_loop_stops_its_channel_with_a_visible_error() {
    let module = PluginModule::load(&plugin_path()).unwrap_or_else(|error| panic!("{error}"));
    let decoder = WasmDecoder::new(module);
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let mut channel = decoder
        .create_rx(
            ChannelCtx {
                input_rate: 48_000.0,
            },
            &settings,
        )
        .unwrap_or_else(|error| panic!("opens: {error}"));
    let iq = vec![Complex::new(0.0f32, 0.0); 256];
    let mut out = ChannelOutputs::default();
    let started = Instant::now();
    channel.process(&iq, &mut out);
    assert!(
        started.elapsed().as_secs_f64() < 5.0,
        "{:?}",
        started.elapsed()
    );
    let status = out.status.take().expect("the stop shows on the node");
    let FieldValue::Text(text) = &status.readouts[0].value else {
        panic!("a text readout");
    };
    assert!(text.starts_with("stopped"), "{text}");
    let started = Instant::now();
    channel.process(&iq, &mut out);
    assert!(
        started.elapsed().as_millis() < 50,
        "a stopped channel returns at once"
    );
    assert_eq!(channel.skipped(), 1);
}
