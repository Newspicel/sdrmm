use serde_json::{Value, json};

use crate::channel::{Control, LanePick, SettingField, lookup};
use crate::patch::{NodeBody, NodeKind};
use crate::processor::beamformer::SteerSource;
use crate::radar::{CfarKind, Illuminator, ReferenceCleaning};

fn formed() -> Vec<(NodeKind, Vec<SettingField>)> {
    NodeKind::ALL
        .iter()
        .map(|kind| (*kind, kind.settings()))
        .filter(|(_, form)| !form.is_empty())
        .collect()
}

fn body_json(kind: NodeKind) -> Value {
    serde_json::to_value(NodeBody::default_for(kind)).expect("body serializes")
}

fn settings_of(body: &Value) -> &Value {
    body.pointer("/data/settings")
        .expect("body carries settings")
}

fn set_at(settings: &mut Value, key: &str, value: Value) {
    let mut target = settings;
    let parts: Vec<&str> = key.split('.').collect();
    for part in &parts[..parts.len() - 1] {
        target = &mut target[*part];
    }
    target[parts[parts.len() - 1]] = value;
}

fn round_trip(kind: NodeKind, settings: Value) -> Value {
    let mut body = body_json(kind);
    body["data"]["settings"] = settings;
    let parsed: NodeBody = serde_json::from_value(body)
        .unwrap_or_else(|error| panic!("{kind} settings refused: {error}"));
    settings_of(&serde_json::to_value(parsed).expect("serializes")).clone()
}

/// `number` as JSON a browser would send: whole numbers carry no fraction.
fn whole(number: f64) -> Value {
    if number.fract() == 0.0 && number.abs() < 1e15 {
        json!(number as i64)
    } else {
        json!(number)
    }
}

/// A value the field accepts that differs from the default where it can.
fn probe_value(field: &SettingField) -> Value {
    match &field.control {
        Control::Number(spec) => whole(spec.max.or(spec.min).unwrap_or(1.0)),
        Control::Choice { options } => options
            .last()
            .map(|option| option.value.clone())
            .unwrap_or(Value::Null),
        Control::Toggle => json!(true),
        Control::Text { .. } | Control::SerialPort { .. } => json!("probe"),
        Control::Lanes {
            pick: LanePick::One,
            ..
        } => json!(3),
        Control::Lanes {
            pick: LanePick::List,
            ..
        } => json!([2, 3]),
        Control::Lanes {
            pick: LanePick::Mask,
            ..
        } => json!(8),
        Control::Numbers { spec, .. } => json!([whole(spec.min.unwrap_or(0.0))]),
        Control::Preset { .. } | Control::Live { .. } | Control::List { .. } => Value::Null,
    }
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (
                a.as_f64().unwrap_or(f64::NAN),
                b.as_f64().unwrap_or(f64::NAN),
            );
            (a - b).abs() <= 1e-6 * a.abs().max(1.0)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        _ => a == b,
    }
}

fn shows(field: &SettingField, settings: &Value) -> bool {
    field.when.iter().all(|condition| {
        let value = lookup(settings, &condition.key).unwrap_or(&Value::Null);
        condition.any_of.is_empty() && !value.is_null() || condition.any_of.contains(value)
    })
}

/// Fills the fallback of every unset field that shows, as the face does on a change.
fn reveal(form: &[SettingField], settings: &mut Value) {
    for field in form {
        let unset = lookup(settings, &field.key).is_none_or(Value::is_null);
        if let Some(fallback) = field
            .fallback
            .clone()
            .filter(|_| unset && shows(field, settings))
        {
            set_at(settings, &field.key, fallback);
        }
    }
}

#[test]
fn every_field_writes_a_setting_its_params_keep() {
    for (kind, form) in formed() {
        for field in &form {
            let mut settings = settings_of(&body_json(kind)).clone();
            for condition in &field.when {
                let holds = condition.any_of.first().cloned().unwrap_or(json!(1));
                set_at(&mut settings, &condition.key, holds);
            }
            let probe = probe_value(field);
            set_at(&mut settings, &field.key, probe.clone());
            reveal(&form, &mut settings);
            let kept = round_trip(kind, settings);
            let read = lookup(&kept, &field.key).cloned().unwrap_or(Value::Null);
            assert!(
                same(&read, &probe),
                "{kind} {} wrote {probe} read {read}",
                field.key
            );
        }
    }
}

