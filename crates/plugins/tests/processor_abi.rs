//! A plugin's array processor round trip through the ABI: its declaration is checked when it
//! loads, lanes go in, readouts, health and events come out, and a runaway stops it visibly.

use std::{sync::Arc, time::Instant};

use sdrmm_channels::{
    ChannelError,
    array_processor::{ProcessorFactory, ProcessorFaults, create_processor, install_processors},
};
use sdrmm_plugins::{PluginModule, WasmProcessors};
use sdrmm_wire::{
    ProcessorParams,
    plugin::processor::{ArrayHeader, lock},
    processor_type,
};

mod common;

use common::array::{Rig, lanes};

const HEADER_AT: u32 = 40_000;
const VALUES_AT: u32 = 40_100;
const EVENTS_AT: u32 = 40_200;
const EVENTS: &str = r#"[{"kind":"probe","data":{"n":1}}]"#;

fn manifest(processor: &str) -> String {
    format!(
        r#"{{"abi":1,"descriptor":{{"type_id":"probe_rx","name":"Probe","bandwidth_hz":1000.0,"input_rate_hz":48000.0}},"processor":{processor}}}"#
    )
}

const METER: &str = r#"{"type_id":"probe_meter","name":"Probe meter","min_lanes":2,"max_lanes":4,"readouts":[{"key":"a","label":"A","unit":"db","digits":1},{"key":"b","label":"B"}]}"#;

/// WAT data for `bytes`, every byte escaped.
fn escaped(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("\\{byte:02x}")).collect()
}

fn header() -> ArrayHeader {
    ArrayHeader {
        report: 1,
        values_ptr: VALUES_AT,
        values_len: 2,
        lock: lock::HELD,
        snr_db: 12.0,
        freq_error_hz: f32::NAN,
        errors_per_s: f32::NAN,
        events_ptr: EVENTS_AT,
        events_len: EVENTS.len() as u32,
        truncated: 3,
    }
}

/// The array exports, `process` being the body of `sdrmm_array_process`.
fn exports(process: &str) -> String {
    let values: Vec<u8> = [3.5f64, f64::NAN]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    format!(
        r#"(data (i32.const {HEADER_AT}) "{header}")
  (data (i32.const {VALUES_AT}) "{values}")
  (data (i32.const {EVENTS_AT}) "{events}")
  (func (export "sdrmm_array_open") (param i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_array_apply") (param i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_array_retune") (param i32 i32 i32) (result i32) i32.const 0)
  (func (export "sdrmm_array_reset") (param i32 i32))
  (func (export "sdrmm_array_input") (param i32 i32 i32) (result i32) i32.const 16384)
  (func (export "sdrmm_array_process") (param i32 i32 i32) (result i32)
    {process})
  (func (export "sdrmm_array_close") (param i32))"#,
        header = escaped(&header().to_bytes()),
        values = escaped(&values),
        events = escaped(EVENTS.as_bytes()),
    )
}

fn load(name: &str, processor: &str, extra: &str) -> Result<Arc<PluginModule>, String> {
    let path = common::wat_plugin(name, 1, &manifest(processor), "i32.const 0", extra);
    PluginModule::load(&path).map_err(|error| error.to_string())
}

fn factory(name: &str, process: &str) -> WasmProcessors {
    let module = load(name, METER, &exports(process)).unwrap_or_else(|error| panic!("{error}"));
    WasmProcessors::of(&module).unwrap_or_else(|| panic!("the plugin declares a processor"))
}

fn refusal(result: Result<Arc<PluginModule>, String>) -> String {
    match result {
        Ok(_) => panic!("the plugin must be refused"),
        Err(error) => error,
    }
}

#[test]
fn a_processor_missing_its_exports_is_refused_by_name() {
    let error = refusal(load("no-array", METER, ""));
    assert!(error.contains("sdrmm_array_process"), "{error}");
}

#[test]
fn a_processor_emitting_an_undeclared_kind_is_refused() {
    let ghost = METER.replace(r#""max_lanes":4"#, r#""max_lanes":4,"emits":["ghost"]"#);
    let error = refusal(load(
        "ghost",
        &ghost,
        &exports(&format!("i32.const {HEADER_AT}")),
    ));
    assert!(error.contains("ghost"), "{error}");
}

#[test]
fn lanes_in_readouts_health_and_events_out() {
    let factory = factory("probe-array", &format!("i32.const {HEADER_AT}"));
    let mut rig = Rig::new(factory.kind(), 2);
    let mut processor = rig
        .open(&factory, serde_json::Value::Null)
        .unwrap_or_else(|error| panic!("opens: {error}"));
    let tally = rig.run(processor.as_mut(), &lanes(&[0.0, 90.0], 256));
    assert!(tally.report && tally.events, "{tally:?}");
    assert_eq!(rig.value("a"), Some(3.5));
    assert_eq!(rig.value("b"), None);
    let health = rig.plugin_reading().health;
    assert_eq!(health.lock, Some(true));
    assert_eq!(health.snr_db, Some(12.0));
    assert_eq!(health.freq_error_hz, None);
    assert_eq!(rig.events.as_slice(), EVENTS.as_bytes());
    assert_eq!(processor.faults().truncated, 3);
}

#[test]
fn a_loaded_processor_runs_by_its_type_and_refuses_too_few_lanes() {
    let factory = factory("probe-lanes", &format!("i32.const {HEADER_AT}"));
    let params = ProcessorParams::Plugin(factory.kind().params(serde_json::Value::Null));
    let kind = factory.kind().clone();
    install_processors(vec![Arc::new(factory)]).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(processor_type(&kind.type_id), Some(kind.clone()));
    let pair = Rig::new(&kind, 2);
    assert!(create_processor(&pair.ctx(), &params).is_ok());
    let single = Rig::new(&kind, 1);
    let refused = create_processor(&single.ctx(), &params).map(|_| ());
    assert!(
        matches!(refused, Err(ChannelError::Refused("Too few elements"))),
        "{refused:?}"
    );
}

#[test]
fn a_processor_stuck_in_a_loop_stops_with_a_visible_error() {
    let factory = factory(
        "probe-runaway",
        "(loop $forever (br $forever))\n    i32.const 0",
    );
    let mut rig = Rig::new(factory.kind(), 2);
    let mut processor = rig
        .open(&factory, serde_json::Value::Null)
        .unwrap_or_else(|error| panic!("opens: {error}"));
    let block = lanes(&[0.0, 0.0], 64);
    let started = Instant::now();
    let tally = rig.run(processor.as_mut(), &block);
    assert!(started.elapsed().as_secs_f64() < 5.0);
    assert!(tally.report);
    assert!(rig.plugin_reading().error.is_some());
    assert_eq!(
        processor.faults(),
        ProcessorFaults {
            solver_failures: 1,
            ..ProcessorFaults::default()
        }
    );
    let started = Instant::now();
    let tally = rig.run(processor.as_mut(), &block);
    assert!(
        started.elapsed().as_millis() < 50,
        "a stopped processor returns at once"
    );
    assert!(!tally.report);
}
