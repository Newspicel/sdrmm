//! Builds the example plugin for WASM, loads it the way the server does and runs a channel of it.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, OnceLock},
};

use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs, Decoder};
use sdrmm_plugins::{PluginModule, WasmDecoder, load_dir};
use sdrmm_wire::{ChannelParams, ChannelSettings, EventKind};

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
