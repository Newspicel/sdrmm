//! What a plugin's manifest declares is checked when it loads: each part needs its exports, a
//! plugin needs at least one part, and a plugin built for another ABI is refused.

use sdrmm_channels::encoder::{Encoder, EncoderError, EncoderParams, TxPayload};
use sdrmm_plugins::{PluginModule, WasmDecoder, WasmEncoder};

mod common;

const DECODER: &str = r#"{"descriptor":{"type_id":"probe","name":"Probe","bandwidth_hz":1000.0,"input_rate_hz":48000.0}}"#;

const AUDIO_ENCODER: &str =
    r#"{"type_id":"probe_tx","name":"Probe","rate_hz":48000.0,"bandwidth_hz":1000.0}"#;

const MESSAGE_ENCODER: &str = r#"{"type_id":"probe_tx","name":"Probe","rate_hz":48000.0,"bandwidth_hz":1000.0,"input":"message","message":[{"key":"text","label":"Text","control":{"type":"text","placeholder":"hello"}}]}"#;

const KINDS: &str = r#"[{"kind":"probe","name":"Probe","noun":"Probes","caps":["log"],"hue":"teal","lifetime":{"type":"instant"}}]"#;

const ENCODER_EXPORTS: &str = r#"(func (export "sdrmm_encoder_open") (param f64 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_encoder_submit") (param i32 i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_encoder_generate") (param i32 i32) (result i64) i64.const 0)
  (func (export "sdrmm_encoder_close") (param i32))"#;

/// A manifest declaring the parts given, each as its JSON.
fn manifest(abi: u32, parts: &[(&str, &str)]) -> String {
    let parts: String = parts
        .iter()
        .map(|(key, value)| format!(r#","{key}":{value}"#))
        .collect();
    format!(r#"{{"abi":{abi}{parts}}}"#)
}

fn load(
    name: &str,
    abi: u32,
    parts: &[(&str, &str)],
    extra: &str,
) -> Result<std::sync::Arc<PluginModule>, String> {
    let path = common::wat_plugin(name, abi, &manifest(abi, parts), "i32.const 0", extra);
    PluginModule::load(&path).map_err(|error| error.to_string())
}

fn encoder(name: &str, declared: &str) -> WasmEncoder {
    let module = load(name, common::ABI, &[("encoder", declared)], ENCODER_EXPORTS)
        .unwrap_or_else(|error| panic!("{error}"));
    WasmEncoder::of(&module).unwrap_or_else(|| panic!("the manifest declares an encoder"))
}

fn refusal<T>(result: Result<T, String>) -> String {
    match result {
        Ok(_) => panic!("the plugin must be refused"),
        Err(error) => error,
    }
}

#[test]
fn an_encoder_alone_is_a_plugin() {
    let module = load(
        "tx-alone",
        common::ABI,
        &[("encoder", AUDIO_ENCODER)],
        ENCODER_EXPORTS,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert!(WasmDecoder::of(&module).is_none());
    assert!(WasmEncoder::of(&module).is_some());
}

#[test]
fn a_plugin_that_brings_nothing_is_refused() {
    let error = refusal(load("empty", common::ABI, &[], ""));
    assert!(
        error.contains("brings no decoder, encoder or processor"),
        "{error}"
    );
}

#[test]
fn an_encoder_missing_its_exports_is_refused_by_name() {
    let error = refusal(load(
        "no-tx",
        common::ABI,
        &[("encoder", AUDIO_ENCODER)],
        "",
    ));
    assert!(error.contains("encodes but lacks"), "{error}");
    assert!(error.contains("sdrmm_encoder_generate"), "{error}");
}

#[test]
fn kinds_without_a_presenter_are_refused() {
    let text = common::wat_text(
        common::ABI,
        &manifest(common::ABI, &[("kinds", KINDS), ("decoder", DECODER)]),
        "i32.const 0",
        "",
    )
    .replace(
        r#"(func (export "sdrmm_present")"#,
        r#"(func (export "sdrmm_unused")"#,
    );
    let error = match PluginModule::from_bytes("no-present".to_owned(), text.as_bytes(), None) {
        Ok(_) => panic!("the plugin must be refused"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("declares event kinds but lacks sdrmm_present"),
        "{error}"
    );
}

#[test]
fn any_other_abi_is_refused() {
    for abi in [0, 1, 99] {
        let error = refusal(load(
            &format!("abi-{abi}"),
            abi,
            &[("decoder", DECODER)],
            "",
        ));
        assert!(error.contains(&format!("ABI {abi}")), "{error}");
    }
}

#[test]
fn a_message_encoder_is_handed_the_filled_form_as_json() {
    let audio = encoder("tx-audio", AUDIO_ENCODER);
    assert!(matches!(
        audio.compose(&serde_json::json!({"text": "hi"})),
        Err(EncoderError::NoMessages(id)) if id == "probe_tx"
    ));

    let messages = encoder("tx-message", MESSAGE_ENCODER);
    assert_eq!(messages.descriptor().message.len(), 1);
    let Ok(TxPayload::Frame(frame)) = messages.compose(&serde_json::json!({"text": "hi"})) else {
        panic!("a filled form becomes a frame");
    };
    assert_eq!(frame, br#"{"text":"hi"}"#);
}

#[test]
fn an_encoder_opens_and_generates_through_the_abi() {
    let encoder = encoder("tx", AUDIO_ENCODER);
    assert!(matches!(
        encoder.open(&EncoderParams::bare("probe")),
        Err(EncoderError::WrongType { got, .. }) if got == "probe"
    ));
    let mut tx = encoder
        .open(&EncoderParams::bare("probe_tx"))
        .unwrap_or_else(|error| panic!("opens: {error}"));
    let mut out = [num_complex::Complex::new(1.0f32, 1.0); 16];
    assert_eq!(tx.generate(&mut out), 0);
}
