use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use super::choices::{ParamOption, SettingChoice, of, option};

/// One setting on a decoder's form, keyed by its field in the decoder's settings object. A
/// dotted key (`header.length`) names a field of a nested object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SettingField {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub control: Control,
    /// What the control shows and commits first while the setting is unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Value>,
    /// Shown only while every one of these holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<Condition>,
    /// Values that mean the setting is off, so the control renders muted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quiet: Vec<Value>,
}

/// Shows a field only while another field holds one of these values; with none listed, while
/// that field is set at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Condition {
    pub key: String,
    pub any_of: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    Choice {
        options: Vec<ParamOption>,
    },
    Number(NumberSpec),
    Toggle,
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default)]
        optional: bool,
        /// Masked while shown, as a key is.
        #[serde(default)]
        secret: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_len: Option<usize>,
    },
    /// Named sets of values for several settings at once; the field's key is only its name.
    /// The option whose values all match the settings shows as chosen.
    Preset {
        options: Vec<PresetOption>,
    },
    /// Options the running decoder publishes in its status under this field's key; until it
    /// has any, the fallback control stands in.
    Live {
        #[schema(no_recursion)]
        fallback: Box<Control>,
    },
    List {
        #[schema(no_recursion)]
        item: Vec<SettingField>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        empty: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PresetOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Setting key to value, written together when chosen.
    #[schema(value_type = std::collections::BTreeMap<String, Value>)]
    pub values: serde_json::Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NumberSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Unset is allowed and shows this placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<ParamOption>,
}

impl SettingField {
    #[must_use]
    pub fn new(key: &str, label: &str, control: impl Into<Control>) -> Self {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            title: None,
            control: control.into(),
            fallback: None,
            when: Vec::new(),
            quiet: Vec::new(),
        }
    }

    #[must_use]
    pub fn title(mut self, title: &str) -> Self {
        self.title = Some(title.to_owned());
        self
    }

    /// The polarity flip every FSK decoder offers, keyed `invert`.
    #[must_use]
    pub fn invert() -> Self {
        Self::new("invert", "Invert", Control::Toggle)
            .title("Flip the signal's polarity; try it when nothing decodes")
    }

    /// A choice of channel widths, keyed `bandwidth_hz`.
    #[must_use]
    pub fn bandwidth(widths_hz: &[f64]) -> Self {
        Self::new(
            "bandwidth_hz",
            "BW",
            Control::values(widths_hz, crate::units::hertz),
        )
        .title("Channel bandwidth")
    }

    #[must_use]
    pub fn fallback(mut self, value: impl Serialize) -> Self {
        self.fallback = serde_json::to_value(value).ok();
        self
    }

    #[must_use]
    pub fn when<V: Serialize>(mut self, key: &str, any_of: impl IntoIterator<Item = V>) -> Self {
        self.when.push(Condition {
            key: key.to_owned(),
            any_of: any_of
                .into_iter()
                .filter_map(|value| serde_json::to_value(value).ok())
                .collect(),
        });
        self
    }

    #[must_use]
    pub fn quiet(mut self, value: impl Serialize) -> Self {
        self.quiet.extend(serde_json::to_value(value).ok());
        self
    }
}

impl Control {
    #[must_use]
    pub fn choice<T: SettingChoice>() -> Self {
        Self::Choice {
            options: T::all().iter().copied().map(of).collect(),
        }
    }

    /// Every variant of `T`, led by an unset option labelled `none`.
    #[must_use]
    pub fn optional_choice<T: SettingChoice>(none: &str) -> Self {
        let mut options = vec![option(Value::Null, none.to_owned(), None)];
        options.extend(T::all().iter().copied().map(of));
        Self::Choice { options }
    }

    #[must_use]
    pub fn values<T: Copy + Serialize>(values: &[T], label: impl Fn(T) -> String) -> Self {
        Self::Choice {
            options: values
                .iter()
                .filter_map(|value| {
                    Some(option(
                        serde_json::to_value(value).ok()?,
                        label(*value),
                        None,
                    ))
                })
                .collect(),
        }
    }

    #[must_use]
    pub fn text(placeholder: &str) -> Self {
        Self::Text {
            placeholder: Some(placeholder.to_owned()).filter(|text| !text.is_empty()),
            optional: false,
            secret: false,
            max_len: None,
        }
    }

    #[must_use]
    pub fn optional_text(placeholder: &str) -> Self {
        Self::Text {
            placeholder: Some(placeholder.to_owned()).filter(|text| !text.is_empty()),
            optional: true,
            secret: false,
            max_len: None,
        }
    }

    /// A text of at most `max_len` characters, masked when `secret`.
    #[must_use]
    pub fn bounded_text(placeholder: &str, max_len: usize, secret: bool) -> Self {
        Self::Text {
            placeholder: Some(placeholder.to_owned()).filter(|text| !text.is_empty()),
            optional: false,
            secret,
            max_len: Some(max_len),
        }
    }

    #[must_use]
    pub fn preset(options: Vec<PresetOption>) -> Self {
        Self::Preset { options }
    }

