//! Array processors loaded at run time, such as a plugin's, beside the built-in ones. Each
//! loaded type is its own descriptor, its facets read from what it declared.

use std::sync::Arc;

use sdrmm_decoder::ChannelError;
use sdrmm_wire::{
    ArrayTuningMode, ProcessorParams, ProcessorType, ProcessorView,
    channel::{ChannelHealth, ChannelStatus},
    present::{Field, FieldValue, Unit},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    ArrayCtx, ArrayProcessor, EVENT_BYTES, Erasable, Execution, GpuUse, LaneFormat, MakeEnricher,
    MissionShape, Params, ProcessorDescriptor, ProcessorNeeds, Reading, Report, TuningNeed,
    no_lane_format,
};

const STOPPED: &str = "Stopped";

/// Builds the processors of one loaded type.
pub trait ProcessorFactory: Send + Sync {
    fn kind(&self) -> &ProcessorType;

    fn create(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &PluginSettings,
    ) -> Result<Box<dyn ArrayProcessor>, ChannelError>;
}

/// The settings of a loaded processor: the JSON its form edits, handed to it as they are.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PluginSettings {
    pub settings: Value,
}

impl Erasable for PluginSettings {}

pub(super) struct PluginDescriptor {
    factory: Arc<dyn ProcessorFactory>,
}

impl PluginDescriptor {
    pub(super) const fn new(factory: Arc<dyn ProcessorFactory>) -> Self {
        Self { factory }
    }
}

impl ProcessorDescriptor for PluginDescriptor {
    fn kind(&self) -> &ProcessorType {
        self.factory.kind()
    }

    fn parse(&self, params: &ProcessorParams) -> Result<Params, String> {
        Ok(Params::erase(
            &self.kind().type_id,
            PluginSettings {
                settings: params.settings.clone(),
            },
        ))
    }

    fn reading(&self) -> Option<Reading> {
        Some(Reading::new(
            &self.kind().type_id,
            PluginReading::reserved(self.kind()),
        ))
    }

    fn needs(&self, _: &Params) -> ProcessorNeeds {
        let needs = self.kind().needs;
        ProcessorNeeds {
            time: needs.time,
            phase: needs.phase,
            gain: needs.gain,
            geometry: needs.geometry,
        }
    }

    fn band(&self, params: &Params) -> Option<(f64, f64)> {
        let keys = self.kind().band.as_ref()?;
        let settings = &params.get::<PluginSettings>()?.settings;
        let read = |key: &str| settings.get(key).and_then(Value::as_f64);
        read(&keys.offset).zip(read(&keys.width))
    }

    fn tuning(&self, _: &Params) -> TuningNeed {
        match self.kind().facets.tuning {
            Some(ArrayTuningMode::Together) => TuningNeed::Together,
            Some(ArrayTuningMode::Spread) => TuningNeed::Spread,
            None => TuningNeed::Any,
        }
    }

    fn lane_format(&self, params: &Params, ctx: &ArrayCtx<'_>, port: usize) -> LaneFormat {
        if port < self.kind().lane_ports().count() {
            LaneFormat {
                center_hz: ctx.center_hz,
                sample_rate: ctx.sample_rate,
                capacity: ctx.max_block,
            }
        } else {
            no_lane_format(params, ctx, port)
        }
    }

    fn execution(&self, _: &Params, ctx: &ArrayCtx<'_>) -> Execution {
        Execution::Worker {
            batch: ctx.max_block,
        }
    }

    fn in_place(&self, previous: &Params, next: &Params) -> bool {
        previous.type_id() == next.type_id()
    }

    fn gpu(&self, _: &Params) -> GpuUse {
        GpuUse::Off
    }

    fn event_bytes(&self) -> usize {
        EVENT_BYTES
    }

    fn enricher(&self) -> Option<MakeEnricher> {
        None
    }

    fn mission(&self) -> Option<MissionShape> {
        None
    }

    fn create(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &Params,
    ) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
        let settings = params
            .get::<PluginSettings>()
            .ok_or_else(|| ChannelError::UnknownType(params.type_id().to_owned()))?;
        self.factory.create(ctx, settings)
    }
}

/// One declared readout and its latest value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginReadout {
    pub key: String,
    pub label: String,
    pub unit: Unit,
    pub digits: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

