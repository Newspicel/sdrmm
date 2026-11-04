use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use utoipa::ToSchema;

/// A decoder's settings as the wire carries them: its type id and its own settings object.
///
/// Only the decoder that owns `type_id` knows the shape of `settings`; everything else passes it
/// through untouched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelParams {
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default = "empty_object")]
    #[schema(value_type = std::collections::BTreeMap<String, Value>)]
    pub settings: Value,
}

/// A decoder's typed settings struct, named by the type id it is registered under.
pub trait DecoderSettings: Serialize + DeserializeOwned + Default + Clone {
    const TYPE_ID: &'static str;
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

impl ChannelParams {
    /// Settings left to the decoder's defaults.
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

    pub fn of<P: DecoderSettings>(settings: &P) -> Result<Self, String> {
        Self::new(P::TYPE_ID, settings)
    }

    #[must_use]
    pub fn type_id(&self) -> &str {
        &self.type_id
    }

    #[must_use]
    pub fn is<P: DecoderSettings>(&self) -> bool {
        self.type_id == P::TYPE_ID
    }

    /// The settings as the decoder's own struct, absent fields taking its defaults.
    pub fn parse<P: DeserializeOwned>(&self) -> Result<P, String> {
        let parsed = match &self.settings {
            Value::Null => P::deserialize(&empty_object()),
            settings => P::deserialize(settings),
        };
        parsed.map_err(|error| format!("{} settings: {error}", self.type_id))
    }

    /// The settings as `P`, refusing settings that belong to another decoder.
    pub fn decode<P: DecoderSettings>(&self) -> Result<P, String> {
        if self.is::<P>() {
            self.parse()
        } else {
            Err(format!(
                "{} channel got {} settings",
                P::TYPE_ID,
                self.type_id
            ))
        }
    }

    /// The settings as `P`, only when they belong to `P`'s decoder.
    #[must_use]
    pub fn typed<P: DecoderSettings>(&self) -> Option<P> {
        if self.is::<P>() {
            self.parse().ok()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Probe {
        #[serde(default)]
        width_hz: f64,
    }

    impl DecoderSettings for Probe {
        const TYPE_ID: &'static str = "probe";
    }

    #[test]
    fn params_keep_the_tagged_wire_shape() {
        let params = ChannelParams::of(&Probe { width_hz: 5.0 }).expect("probe serializes");
        let json = serde_json::to_value(&params).expect("params serialize");
        assert_eq!(
            json,
            serde_json::json!({"type": "probe", "settings": {"width_hz": 5.0}})
        );
        let back: ChannelParams = serde_json::from_value(json).expect("params parse");
        assert_eq!(back.typed::<Probe>(), Some(Probe { width_hz: 5.0 }));
    }

    #[test]
    fn missing_settings_take_the_decoder_defaults() {
        let params: ChannelParams =
            serde_json::from_value(serde_json::json!({"type": "probe"})).expect("bare params");
        assert_eq!(params.parse::<Probe>(), Ok(Probe::default()));
        assert_eq!(ChannelParams::bare("other").typed::<Probe>(), None);
    }
}
