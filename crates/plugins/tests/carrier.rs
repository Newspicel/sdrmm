//! Builds the example plugin for WASM, loads it the way the server does and runs a channel and
//! an array processor of it.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, OnceLock},
};

use num_complex::Complex;
use sdrmm_channels::array_processor::ProcessorFactory;
use sdrmm_channels::{ChannelCtx, ChannelError, ChannelOutputs, Decoder, TxPayload};
use sdrmm_plugins::{PluginModule, WasmDecoder, WasmProcessors, load_dir};
use sdrmm_wire::{ChannelParams, ChannelSettings, EventKind};

mod common;

use common::array::{Rig, lanes};

const RATE_HZ: f64 = 48_000.0;
const WASM: &str = "sdrmm_plugin_example.wasm";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The example plugin, built once per test run into its own target directory.
fn plugin_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let target = workspace().join("target/plugin-tests");
        let status = Command::new(env!("CARGO"))
            .current_dir(workspace())
            .args([
                "build",
                "-p",
                "sdrmm-plugin-example",
                "--target",
                "wasm32-unknown-unknown",
                "--release",
                "--target-dir",
            ])
            .arg(&target)
            .status()
            .unwrap_or_else(|error| panic!("cargo did not start: {error}"));
        assert!(status.success(), "the example plugin did not build");
        let dir = target.join("plugins");
        std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("plugins dir: {error}"));
        std::fs::copy(
            target.join("wasm32-unknown-unknown/release").join(WASM),
            dir.join(WASM),
        )
        .unwrap_or_else(|error| panic!("copy plugin: {error}"));
        dir
    })
}

fn decoder() -> WasmDecoder {
    let module = PluginModule::load(&plugin_dir().join(WASM))
        .unwrap_or_else(|error| panic!("plugin loads: {error}"));
    WasmDecoder::new(module)
}

fn tone(offset_hz: f64, samples: usize) -> Vec<Complex<f32>> {
    (0..samples)
        .map(|n| {
            let phase = 2.0 * std::f64::consts::PI * offset_hz * n as f64 / RATE_HZ;
            Complex::new(phase.cos() as f32, phase.sin() as f32)
        })
        .collect()
}

#[test]
fn a_plugin_describes_itself_like_a_built_in_decoder() {
    let decoder = decoder();
    let descriptor = decoder.descriptor();
    assert_eq!(descriptor.type_id, "carrier");
    assert_eq!(descriptor.emits, vec![EventKind::named("carrier")]);
    assert_eq!(descriptor.settings.len(), 2);
    let defaults = descriptor.defaults.as_ref().expect("defaults");
    assert_eq!(defaults.params.settings["threshold_db"], 20.0);
    assert!(!descriptor.ports.is_empty());
    let kinds = decoder.kinds();
    assert_eq!(kinds.len(), 1);
    assert_eq!(kinds[0].entry.name, "Carrier");
}

#[test]
fn a_plugin_channel_reports_a_tone_through_the_host_fft() {
    let decoder = decoder();
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let mut channel = decoder
        .create_rx(
            ChannelCtx {
                input_rate: RATE_HZ,
            },
            &settings,
        )
        .unwrap_or_else(|error| panic!("channel opens: {error}"));
    let mut out = ChannelOutputs::default();
    for block in tone(2_000.0, 8_192).chunks(1_000) {
        channel.process(block, &mut out);
    }
    assert_eq!(out.events.len(), 1, "{:?}", out.events);
    let event = &out.events[0];
    assert_eq!(event.kind, EventKind::named("carrier"));
    let offset = event.data()["offset_hz"].as_f64().expect("offset");
    assert!((offset - 2_000.0).abs() < 50.0, "{offset}");
    let view = decoder.kinds()[0]
        .codec
        .present(&event.kind, &event.data())
        .expect("presents");
    assert!(view.summary.starts_with("Carrier +"), "{}", view.summary);
}

#[test]
fn a_plugin_reports_health_and_view_hints_like_a_built_in() {
    let decoder = decoder();
    assert_eq!(decoder.kinds()[0].entry.views.len(), 2);
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let mut channel = decoder
        .create_rx(
            ChannelCtx {
                input_rate: RATE_HZ,
            },
            &settings,
        )
        .unwrap_or_else(|error| panic!("channel opens: {error}"));
    let mut out = ChannelOutputs::default();
    channel.process(&tone(-3_000.0, 1_024 * 16), &mut out);
    let health = out.status.expect("a health report").health;
    assert_eq!(health.lock, Some(true));
    assert!((health.freq_error_hz.expect("offset") + 3_000.0).abs() < 50.0);
}