    #[must_use]
    pub fn live(fallback: impl Into<Control>) -> Self {
        Self::Live {
            fallback: Box::new(fallback.into()),
        }
    }

    #[must_use]
    pub fn list(item: Vec<SettingField>, empty: &str) -> Self {
        Self::List {
            item,
            empty: Some(empty.to_owned()).filter(|text| !text.is_empty()),
        }
    }
}

impl PresetOption {
    /// `values` as key and value pairs; a value that cannot be serialized is left out.
    #[must_use]
    pub fn new(label: &str, values: &[(&str, Value)]) -> Self {
        Self {
            label: label.to_owned(),
            title: None,
            values: values
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        }
    }

    #[must_use]
    pub fn title(mut self, title: &str) -> Self {
        self.title = Some(title.to_owned());
        self
    }
}

/// The value at a dotted `key` inside `settings`.
#[must_use]
pub fn lookup<'a>(settings: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.')
        .try_fold(settings, |value, part| value.get(part))
}

impl NumberSpec {
    #[must_use]
    pub fn new(min: f64, max: f64, step: f64) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
            step: Some(step),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn hz(min: f64, max: f64, step: f64) -> Self {
        Self::new(min, max, step).unit("Hz")
    }

    #[must_use]
    pub fn unit(mut self, unit: &str) -> Self {
        self.unit = Some(unit.to_owned());
        self
    }

    #[must_use]
    pub fn optional(mut self, placeholder: &str) -> Self {
        self.optional = Some(placeholder.to_owned());
        self
    }

    #[must_use]
    pub fn presets(mut self, values: &[f64], label: impl Fn(f64) -> String) -> Self {
        self.presets = values
            .iter()
            .map(|value| option(Value::from(*value), label(*value), None))
            .collect();
        self
    }

    fn check(&self, value: f64) -> bool {
        value.is_finite()
            && self.min.is_none_or(|min| value >= min)
            && self.max.is_none_or(|max| value <= max)
    }
}

impl From<NumberSpec> for Control {
    fn from(spec: NumberSpec) -> Self {
        Self::Number(spec)
    }
}

/// Every number in `settings` checked against the range its form field publishes.
pub(crate) fn check(type_id: &str, settings: &Value, form: &[SettingField]) -> Result<(), String> {
    for field in form {
        let Some(value) = lookup(settings, &field.key).filter(|value| !value.is_null()) else {
            continue;
        };
        check_field(type_id, field, value)?;
    }
    Ok(())
}

fn check_field(type_id: &str, field: &SettingField, value: &Value) -> Result<(), String> {
    check_control(type_id, &field.key, &field.control, value)
}

fn check_control(type_id: &str, key: &str, control: &Control, value: &Value) -> Result<(), String> {
    match control {
        Control::Number(spec) => check_number(type_id, key, spec, value),
        Control::Live { fallback } => check_control(type_id, key, fallback, value),
        Control::List { item, .. } => value
            .as_array()
            .into_iter()
            .flatten()
            .try_for_each(|entry| check(type_id, entry, item)),
        Control::Text {
            max_len: Some(max), ..
        } => match value.as_str() {
            Some(text) if text.chars().count() > *max => {
                Err(format!("{type_id} {key} is longer than {max} characters"))
            }
            _ => Ok(()),
        },
        Control::Choice { .. }
        | Control::Toggle
        | Control::Text { .. }
        | Control::Preset { .. } => Ok(()),
    }
}

fn check_number(type_id: &str, key: &str, spec: &NumberSpec, value: &Value) -> Result<(), String> {
    let bounded = spec.min.is_some() || spec.max.is_some();
    if !bounded || value.as_f64().is_some_and(|number| spec.check(number)) {
        return Ok(());
    }
    Err(format!(
        "{type_id} {key} must be in {}..={}, got {value}",
        spec.min.unwrap_or(f64::NEG_INFINITY),
        spec.max.unwrap_or(f64::INFINITY),
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn form() -> Vec<SettingField> {
        vec![
            SettingField::new("baud", "Baud", NumberSpec::new(10.0, 1_200.0, 0.05)),
            SettingField::new(
                "keys",
                "Keys",
                Control::list(
                    vec![SettingField::new(
                        "slot",
                        "Slot",
                        NumberSpec::new(0.0, 7.0, 1.0),
                    )],
                    "",
                ),
            ),
        ]
    }

    #[test]
    fn numbers_inside_their_range_pass() {
        assert_eq!(
            check(
                "rtty",
                &json!({"baud": 45.45, "keys": [{"slot": 3}]}),
                &form()
            ),
            Ok(())
        );
        assert_eq!(check("rtty", &json!({}), &form()), Ok(()));
    }

    #[test]
    fn numbers_outside_their_range_name_the_field() {
        let error = check("rtty", &json!({"baud": 5.0}), &form()).expect_err("too slow");
        assert!(error.contains("rtty baud"), "{error}");
        assert!(check("rtty", &json!({"keys": [{"slot": 9}]}), &form()).is_err());
    }
}