/// What a loaded processor reported. Built once per type with every readout in place, so a
/// report only writes numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginReading {
    #[serde(default)]
    pub health: ChannelHealth,
    #[serde(default)]
    pub readouts: Vec<PluginReadout>,
    /// Why the processor stopped, once it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PluginReading {
    #[must_use]
    pub fn reserved(kind: &ProcessorType) -> Self {
        Self {
            health: ChannelHealth::default(),
            readouts: kind
                .readouts
                .iter()
                .map(|spec| PluginReadout {
                    key: spec.key.clone(),
                    label: spec.label.clone(),
                    unit: spec.unit,
                    digits: spec.digits,
                    value: None,
                })
                .collect(),
            error: None,
        }
    }

    /// Takes `values` in declaration order; a NaN or a missing value clears its readout.
    pub fn fill(&mut self, values: impl IntoIterator<Item = f64>) {
        let mut values = values.into_iter();
        for readout in &mut self.readouts {
            readout.value = values.next().filter(|value| !value.is_nan());
        }
    }
}

impl Report for PluginReading {
    fn status(&self) -> ChannelStatus {
        let mut readouts: Vec<Field> = self
            .readouts
            .iter()
            .filter_map(|readout| {
                readout.value.map(|value| {
                    Field::new(
                        readout.key.clone(),
                        readout.label.clone(),
                        FieldValue::Number {
                            value,
                            unit: readout.unit,
                            digits: readout.digits,
                        },
                    )
                })
            })
            .collect();
        if let Some(error) = &self.error {
            readouts.push(Field::new(
                "plugin",
                STOPPED,
                FieldValue::Text(error.clone()),
            ));
        }
        ChannelStatus {
            health: self.health,
            readouts,
            choices: Vec::new(),
        }
    }

    fn view(&self) -> ProcessorView {
        ProcessorView::default()
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::ReadoutSpec;

    use super::*;
    use crate::{install_plugins, processor_plugin};

    struct Named(ProcessorType);

    impl ProcessorFactory for Named {
        fn kind(&self) -> &ProcessorType {
            &self.0
        }

        fn create(
            &self,
            _: &ArrayCtx<'_>,
            _: &PluginSettings,
        ) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
            Err(ChannelError::UnknownType(self.0.type_id.clone()))
        }
    }

    fn kind(type_id: &str) -> ProcessorType {
        let mut kind: ProcessorType =
            serde_json::from_value(serde_json::json!({"type_id": type_id, "name": type_id}))
                .expect("a bare type parses");
        kind.readouts = vec![
            ReadoutSpec::new("power", "Power", Unit::Db, 1),
            ReadoutSpec::new("phase", "Phase", Unit::Degrees, 0),
        ];
        kind.needs.phase = true;
        kind
    }

    fn named(type_id: &str) -> Arc<dyn ProcessorFactory> {
        Arc::new(Named(kind(type_id)))
    }

    #[test]
    fn a_loaded_type_may_not_shadow_a_built_in_or_load_twice() {
        for refused in [vec![named("df")], vec![named("twin"), named("twin")]] {
            assert!(install_plugins(refused).is_err());
        }
        assert!(processor_plugin("twin").is_none());
    }

    #[test]
    fn a_loaded_type_declares_its_own_facets() {
        let descriptor = PluginDescriptor::new(named("meter"));
        let params = descriptor
            .parse(&ProcessorParams::bare("meter"))
            .expect("parses");
        assert!(descriptor.needs(&params).phase);
        assert!(descriptor.in_place(&params, &params));
        let other = Params::erase("other", PluginSettings::default());
        assert!(!descriptor.in_place(&params, &other));
        assert_eq!(descriptor.type_id(), "meter");
    }

    #[test]
    fn a_reading_shows_only_the_values_it_has() {
        let mut reading = PluginReading::reserved(&kind("meter"));
        reading.fill([-12.5, f64::NAN]);
        let status = reading.status();
        assert_eq!(status.readouts.len(), 1);
        assert_eq!(status.readouts[0].key, "power");
    }

    #[test]
    fn a_stopped_processor_says_why() {
        let mut reading = PluginReading::reserved(&kind("meter"));
        reading.error = Some("trapped".into());
        let status = reading.status();
        assert_eq!(
            status.readout("plugin").map(|f| &f.label),
            Some(&STOPPED.to_owned())
        );
    }
}