#[test]
fn a_plugin_transmits_what_its_own_receiver_hears() {
    let decoder = decoder();
    assert!(decoder.descriptor().can_transmit);
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let ctx = ChannelCtx {
        input_rate: RATE_HZ,
    };
    let mut tx = decoder
        .create_tx(ctx, &settings)
        .unwrap_or_else(|error| panic!("transmitter opens: {error}"));
    tx.submit(TxPayload::Frame(vec![0xFF; 8]))
        .unwrap_or_else(|error| panic!("frame queues: {error}"));
    let refused = tx.submit(TxPayload::Audio(vec![0.0; 4]));
    let Err(ChannelError::InvalidPayload(reason)) = refused else {
        panic!("audio must be refused, got {refused:?}");
    };
    assert!(reason.contains("frames"), "{reason}");
    let mut wave = vec![Complex::new(0.0f32, 0.0); 64 * 480 + 100];
    let sent = tx.generate(&mut wave);
    assert_eq!(sent, 64 * 480);
    let mut rx = decoder
        .create_rx(ctx, &settings)
        .unwrap_or_else(|error| panic!("receiver opens: {error}"));
    let mut out = ChannelOutputs::default();
    for block in wave[..sent].chunks(2_000) {
        rx.process(block, &mut out);
    }
    assert_eq!(out.events.len(), 1, "{:?}", out.events);
    let offset = out.events[0].data()["offset_hz"].as_f64().expect("offset");
    assert!((offset - 1_000.0).abs() < 50.0, "{offset}");
}

#[test]
fn a_plugin_refuses_what_it_cannot_run() {
    let decoder = decoder();
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let refused = decoder.create_rx(
        ChannelCtx {
            input_rate: 96_000.0,
        },
        &settings,
    );
    let Err(error) = refused else {
        panic!("a 96 kHz feed must be refused");
    };
    assert!(error.to_string().contains("48000"), "{error}");
    let other = ChannelSettings::new(1e6, ChannelParams::bare("nfm"));
    assert!(decoder.layout(&other.params).is_err());
}

#[test]
fn a_plugin_layout_and_filter_come_from_its_settings() {
    let decoder = decoder();
    let params = ChannelParams {
        type_id: "carrier".to_owned(),
        settings: serde_json::json!({ "bandwidth_hz": 4_000.0 }),
    };
    let layout = decoder.layout(&params).expect("layout");
    assert_eq!(layout.band, (-2_000.0, 2_000.0));
    assert_eq!(layout.input_rate_hz, RATE_HZ);
    let normalized = decoder.normalize(&params).expect("normalize");
    assert_eq!(normalized.settings["threshold_db"], 20.0);
    assert!(decoder.filter(&params).is_ok());
}

#[test]
fn the_example_lane_meter_reports_power_and_phase_through_the_abi() {
    let module = PluginModule::load(&plugin_dir().join(WASM))
        .unwrap_or_else(|error| panic!("plugin loads: {error}"));
    let factory = WasmProcessors::of(&module).expect("a processor");
    assert_eq!(factory.kind().type_id, "lane_meter");
    let mut rig = Rig::new(factory.kind(), 3);
    let mut meter = rig
        .open(&factory, serde_json::json!({ "blocks": 2 }))
        .unwrap_or_else(|error| panic!("opens: {error}"));
    let block = lanes(&[0.0, 60.0, -120.0], 512);
    assert!(!rig.run(meter.as_mut(), &block).report);
    assert!(rig.run(meter.as_mut(), &block).report);
    let power = rig.value("power_2").expect("lane 2 power");
    assert!((power - 20.0 * 0.5f64.log10()).abs() < 0.1, "{power}");
    let phase = rig.value("phase_2").expect("lane 2 phase");
    assert!((phase - 60.0).abs() < 0.5, "{phase}");
    assert!((rig.value("phase_3").expect("lane 3 phase") + 120.0).abs() < 0.5);
    assert_eq!(rig.value("power_4"), None);
}

#[test]
fn a_directory_loads_every_plugin_in_it() {
    let (decoders, errors) = load_dir(plugin_dir());
    assert!(errors.is_empty(), "{errors:?}");
    let ids: Vec<String> = decoders
        .iter()
        .map(|decoder: &Arc<dyn Decoder>| decoder.descriptor().type_id.clone())
        .collect();
    assert_eq!(ids, ["carrier"]);
    let (none, missing) = load_dir(&plugin_dir().join("absent"));
    assert!(none.is_empty() && missing.is_empty());
}