#[test]
fn defaults_fill_every_field_they_show() {
    for (kind, form) in formed() {
        let body = body_json(kind);
        let settings = settings_of(&body);
        for field in form.iter().filter(|field| field.when.is_empty()) {
            let optional = matches!(&field.control, Control::Number(spec) if spec.optional.is_some())
                || matches!(&field.control, Control::Text { optional: true, .. });
            assert!(
                optional || lookup(settings, &field.key).is_some(),
                "{kind} defaults lack {}",
                field.key
            );
        }
    }
}

fn number_in_range(field: &SettingField, value: &Value) -> bool {
    let Control::Number(spec) = &field.control else {
        return true;
    };
    let Some(number) = value.as_f64() else {
        return spec.optional.is_some();
    };
    let slack = 1e-6 * number.abs().max(1.0);
    spec.min.is_none_or(|min| number >= min - slack)
        && spec.max.is_none_or(|max| number <= max + slack)
}

#[test]
fn defaults_and_fallbacks_sit_inside_their_ranges() {
    for (kind, form) in formed() {
        let body = body_json(kind);
        let settings = settings_of(&body);
        for field in &form {
            if let Some(value) = lookup(settings, &field.key) {
                assert!(
                    number_in_range(field, value),
                    "{kind} default {} out of range",
                    field.key
                );
            }
            if let Some(fallback) = &field.fallback {
                assert!(
                    number_in_range(field, fallback),
                    "{kind} fallback {} out of range",
                    field.key
                );
            }
        }
    }
}

fn excludes(a: &SettingField, b: &SettingField) -> bool {
    a.when.iter().any(|one| {
        b.when.iter().any(|other| {
            one.key == other.key && one.any_of.iter().all(|value| !other.any_of.contains(value))
        })
    })
}

#[test]
fn fields_sharing_a_key_never_show_together() {
    for (kind, form) in formed() {
        for (index, field) in form.iter().enumerate() {
            for other in form[index + 1..]
                .iter()
                .filter(|other| other.key == field.key)
            {
                assert!(excludes(field, other), "{kind} shows {} twice", field.key);
            }
        }
    }
}

fn references(field: &SettingField) -> Vec<&str> {
    let mut keys: Vec<&str> = field
        .when
        .iter()
        .map(|condition| condition.key.as_str())
        .collect();
    keys.extend(field.at_most.as_deref());
    if let Control::Lanes {
        except: Some(except),
        ..
    } = &field.control
    {
        keys.push(except);
    }
    keys
}

#[test]
fn every_reference_names_a_field_of_the_same_form() {
    for (kind, form) in formed() {
        for field in &form {
            for key in references(field) {
                assert!(
                    form.iter().any(|other| other.key == key),
                    "{kind} {} names missing {key}",
                    field.key
                );
            }
        }
    }
}

#[test]
fn every_processor_kind_publishes_its_form() {
    for kind in NodeKind::ALL
        .iter()
        .filter(|kind| NodeBody::default_for(**kind).is_array_processor())
        .filter(|kind| !kind.info().needs_processor_type)
    {
        assert!(!kind.settings().is_empty(), "{kind} has no form");
        assert_eq!(kind.info().settings, kind.settings());
    }
}

#[test]
fn a_variant_switched_back_keeps_parsing_with_its_old_fields() {
    let steer: SteerSource =
        serde_json::from_value(json!({"kind": "wired", "azimuth_deg": 10.0})).expect("steer");
    assert_eq!(steer, SteerSource::Wired);
    let illuminator: Illuminator =
        serde_json::from_value(json!({"kind": "fm", "bandwidth_hz": 8e6})).expect("illuminator");
    assert_eq!(illuminator, Illuminator::Fm);
    let cleaning: ReferenceCleaning =
        serde_json::from_value(json!({"kind": "off", "taps": 16, "step": 0.001}))
            .expect("cleaning");
    assert_eq!(cleaning, ReferenceCleaning::Off);
    let cfar: CfarKind = serde_json::from_value(json!({"kind": "ca", "rank": 0.75})).expect("cfar");
    assert_eq!(cfar, CfarKind::Ca);
}

#[test]
fn every_form_survives_the_wire() {
    for (kind, form) in formed() {
        for field in form {
            let text = serde_json::to_string(&field).expect("serializes");
            let back: SettingField = serde_json::from_str(&text).expect("parses");
            assert_eq!(back, field, "{kind} {text}");
        }
    }
}
