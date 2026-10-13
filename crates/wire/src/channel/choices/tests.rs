use std::collections::BTreeSet;

use utoipa::{
    PartialSchema, ToSchema,
    openapi::{RefOr, Schema},
};

use super::*;
use crate::ChannelSettings;

type Schemas = Vec<(String, RefOr<Schema>)>;

fn variants() -> Vec<(String, Option<String>)> {
    let RefOr::T(Schema::OneOf(variants)) = ChannelParams::schema() else {
        return Vec::new();
    };
    variants
        .items
        .iter()
        .filter_map(|variant| {
            let RefOr::T(Schema::Object(object)) = variant else {
                return None;
            };
            let RefOr::T(Schema::Object(tag)) = object.properties.get("type")? else {
                return None;
            };
            let tag = tag.enum_values.as_ref()?.first()?.as_str()?.to_owned();
            Some((tag, object.properties.get("settings").and_then(ref_name)))
        })
        .collect()
}

fn all_params() -> Vec<ChannelParams> {
    variants()
        .iter()
        .filter_map(|(tag, _)| ChannelParams::default_for(tag))
        .collect()
}

fn schemas() -> Schemas {
    let mut schemas = Vec::new();
    ChannelParams::schemas(&mut schemas);
    schemas
}

fn named<'a>(schemas: &'a Schemas, name: &str) -> Option<&'a Schema> {
    schemas
        .iter()
        .find(|(candidate, _)| candidate == name)
        .and_then(|(_, schema)| match schema {
            RefOr::T(schema) => Some(schema),
            RefOr::Ref(_) => None,
        })
}

fn enum_values(schema: &Schema) -> Option<BTreeSet<String>> {
    let Schema::Object(object) = schema else {
        return None;
    };
    let values = object.enum_values.as_ref()?;
    Some(
        values
            .iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect(),
    )
}

fn ref_name(schema: &RefOr<Schema>) -> Option<String> {
    let reference = match schema {
        RefOr::Ref(reference) => reference,
        RefOr::T(Schema::OneOf(one_of)) => one_of.items.iter().find_map(|item| match item {
            RefOr::Ref(reference) => Some(reference),
            RefOr::T(_) => None,
        })?,
        RefOr::T(Schema::AllOf(all_of)) => all_of.items.iter().find_map(|item| match item {
            RefOr::Ref(reference) => Some(reference),
            RefOr::T(_) => None,
        })?,
        RefOr::T(_) => return None,
    };
    reference.ref_location.rsplit('/').next().map(str::to_owned)
}

fn settings_schema(params: &ChannelParams) -> Option<String> {
    variants()
        .into_iter()
        .find(|(tag, _)| tag == params.type_id())
        .and_then(|(_, settings)| settings)
}

fn enum_fields(schemas: &Schemas, settings: &str) -> Vec<(String, BTreeSet<String>)> {
    let Some(Schema::Object(object)) = named(schemas, settings) else {
        return Vec::new();
    };
    object
        .properties
        .iter()
        .filter_map(|(field, schema)| {
            let name = ref_name(schema)?;
            let values = enum_values(named(schemas, &name)?)?;
            Some((field.clone(), values))
        })
        .collect()
}

fn offered(choice: &ParamChoice) -> BTreeSet<String> {
    choice
        .options
        .iter()
        .filter_map(|option| option.value.as_str().map(str::to_owned))
        .collect()
}

#[test]
fn every_enum_valued_setting_offers_every_value() {
    let schemas = schemas();
    let mut checked = 0;
    for params in all_params() {
        let Some(settings) = settings_schema(&params) else {
            continue;
        };
        let choices = params.choices();
        for (field, values) in enum_fields(&schemas, &settings) {
            if values.len() < 2 {
                continue;
            }
            let choice = choices.iter().find(|choice| choice.name == field);
            let choice =
                choice.unwrap_or_else(|| panic!("{} {field} has no choice", params.type_id()));
            assert_eq!(offered(choice), values, "{} {field}", params.type_id());
            checked += 1;
        }
    }
    assert!(checked > 40, "only {checked} enum settings found");
}

fn parent(name: &str) -> serde_json::Value {
    match name {
        "implicit_header" => serde_json::json!({ "length": 16 }),
        _ => serde_json::Value::Null,
    }
}

fn setting_at(settings: &serde_json::Value, name: &str) -> serde_json::Value {
    name.split('.')
        .try_fold(settings, |slot, part| slot.get(part))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

#[test]
fn every_option_is_a_setting_the_channel_accepts() {
    for params in all_params() {
        let base = serde_json::to_value(&params).unwrap();
        for choice in params.choices() {
            assert!(!choice.options.is_empty(), "{}", choice.name);
            for option in &choice.options {
                assert!(!option.label.is_empty(), "{} {}", choice.name, option.value);
                let mut json = base.clone();
                let mut slot = &mut json["settings"];
                for part in choice.name.split('.') {
                    if slot.get(part).is_none() {
                        slot[part] = parent(part);
                    }
                    slot = &mut slot[part];
                }
                *slot = option.value.clone();
                let parsed = serde_json::from_value::<ChannelParams>(json);
                let kept = parsed
                    .ok()
                    .and_then(|parsed| serde_json::to_value(parsed).ok())
                    .map(|back| setting_at(&back["settings"], &choice.name));
                assert_eq!(
                    kept,
                    Some(option.value.clone()),
                    "{} {} {}",
                    params.type_id(),
                    choice.name,
                    option.value
                );
            }
        }
    }
}

#[test]
fn optional_settings_lead_with_their_unset_option() {
    let sstv = ChannelSettings::default_for("sstv")
        .unwrap()
        .params
        .choices();
    let mode = sstv.iter().find(|choice| choice.name == "mode").unwrap();
    assert_eq!(mode.options[0].value, serde_json::Value::Null);
    assert_eq!(mode.options[0].label, "Follow VIS");
    assert_eq!(mode.options.len(), SstvMode::ALL.len() + 1);
}

#[test]
fn numeric_choices_carry_numbers_and_units() {
    let nfm = ChannelSettings::default_for("nfm")
        .unwrap()
        .params
        .choices();
    let ctcss = nfm.iter().find(|choice| choice.name == "ctcss_hz").unwrap();
    assert_eq!(ctcss.options.len(), CTCSS_TONES_HZ.len());
    assert_eq!(ctcss.options[8].value, serde_json::json!(88.5));
    assert_eq!(ctcss.options[8].label, "88.5 Hz");
    let dcs = nfm.iter().find(|choice| choice.name == "dcs_code").unwrap();
    assert_eq!(dcs.options[0].label, "023");
}

#[test]
fn titles_explain_the_terse_labels() {
    let tetrapol = ChannelSettings::default_for("tetrapol")
        .unwrap()
        .params
        .choices();
    let vhf = &tetrapol[0].options[1];
    assert_eq!(vhf.value, "vhf");
    assert_eq!(vhf.title.as_deref(), Some("70 to 87 MHz systems"));
    assert!(SstvMode::Pd50.choice_title().is_none());
}

#[test]
fn every_channel_type_is_walked() {
    assert_eq!(all_params().len(), variants().len());
    assert!(all_params().len() > 60);
}
