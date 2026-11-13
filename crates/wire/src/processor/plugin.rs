//! Array processors a plugin brings: what one declares, the settings it runs on and the reading
//! it reports. The server runs them beside the built-in processors.

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    array::{MAX_ARRAY_LANES, MIN_ARRAY_LANES},
    channel::{ChannelHealth, ChannelStatus, SettingField},
    decode::EventKind,
    present::{Field, FieldValue, Unit},
};

const NO_TYPE: &str = "Pick a processor";
const STOPPED: &str = "Stopped";

/// What a processor needs of the array before its results mean anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ArrayNeeds {
    pub time: bool,
    pub phase: bool,
    pub gain: bool,
    pub geometry: bool,
}

/// One number a processor reports, in the order its values arrive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReadoutSpec {
    pub key: String,
    pub label: String,
    #[serde(default = "no_unit")]
    pub unit: Unit,
    #[serde(default)]
    pub digits: u8,
}

const fn no_unit() -> Unit {
    Unit::None
}

impl ReadoutSpec {
    #[must_use]
    pub fn new(key: &str, label: &str, unit: Unit, digits: u8) -> Self {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            unit,
            digits,
        }
    }
}

/// An array processor a plugin declares, as the patch catalog lists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProcessorType {
    pub type_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default = "min_lanes")]
    pub min_lanes: u32,
    #[serde(default = "max_lanes")]
    pub max_lanes: u32,
    #[serde(default)]
    pub needs: ArrayNeeds,
    /// The form a face renders for its settings.
    #[serde(default)]
    pub settings: Vec<SettingField>,
    #[serde(default)]
    pub readouts: Vec<ReadoutSpec>,
    /// Kinds of the events it emits, each one the plugin declares.
    #[serde(default)]
    pub emits: Vec<EventKind>,
    /// What a fresh node starts on.
    #[serde(default)]
    pub defaults: Value,
    #[serde(default)]
    pub facets: super::view::ProcessorFacets,
}

const fn min_lanes() -> u32 {
    MIN_ARRAY_LANES
}

const fn max_lanes() -> u32 {
    MAX_ARRAY_LANES
}

impl ProcessorType {
    /// The settings `params` would run on, needs taken from this type.
    #[must_use]
    pub fn params(&self, settings: Value) -> PluginParams {
        PluginParams {
            type_id: self.type_id.clone(),
            settings,
            needs: self.needs,
        }
    }
}

/// Settings of a plugin processor: its type and the JSON its form edits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginParams {
    pub type_id: String,
    #[serde(default)]
    pub settings: Value,
    /// Copied from the type when the params are made, so the DSP plane never looks it up.
    #[serde(default)]
    pub needs: ArrayNeeds,
}

impl PluginParams {
    #[must_use]
    pub fn problem(&self) -> Option<&'static str> {
        self.type_id.is_empty().then_some(NO_TYPE)
    }
}

/// A patch node running a plugin processor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProcessorNode {
    #[serde(default)]
    pub processor_type: String,
    #[serde(default)]
    pub settings: Value,
}

impl ProcessorNode {
    #[must_use]
    pub fn params(&self) -> PluginParams {
        let needs =
            processor_type(&self.processor_type).map_or_else(ArrayNeeds::default, |t| t.needs);
        PluginParams {
            type_id: self.processor_type.clone(),
            settings: self.settings.clone(),
            needs,
        }
    }
}

/// One declared readout and its latest value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PluginReadout {
    pub key: String,
    pub label: String,
    pub unit: Unit,
    pub digits: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

/// What a plugin processor reported. Built once per type with every readout in place, so a
/// report only writes numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PluginReading {
    pub type_id: String,
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
            type_id: kind.type_id.clone(),
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

    #[must_use]
    pub fn status(&self) -> ChannelStatus {
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
}

static TYPES: LazyLock<RwLock<Arc<Vec<ProcessorType>>>> =
    LazyLock::new(|| RwLock::new(Arc::new(Vec::new())));

/// The plugin processors loaded right now. Read on the control plane only.
#[must_use]
pub fn processor_types() -> Arc<Vec<ProcessorType>> {
    TYPES.read().unwrap_or_else(PoisonError::into_inner).clone()
}

#[must_use]
pub fn processor_type(type_id: &str) -> Option<ProcessorType> {
    processor_types()
        .iter()
        .find(|kind| kind.type_id == type_id)
        .cloned()
}

/// Replaces the plugin processors the catalog lists.
pub fn install_processor_types(types: Vec<ProcessorType>) {
    *TYPES.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(types);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter() -> ProcessorType {
        ProcessorType {
            type_id: "meter".into(),
            name: "Meter".into(),
            summary: String::new(),
            min_lanes: 2,
            max_lanes: 4,
            needs: ArrayNeeds::default(),
            settings: Vec::new(),
            readouts: vec![
                ReadoutSpec::new("power", "Power", Unit::Db, 1),
                ReadoutSpec::new("phase", "Phase", Unit::Degrees, 0),
            ],
            emits: Vec::new(),
            defaults: Value::Null,
            facets: super::super::view::ProcessorFacets::default(),
        }
    }

    #[test]
    fn a_reading_shows_only_the_values_it_has() {
        let mut reading = PluginReading::reserved(&meter());
        reading.fill([-12.5, f64::NAN]);
        let status = reading.status();
        assert_eq!(status.readouts.len(), 1);
        assert_eq!(status.readouts[0].key, "power");
    }

    #[test]
    fn a_stopped_processor_says_why() {
        let mut reading = PluginReading::reserved(&meter());
        reading.error = Some("trapped".into());
        let status = reading.status();
        assert_eq!(
            status.readout("plugin").map(|f| &f.label),
            Some(&STOPPED.to_owned())
        );
    }

    #[test]
    fn a_type_parses_with_only_its_name() {
        let kind: ProcessorType =
            serde_json::from_str(r#"{"type_id":"x","name":"X"}"#).expect("parses");
        assert_eq!(
            (kind.min_lanes, kind.max_lanes),
            (MIN_ARRAY_LANES, MAX_ARRAY_LANES)
        );
        assert!(kind.params(Value::Null).problem().is_none());
        assert_eq!(PluginParams::default().problem(), Some(NO_TYPE));
    }
}
