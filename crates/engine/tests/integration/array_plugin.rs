#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A loaded array processor, as a plugin brings one, runs on an array's lanes beside the
//! built-ins, hands on its events and reopens when its plugin reloads.

use std::sync::Arc;

use sdrmm_channels::{
    ChannelError,
    array_processor::{
        ArrayBlock, ArrayCtx, ArrayProcessor, ProcessorAction, ProcessorDescriptor,
        ProcessorFactory, ProcessorFaults, ProcessorOutput, ResetCause, install_processors,
        plugin_descriptor,
    },
};
use sdrmm_wire::{
    ArrayCal, ArrayCalSource, ArrayNeeds, DecodedRecord, EventKind, PluginParams, PluginReading,
    ProcessorParams, ProcessorReading, ProcessorType, ReadoutSpec, present::Unit,
};
use tokio::sync::broadcast::{Receiver, error::TryRecvError};

use crate::common::array::{
    Bench, KRAKEN, WAIT, drain, kraken_array, next_reading, processor, wait_for,
};

const TYPE_ID: &str = "engine_lane_meter";
const NODE: &str = "meter";
const REPORT_EVERY: u32 = 8;
const EVENTS: &str = r#"[{"kind":"lane_meter","data":{"blocks":8}}]"#;

struct Meter {
    lanes: usize,
    build: f64,
    blocks: u32,
    power: f64,
    samples: usize,
    faults: ProcessorFaults,
}

impl ArrayProcessor for Meter {
    fn descriptor() -> &'static ProcessorDescriptor {
        plugin_descriptor()
    }

    fn new(_: &ArrayCtx<'_>, params: &ProcessorParams) -> Result<Self, ChannelError> {
        Err(ChannelError::UnknownType(params.type_id().to_owned()))
    }

    fn apply(&mut self, _: &ProcessorParams) -> Result<(), ChannelError> {
        Ok(())
    }

    fn retune(&mut self, ctx: &ArrayCtx<'_>) -> Result<(), ChannelError> {
        self.lanes = ctx.lanes;
        Ok(())
    }

    fn reset(&mut self, _: ResetCause) {}

    fn action(&mut self, _: ProcessorAction) -> Result<(), ChannelError> {
        Err(ChannelError::Refused("No actions"))
    }

    fn process(&mut self, block: &ArrayBlock<'_>, out: &mut ProcessorOutput<'_>) {
        if !self.faults.lanes_match(block, self.lanes) {
            return;
        }
        self.power += block.lanes[0]
            .iter()
            .map(|sample| f64::from(sample.norm_sqr()))
            .sum::<f64>();
        self.samples += block.len();
        self.blocks += 1;
        if !self.blocks.is_multiple_of(REPORT_EVERY) {
            return;
        }
        let power_db = 10.0 * (self.power / self.samples.max(1) as f64).log10();
        if let Some(ProcessorReading::Plugin(reading)) = out.report() {
            reading.fill([self.build, self.lanes as f64, power_db]);
            out.publish_report();
        }
        out.events(EVENTS.as_bytes());
    }

    fn faults(&self) -> ProcessorFaults {
        self.faults
    }
}

/// One build of the plugin: its readout `build` tells builds apart.
struct MeterBuild {
    kind: ProcessorType,
    build: f64,
}

impl ProcessorFactory for MeterBuild {
    fn kind(&self) -> &ProcessorType {
        &self.kind
    }

    fn create(
        &self,
        ctx: &ArrayCtx<'_>,
        _: &PluginParams,
    ) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
        Ok(Box::new(Meter {
            lanes: ctx.lanes,
            build: self.build,
            blocks: 0,
            power: 0.0,
            samples: 0,
            faults: ProcessorFaults::default(),
        }))
    }
}

fn build(build: f64) -> Arc<dyn ProcessorFactory> {
    Arc::new(MeterBuild {
        kind: ProcessorType {
            type_id: TYPE_ID.to_owned(),
            name: "Lane meter".to_owned(),
            summary: String::new(),
            min_lanes: 2,
            max_lanes: 16,
            needs: ArrayNeeds::default(),
            settings: Vec::new(),
            readouts: vec![
                ReadoutSpec::new("build", "Build", Unit::None, 0),
                ReadoutSpec::new("lanes", "Lanes", Unit::None, 0),
                ReadoutSpec::new("power", "Power", Unit::Dbfs, 1),
            ],
            emits: vec![EventKind::named("lane_meter")],
            defaults: serde_json::Value::Null,
            facets: sdrmm_wire::ProcessorFacets::default(),
        },
        build,
    })
}

fn plugin_reading(reading: &ProcessorReading) -> PluginReading {
    match reading {
        ProcessorReading::Plugin(reading) => reading.clone(),
        other => panic!("a plugin reading, got {other:?}"),
    }
}

fn value(reading: &PluginReading, key: &str) -> Option<f64> {
    reading
        .readouts
        .iter()
        .find(|readout| readout.key == key)
        .and_then(|readout| readout.value)
}

fn next_event(decoded: &mut Receiver<DecodedRecord>) -> DecodedRecord {
    wait_for("an event from the meter", WAIT, || {
        loop {
            match decoded.try_recv() {
                Ok(record) if record.origin.as_ref().is_some_and(|o| o.node == NODE) => {
                    return Some(record);
                }
                Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Closed) => panic!("the decoded stream closed"),
            }
        }
    })
}

#[test]
fn a_plugin_processor_runs_on_the_lanes_and_reopens_on_reload() {
    install_processors(vec![build(1.0)]).unwrap();
    let bench = Bench::new();
    let ds = bench.open(KRAKEN);
    let mut array = kraken_array(ds);
    array.settings.cal = ArrayCal {
        source: ArrayCalSource::Off,
        ..ArrayCal::default()
    };
    bench.engine.apply_array(array).unwrap();
    let kind = build(1.0).kind().clone();
    let mut events = bench.engine.subscribe_arrays();
    let mut decoded = bench.engine.subscribe_decoded();
    bench
        .engine
        .apply_processor(processor(
            NODE,
            ProcessorParams::Plugin(kind.params(serde_json::Value::Null)),
        ))
        .unwrap();

    let first = plugin_reading(&next_reading(&mut events, NODE, WAIT));
    assert_eq!(value(&first, "build"), Some(1.0));
    assert_eq!(value(&first, "lanes"), Some(5.0));
    assert!(value(&first, "power").is_some_and(f64::is_finite));
    assert!(first.status().readout("power").is_some());
    let event = next_event(&mut decoded);
    assert_eq!(event.event.kind, EventKind::named("lane_meter"));

    let replaced = install_processors(vec![build(2.0)]).unwrap();
    assert_eq!(replaced, [TYPE_ID]);
    assert!(bench.engine.plugins_reloaded(&replaced).is_empty());
    drain(&mut events);
    let reopened = wait_for("a reading from the new build", WAIT, || {
        let reading = plugin_reading(&next_reading(&mut events, NODE, WAIT));
        (value(&reading, "build") == Some(2.0)).then_some(reading)
    });
    assert_eq!(value(&reopened, "lanes"), Some(5.0));
}
