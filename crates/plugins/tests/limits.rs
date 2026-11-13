//! A plugin cannot claim more memory than its cap, nor make the host allocate a reply it never
//! wrote.

use sdrmm_plugins::PluginModule;

mod common;

const MANIFEST: &str = r#"{"abi":1,"descriptor":{"type_id":"capped","name":"Capped","bandwidth_hz":1000.0,"input_rate_hz":48000.0}}"#;
const MEMORY: &str = r#"(memory (export "memory") 2)"#;

fn text() -> String {
    common::wat_text(1, MANIFEST, "i32.const 0", "")
}

fn load(text: &str) -> Result<(), String> {
    PluginModule::from_bytes("capped".to_owned(), text.as_bytes(), None)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[test]
fn a_plugin_within_its_cap_loads() {
    load(&text()).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn a_memory_past_the_cap_is_refused() {
    let huge = text().replace(MEMORY, r#"(memory (export "memory") 8192)"#);
    assert!(load(&huge).is_err(), "512 MiB of memory loaded");
}

#[test]
fn a_reply_past_guest_memory_is_refused_before_it_is_allocated() {
    let packed = (1024_i64 << 32) | MANIFEST.len() as i64;
    let lying = text().replace(
        &format!("i64.const {packed}"),
        &format!("i64.const {}", (1024_i64 << 32) | 0xFFFF_FFFF),
    );
    let error = load(&lying).expect_err("a 4 GiB manifest loaded");
    assert!(error.contains("outside guest memory"), "{error}");
}
