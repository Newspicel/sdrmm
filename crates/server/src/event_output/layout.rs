use sdrmm_wire::{
    DecodedRecord,
    message::fill,
    present::{FieldValue, Presentation},
};

/// The operator's layout for one event.
pub(super) fn written(
    layout: &str,
    record: &DecodedRecord,
    view: &Presentation,
    encoded: &serde_json::Value,
) -> String {
    fill(layout, |name| value(name, record, view, encoded))
}

fn value(
    name: &str,
    record: &DecodedRecord,
    view: &Presentation,
    encoded: &serde_json::Value,
) -> Option<String> {
    match name {
        "headline" => Some(super::headline(record, view)),
        "summary" => Some(view.summary.clone()),
        "station" => view.entity.clone(),
        "freq" => Some(megahertz(record.freq_hz)),
        "time" => Some(record.at.clone()),
        "kind" => Some(record.event.kind().name()),
        "channel" => Some(record.channel.to_string()),
        _ => name
            .strip_prefix("record.")
            .map_or_else(|| field(view, name), |path| at_path(encoded, path)),
    }
}

fn megahertz(hz: f64) -> String {
    let text = format!("{:.6}", hz / 1e6);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn field(view: &Presentation, name: &str) -> Option<String> {
    view.fields
        .iter()
        .find(|field| field.key == name || field.label.eq_ignore_ascii_case(name))
        .map(|field| field_text(&field.value))
}

fn field_text(value: &FieldValue) -> String {
    match value {
        FieldValue::Text(text) | FieldValue::Time(text) => text.clone(),
        FieldValue::Number { value, digits, .. } => format!("{value:.*}", usize::from(*digits)),
        FieldValue::Flag(flag) => if *flag { "yes" } else { "no" }.to_owned(),
        FieldValue::Position(at) => format!("{:.5}, {:.5}", at.lat, at.lon),
    }
}

fn at_path(encoded: &serde_json::Value, path: &str) -> Option<String> {
    let found = path
        .split('.')
        .try_fold(encoded, |value, step| match value {
            serde_json::Value::Array(items) => items.get(step.parse::<usize>().ok()?),
            _ => value.get(step),
        })?;
    match found {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}
