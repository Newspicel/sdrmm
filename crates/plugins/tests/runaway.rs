//! A plugin that never returns stops its own channel and leaves the caller free.

use std::{path::PathBuf, time::Instant};

use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs, Decoder};
use sdrmm_plugins::{PluginModule, WasmDecoder};
use sdrmm_wire::present::FieldValue;

mod common;

const MANIFEST: &str = r#"{"abi":1,"descriptor":{"type_id":"runaway","name":"Runaway","bandwidth_hz":1000.0,"input_rate_hz":48000.0}}"#;

fn plugin_path() -> PathBuf {
    common::wat_plugin(
        "runaway",
        1,
        MANIFEST,
        "(loop $forever (br $forever))\n    i32.const 0",
        "",
    )
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
