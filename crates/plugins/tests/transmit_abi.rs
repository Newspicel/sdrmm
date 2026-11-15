//! A plugin's encoder is checked when it loads, and a plugin built for another ABI is refused.

use sdrmm_channels::{
    ChannelError,
    encoder::{Encoder, EncoderParams, TxPayload},
};
use sdrmm_plugins::{PluginModule, WasmEncoder};

mod common;

const DECODER: &str = r#""descriptor":{"type_id":"probe","name":"Probe","bandwidth_hz":1000.0,"input_rate_hz":48000.0}"#;

fn manifest(abi: u32, encoder: Option<&str>) -> String {
    let encoder = encoder.map_or_else(String::new, |encoder| format!(r#","encoder":{encoder}"#));
    format!(r#"{{"abi":{abi},{DECODER}{encoder}}}"#)
}

const AUDIO_ENCODER: &str =
    r#"{"type_id":"probe_tx","name":"Probe","rate_hz":48000.0,"bandwidth_hz":1000.0}"#;

const MESSAGE_ENCODER: &str = r#"{"type_id":"probe_tx","name":"Probe","rate_hz":48000.0,"bandwidth_hz":1000.0,"input":"message","message":[{"key":"text","label":"Text","control":{"type":"text","placeholder":"hello"}}]}"#;

const TX_EXPORTS: &str = r#"(func (export "sdrmm_tx_open") (param f64 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_tx_submit") (param i32 i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_tx_generate") (param i32 i32) (result i64) i64.const 0)
  (func (export "sdrmm_tx_close") (param i32))"#;

fn load(
    name: &str,
    abi: u32,
    encoder: Option<&str>,
    extra: &str,
) -> Result<std::sync::Arc<PluginModule>, String> {
    let path = common::wat_plugin(name, abi, &manifest(abi, encoder), "i32.const 0", extra);
    PluginModule::load(&path).map_err(|error| error.to_string())
}

fn encoder(name: &str, declared: &str) -> WasmEncoder {
    let module =
        load(name, 1, Some(declared), TX_EXPORTS).unwrap_or_else(|error| panic!("{error}"));
    WasmEncoder::of(&module).unwrap_or_else(|| panic!("the manifest declares an encoder"))
}

fn refusal<T>(result: Result<T, String>) -> String {
    match result {
        Ok(_) => panic!("the plugin must be refused"),
        Err(error) => error,
    }
}

#[test]
fn an_encoder_missing_its_exports_is_refused_by_name() {
    let error = refusal(load("no-tx", 1, Some(AUDIO_ENCODER), ""));
    assert!(error.contains("sdrmm_tx_generate"), "{error}");
}

#[test]
fn any_other_abi_is_refused() {
    for abi in [0, 2, 99] {
        let error = refusal(load(&format!("abi-{abi}"), abi, None, ""));
        assert!(error.contains(&format!("ABI {abi}")), "{error}");
    }
}

#[test]
fn a_message_encoder_is_handed_the_filled_form_as_json() {
    let audio = encoder("tx-audio", AUDIO_ENCODER);
    assert!(matches!(
        audio.compose(&serde_json::json!({"text": "hi"})),
        Err(ChannelError::InvalidPayload(_))
    ));

    let messages = encoder("tx-message", MESSAGE_ENCODER);
    assert_eq!(messages.descriptor().message.len(), 1);
    let Ok(TxPayload::Frame(frame)) = messages.compose(&serde_json::json!({"text": "hi"})) else {
        panic!("a filled form becomes a frame");
    };
    assert_eq!(frame, br#"{"text":"hi"}"#);
}

#[test]
fn a_plugin_without_an_encoder_brings_none() {
    let module = load("rx-only", 1, None, TX_EXPORTS).unwrap_or_else(|error| panic!("{error}"));
    assert!(WasmEncoder::of(&module).is_none());
}

#[test]
fn an_encoder_opens_and_generates_through_the_abi() {
    let encoder = encoder("tx", AUDIO_ENCODER);
    assert!(matches!(
        encoder.open(&EncoderParams::bare("probe")),
        Err(ChannelError::InvalidSettings(_))
    ));
    let mut tx = encoder
        .open(&EncoderParams::bare("probe_tx"))
        .unwrap_or_else(|error| panic!("opens: {error}"));
    let mut out = [num_complex::Complex::new(1.0f32, 1.0); 16];
    assert_eq!(tx.generate(&mut out), 0);
}
