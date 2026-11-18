//! Builds the example plugin for WASM, loads it the way the server does and runs a channel and
//! an array processor of it.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, OnceLock},
    time::{Duration, UNIX_EPOCH},
};

use num_complex::Complex;
use sdrmm_array::ProcessorFactory;
use sdrmm_decoder::{
    ChannelCtx, ChannelOutputs, Decoder,
    encoder::{Encoder, EncoderError, EncoderParams, TxPayload},
};
use sdrmm_plugins::{
    PluginModule, WasmDecoder, WasmEncoder, WasmProcessors, load_dir, plugin_kinds,
};
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DecoderEvent, EventKind, FilterSpec, IdentSignal, PositionFix,
};

mod common;

use common::array::{Rig, lanes};

const RATE_HZ: f64 = 48_000.0;
const WASM: &str = "sdrmm_plugin_example.wasm";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The example plugin, built once per test process into its own target directory and moved
/// into place whole, so a test in another process never loads it half copied.
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
        let staged = target.join(format!("{WASM}.{}", std::process::id()));
        std::fs::copy(
            target.join("wasm32-unknown-unknown/release").join(WASM),
            &staged,
        )
        .unwrap_or_else(|error| panic!("copy plugin: {error}"));
        std::fs::rename(&staged, dir.join(WASM))
            .unwrap_or_else(|error| panic!("place plugin: {error}"));
        dir
    })
}

fn module() -> std::sync::Arc<PluginModule> {
    PluginModule::load(&plugin_dir().join(WASM))
        .unwrap_or_else(|error| panic!("plugin loads: {error}"))
}

fn decoder() -> WasmDecoder {
    WasmDecoder::of(&module()).unwrap_or_else(|| panic!("the plugin declares a decoder"))
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
    let kinds = plugin_kinds(&module());
    assert_eq!(kinds.len(), 1);
    assert_eq!(kinds[0].entry.name, "Carrier");
}

