//! Array processors as the wire carries them: a type id and the processor's own settings or
//! reading. Only the processor that owns the type id knows their shape.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use utoipa::ToSchema;

pub mod df;
mod kind;
pub mod view;

pub use kind::{
    ArrayNeeds, BandKeys, ProcessorNode, ProcessorPort, ProcessorType, ReadoutSpec,
    install_processor_types, processor_type, processor_types,
};

pub const MAX_AT_LEN: usize = 40;
pub const MIN_BAND_HZ: f64 = 1_000.0;
pub const DEFAULT_BAND_HZ: f64 = 200_000.0;

const NO_TYPE: &str = "Pick a processor";

/// A timestamp string with room for every one a reading writes, so filling it allocates nothing.
#[must_use]
pub fn reserved_at() -> String {
    String::with_capacity(MAX_AT_LEN)
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

/// A processor's settings: its type id and its own settings object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProcessorParams {
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default = "empty_object")]
    #[schema(value_type = std::collections::BTreeMap<String, Value>)]
    pub settings: Value,
}

impl ProcessorParams {
    /// Settings left to the processor's defaults.
    #[must_use]
    pub fn bare(type_id: impl Into<String>) -> Self {
        Self {
            type_id: type_id.into(),
            settings: empty_object(),
        }
    }

    pub fn new(type_id: impl Into<String>, settings: &impl Serialize) -> Result<Self, String> {
        Ok(Self {
            type_id: type_id.into(),
            settings: serde_json::to_value(settings).map_err(|error| error.to_string())?,
        })
    }

    #[must_use]
    pub fn type_id(&self) -> &str {
        &self.type_id
    }

    /// The settings as the processor's own struct, absent fields taking its defaults.
    pub fn parse<P: DeserializeOwned>(&self) -> Result<P, String> {
        let parsed = match &self.settings {
            Value::Null => P::deserialize(&empty_object()),
            settings => P::deserialize(settings),
        };
        parsed.map_err(|error| format!("{} settings: {error}", self.type_id))
    }

    #[must_use]
    pub fn problem(&self) -> Option<&'static str> {
        self.type_id.is_empty().then_some(NO_TYPE)
    }

    #[must_use]
    pub fn valid(&self) -> bool {
        self.problem().is_none()
    }
}

/// What a processor reported: its type id and its own reading.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProcessorReading {
    #[serde(rename = "type")]
    pub type_id: String,
    #[schema(value_type = std::collections::BTreeMap<String, Value>)]
    pub reading: Value,
}

impl ProcessorReading {
    #[must_use]
    pub fn type_id(&self) -> &str {
        &self.type_id
    }

    /// The reading as `R`, when it has that shape.
    pub fn parse<R: DeserializeOwned>(&self) -> Result<R, String> {
        R::deserialize(&self.reading).map_err(|error| format!("{} reading: {error}", self.type_id))
    }
}

#[cfg(test)]
mod tests;
