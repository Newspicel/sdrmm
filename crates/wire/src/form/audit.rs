//! Checks every settings form must pass, shared by the forms of nodes, decoders and
//! processors so each runs the same ones.

use serde_json::{Value, json};

use crate::channel::{Control, LanePick, SettingField, lookup};

fn set_at(settings: &mut Value, key: &str, value: Value) {
    let mut target = settings;
    let parts: Vec<&str> = key.split('.').collect();
    for part in &parts[..parts.len() - 1] {
        target = &mut target[*part];
    }
    target[parts[parts.len() - 1]] = value;
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
            .map_or(Value::Null, |option| option.value.clone()),
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

fn excludes(a: &SettingField, b: &SettingField) -> bool {
    a.when.iter().any(|one| {
        b.when.iter().any(|other| {
            one.key == other.key && one.any_of.iter().all(|value| !other.any_of.contains(value))
        })
    })
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

/// Runs every form check on `form` of `name`, whose defaults are `defaults` and whose owner
/// keeps what `round_trip` hands back of the settings it is given.
///
/// # Panics
///
/// When a check fails, naming the field.
pub fn audit(
    name: &str,
    form: &[SettingField],
    defaults: &Value,
    round_trip: &dyn Fn(Value) -> Value,
) {
    fields_write_settings_kept(name, form, defaults, round_trip);
    defaults_fill_shown_fields(name, form, defaults);
    defaults_sit_in_range(name, form, defaults);
    shared_keys_exclude(name, form);
    references_resolve(name, form);
    fields_survive_the_wire(name, form);
}

fn fields_write_settings_kept(
    name: &str,
    form: &[SettingField],
    defaults: &Value,
    round_trip: &dyn Fn(Value) -> Value,
) {
    for field in form {
        let mut settings = defaults.clone();
        for condition in &field.when {
            let holds = condition.any_of.first().cloned().unwrap_or(json!(1));
            set_at(&mut settings, &condition.key, holds);
        }
        let probe = probe_value(field);
        set_at(&mut settings, &field.key, probe.clone());
        reveal(form, &mut settings);
        let kept = round_trip(settings);
        let read = lookup(&kept, &field.key).cloned().unwrap_or(Value::Null);
        assert!(
            same(&read, &probe),
            "{name} {} wrote {probe} read {read}",
            field.key
        );
    }
}

fn defaults_fill_shown_fields(name: &str, form: &[SettingField], defaults: &Value) {
    for field in form.iter().filter(|field| field.when.is_empty()) {
        let optional = matches!(&field.control, Control::Number(spec) if spec.optional.is_some())
            || matches!(&field.control, Control::Text { optional: true, .. });
        assert!(
            optional || lookup(defaults, &field.key).is_some(),
            "{name} defaults lack {}",
            field.key
        );
    }
}

fn defaults_sit_in_range(name: &str, form: &[SettingField], defaults: &Value) {
    for field in form {
        if let Some(value) = lookup(defaults, &field.key) {
            assert!(
                number_in_range(field, value),
                "{name} default {} out of range",
                field.key
            );
        }
        if let Some(fallback) = &field.fallback {
            assert!(
                number_in_range(field, fallback),
                "{name} fallback {} out of range",
                field.key
            );
        }
    }
}

fn shared_keys_exclude(name: &str, form: &[SettingField]) {
    for (index, field) in form.iter().enumerate() {
        for other in form[index + 1..]
            .iter()
            .filter(|other| other.key == field.key)
        {
            assert!(excludes(field, other), "{name} shows {} twice", field.key);
        }
    }
}

fn references_resolve(name: &str, form: &[SettingField]) {
    for field in form {
        for key in references(field) {
            assert!(
                form.iter().any(|other| other.key == key),
                "{name} {} names missing {key}",
                field.key
            );
        }
    }
}

fn fields_survive_the_wire(name: &str, form: &[SettingField]) {
    for field in form {
        let text = serde_json::to_string(field).unwrap_or_default();
        let back: Result<SettingField, _> = serde_json::from_str(&text);
        assert_eq!(back.ok().as_ref(), Some(field), "{name} {text}");
    }
}
