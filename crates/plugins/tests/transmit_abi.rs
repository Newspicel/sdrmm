//! A plugin's transmit promise is checked when it loads, and a plugin built for another ABI is
//! refused.

use sdrmm_channels::{ChannelCtx, ChannelError, Decoder, TxPayload};
use sdrmm_plugins::{PluginModule, WasmDecoder};

mod common;

fn manifest(abi: u32, can_transmit: bool) -> String {
    format!(
        r#"{{"abi":{abi},"descriptor":{{"type_id":"probe","name":"Probe","bandwidth_hz":1000.0,"input_rate_hz":48000.0,"can_transmit":{can_transmit}}}}}"#
    )
}

const TX_EXPORTS: &str = r#"(func (export "sdrmm_tx_open") (param f64 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_tx_apply") (param i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_tx_submit") (param i32 i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_tx_generate") (param i32 i32) (result i64) i64.const 0)
  (func (export "sdrmm_tx_close") (param i32))"#;

fn load(name: &str, abi: u32, can_transmit: bool, extra: &str) -> Result<WasmDecoder, String> {
    let path = common::wat_plugin(
        name,
        abi,
        &manifest(abi, can_transmit),
        "i32.const 0",
        extra,
    );
    PluginModule::load(&path)
        .map(WasmDecoder::new)
        .map_err(|error| error.to_string())
}

fn refusal(result: Result<WasmDecoder, String>) -> String {
    match result {
        Ok(_) => panic!("the plugin must be refused"),
        Err(error) => error,
    }
}

#[test]
fn a_transmitter_missing_its_exports_is_refused_by_name() {
    let error = refusal(load("no-tx", 1, true, ""));
    assert!(error.contains("sdrmm_tx_generate"), "{error}");
}

#[test]
fn any_other_abi_is_refused() {
    for abi in [0, 2, 99] {
        let error = refusal(load(&format!("abi-{abi}"), abi, false, ""));
        assert!(error.contains(&format!("ABI {abi}")), "{error}");
    }
}

#[test]
fn a_plugin_with_a_message_form_is_handed_the_filled_form_as_json() {
    let plain = load("tx-plain", 1, true, TX_EXPORTS).unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(
        plain.compose(&serde_json::json!({"text": "hi"})),
        Err(ChannelError::InvalidPayload(_))
    ));

    let manifest = r#"{"abi":1,"descriptor":{"type_id":"probe","name":"Probe","bandwidth_hz":1000.0,"input_rate_hz":48000.0,"can_transmit":true,"message":[{"key":"text","label":"Text","control":{"type":"text","placeholder":"hello"}}]}}"#;
    let path = common::wat_plugin("tx-message", 1, manifest, "i32.const 0", TX_EXPORTS);
    let decoder = PluginModule::load(&path)
        .map(WasmDecoder::new)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(decoder.descriptor().message.len(), 1);
    let Ok(TxPayload::Frame(frame)) = decoder.compose(&serde_json::json!({"text": "hi"})) else {
        panic!("a filled form becomes a frame");
    };
    assert_eq!(frame, br#"{"text":"hi"}"#);
}

#[test]
fn a_receiver_only_plugin_has_no_modulator() {
    let decoder = load("rx-only", 1, false, TX_EXPORTS).unwrap_or_else(|error| panic!("{error}"));
    assert!(!decoder.descriptor().can_transmit);
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let refused = decoder.create_tx(
        ChannelCtx {
            input_rate: 48_000.0,
        },
        &settings,
    );
    assert!(matches!(refused, Err(ChannelError::NoTransmitter(_))));
}

#[test]
fn a_transmitter_opens_and_generates_through_the_abi() {
    let decoder = load("tx", 1, true, TX_EXPORTS).unwrap_or_else(|error| panic!("{error}"));
    assert!(decoder.descriptor().can_transmit);
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let mut tx = decoder
        .create_tx(
            ChannelCtx {
                input_rate: 48_000.0,
            },
            &settings,
        )
        .unwrap_or_else(|error| panic!("opens: {error}"));
    let mut out = [num_complex::Complex::new(1.0f32, 1.0); 16];
    assert_eq!(tx.generate(&mut out), 0);
}