#[test]
fn a_plugin_channel_reports_a_tone_through_the_host_fft() {
    let decoder = decoder();
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let mut channel = decoder
        .open(
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
    let view = plugin_kinds(&module())[0]
        .codec
        .present(&event.kind, &event.data())
        .expect("presents");
    assert!(view.summary.starts_with("Carrier +"), "{}", view.summary);
}

#[test]
fn a_plugin_reports_health_and_view_hints_like_a_built_in() {
    let decoder = decoder();
    assert_eq!(plugin_kinds(&module())[0].entry.views.len(), 2);
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let mut channel = decoder
        .open(
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
    let encoder = WasmEncoder::of(&module()).expect("the example brings an encoder");
    assert_eq!(encoder.descriptor().message.len(), 1);
    let Ok(TxPayload::Frame(text)) = encoder.compose(&serde_json::json!({"text": "CQ"})) else {
        panic!("the beacon's form composes to a frame");
    };
    assert_eq!(text, b"CQ");
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let ctx = ChannelCtx {
        input_rate: RATE_HZ,
    };
    let mut tx = encoder
        .open(&EncoderParams::bare(&encoder.descriptor().type_id))
        .unwrap_or_else(|error| panic!("transmitter opens: {error}"));
    tx.submit(TxPayload::Frame(vec![0xFF; 8]))
        .unwrap_or_else(|error| panic!("frame queues: {error}"));
    let refused = tx.submit(TxPayload::Audio(vec![0.0; 4]));
    let Err(EncoderError::Refused { reason, .. }) = refused else {
        panic!("audio must be refused, got {refused:?}");
    };
    assert!(reason.contains("frames"), "{reason}");
    let mut wave = vec![Complex::new(0.0f32, 0.0); 64 * 480 + 100];
    let sent = tx.generate(&mut wave);
    assert_eq!(sent, 64 * 480);
    let mut rx = decoder
        .open(ctx, &settings)
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
    let refused = decoder.open(
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
    assert_eq!(
        decoder.filter(&params).expect("a filter"),
        FilterSpec::lowpass(65, 2_000.0)
    );
}

#[test]
fn a_plugin_shapes_its_settings_to_an_identified_signal() {
    let decoder = decoder();
    let params = ChannelParams::bare("carrier");
    let signal = IdentSignal {
        bandwidth_hz: 3_000.0,
        ..IdentSignal::default()
    };
    let tuned = decoder.tune(&params, &signal).expect("tunes");
    assert_eq!(tuned.params.settings["bandwidth_hz"], 6_000.0);
    assert_eq!(tuned.offset_hz, 0.0);
}

#[test]
fn a_plugin_feed_frames_events_through_the_plugin() {
    let feeds = decoder().feeds();
    assert_eq!(feeds.len(), 1);
    assert_eq!(feeds[0].format, "carrier-text");
    let event = DecoderEvent::tagged(
        EventKind::named("carrier"),
        &serde_json::json!({"offset_hz": -500.0, "snr_db": 12.0}),
    );
    let line = feeds[0].encoder.encode(&event).expect("encodes");
    assert_eq!(line, b"-500 Hz 12.0 dB\n");
    let other = DecoderEvent::tagged(EventKind::named("other"), &1);
    assert!(feeds[0].encoder.encode(&other).is_err());
}

#[test]
fn a_plugin_channel_hears_its_hooks() {
    let decoder = decoder();
    let settings = decoder.descriptor().defaults.clone().expect("defaults");
    let mut channel = decoder
        .open(
            ChannelCtx {
                input_rate: RATE_HZ,
            },
            &settings,
        )
        .unwrap_or_else(|error| panic!("channel opens: {error}"));
    assert!(
        !channel.needs_gated_input(),
        "the carrier waits for squelch"
    );
    let mut out = ChannelOutputs::default();
    channel.lo_artifact_at(Some(1_500.0));
    channel.process(&tone(1_500.0, 4_096), &mut out);
    assert!(out.events.is_empty(), "the LO artifact is no carrier");
    channel.lo_artifact_at(None);
    let fix: PositionFix = serde_json::from_value(serde_json::json!({
        "latitude": 48.1,
        "longitude": 11.6,
        "time": "2026-11-17T12:00:00Z",
    }))
    .expect("a fix");
    channel.position_changed(Some(&fix));
    channel.captured_at(UNIX_EPOCH + Duration::from_secs(3));
    channel.process(&tone(2_000.0, 4_096), &mut out);
    assert_eq!(out.events.len(), 1, "{:?}", out.events);
    let data = out.events[0].data();
    assert_eq!(data["heard_at_ms"], 3_000);
    assert_eq!(data["receiver"]["latitude"], 48.1);
}

#[test]
fn a_plugin_encoder_width_follows_its_settings() {
    let encoder = WasmEncoder::of(&module()).expect("the example brings an encoder");
    let params = EncoderParams {
        type_id: "beacon".to_owned(),
        settings: serde_json::json!({"baud": 50.0}),
    };
    assert_eq!(encoder.width_hz(&params).expect("a width"), 2_200.0);
    assert!(encoder.compose(&serde_json::json!({"nope": 1})).is_err());
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
    let sum = rig.lanes[0].samples();
    assert_eq!(sum.len(), 512, "the sum lane carries the block");
    let want = (block[0][7] + block[1][7] + block[2][7]) / 3.0;
    assert!((sum[7] - want).norm() < 1e-5, "{} vs {want}", sum[7]);
}

#[test]
fn a_directory_loads_every_part_of_every_plugin_in_it() {
    let scan = load_dir(plugin_dir());
    assert!(scan.errors.is_empty(), "{:?}", scan.errors);
    let decoders: Vec<String> = scan
        .decoders
        .iter()
        .map(|decoder: &Arc<dyn Decoder>| decoder.descriptor().type_id.clone())
        .collect();
    assert_eq!(decoders, ["carrier"]);
    let encoders: Vec<&str> = scan
        .encoders
        .iter()
        .map(|encoder| encoder.descriptor().type_id.as_str())
        .collect();
    assert_eq!(encoders, ["beacon"]);
    assert_eq!(scan.processors.len(), 1);
    assert_eq!(scan.kinds.len(), 1);
    let none = load_dir(&plugin_dir().join("absent"));
    assert!(none.decoders.is_empty() && none.errors.is_empty());
}
