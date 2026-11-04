//! Hand-written plugins in WAT, for what the example plugin cannot get wrong.
#![allow(dead_code, reason = "each test binary uses part of it")]

pub mod array;

use std::path::PathBuf;

const REPLY: &str = r#"{"ok":{}}"#;

/// Array processor exports that open, take settings and report nothing.
pub const ARRAY_EXPORTS: &str = r#"(func (export "sdrmm_array_open") (param i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_array_apply") (param i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_array_retune") (param i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_array_reset") (param i32 i32))
  (func (export "sdrmm_array_input") (param i32 i32 i32) (result i32) i32.const 16384)
  (func (export "sdrmm_array_process") (param i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_array_close") (param i32))"#;

/// A plugin answering every control call with an empty object, written where tests may write.
pub fn wat_plugin(name: &str, abi: u32, manifest: &str, process: &str, extra: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.wat"));
    std::fs::write(&path, wat_text(abi, manifest, process, extra))
        .unwrap_or_else(|error| panic!("write module: {error}"));
    path
}

/// The text of a plugin whose `sdrmm_process` is `process` and whose `extra` holds any further
/// exports.
pub fn wat_text(abi: u32, manifest: &str, process: &str, extra: &str) -> String {
    let escaped = |text: &str| text.replace('"', "\\\"");
    format!(
        r#"(module
  (memory (export "memory") 2)
  (data (i32.const 1024) "{manifest_text}")
  (data (i32.const 4096) "{reply}")
  (global $next (mut i32) (i32.const 8192))
  (func (export "sdrmm_abi") (result i32) i32.const {abi})
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
    {process})
  (func (export "sdrmm_close") (param i32))
  {extra})"#,
        manifest_text = escaped(manifest),
        reply = escaped(REPLY),
        manifest_packed = (1024_i64 << 32) | manifest.len() as i64,
        reply_packed = (4096_i64 << 32) | REPLY.len() as i64,
    )
}
